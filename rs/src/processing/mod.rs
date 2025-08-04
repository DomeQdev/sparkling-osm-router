use crate::core::errors::{GraphError, Result};
use crate::core::types::{Node, Profile, Relation, RelationMember, Way};
use crate::graph::{ProcessedGraph, RouteNode, WayInfo};
use crate::routing::distance;
use rustc_hash::FxHashMap;
use std::collections::HashMap;

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
enum TurnRestriction {
    Inapplicable,
    Prohibitory,
    Mandatory,
}

struct StringInterner {
    map: FxHashMap<String, u32>,
    vec: Vec<String>,
}

impl StringInterner {
    fn new() -> Self {
        Self {
            map: FxHashMap::default(),
            vec: Vec::new(),
        }
    }

    fn intern(&mut self, s: &str) -> u32 {
        if let Some(id) = self.map.get(s) {
            return *id;
        }
        let id = self.vec.len() as u32;
        let s_owned = s.to_owned();
        self.map.insert(s_owned.clone(), id);
        self.vec.push(s_owned);
        id
    }
}

struct InternedProfile {
    key: u32,
    penalties: FxHashMap<u32, f64>,
    default_penalty: Option<u32>,
    access_tags: Vec<u32>,
    oneway_tags: Vec<u32>,
    except_tags: Vec<u32>,
}

pub struct GraphBuilder<'a> {
    raw_nodes: &'a HashMap<i64, Node>,
    raw_ways: &'a HashMap<i64, Way>,
    raw_relations: &'a HashMap<i64, Relation>,

    interner: StringInterner,
    profile: InternedProfile,

    node_map: FxHashMap<i64, u32>,
    next_internal_id: u32,
    nodes: Vec<RouteNode>,
    temp_edges: FxHashMap<u32, FxHashMap<u32, u16>>,
    processed_ways: Vec<(i64, Vec<i64>, FxHashMap<u32, u32>)>,

    phantom_node_counter: i64,
    way_node_map: FxHashMap<i64, Vec<i64>>,

    phantom_via_map: FxHashMap<(u32, i64), u32>,
}

impl<'a> GraphBuilder<'a> {
    pub fn new(
        profile: &'a Profile,
        raw_nodes: &'a HashMap<i64, Node>,
        raw_ways: &'a HashMap<i64, Way>,
        raw_relations: &'a HashMap<i64, Relation>,
    ) -> Self {
        let mut interner = StringInterner::new();

        let interned_profile = InternedProfile {
            key: interner.intern(&profile.key),
            penalties: profile
                .penalties
                .penalties
                .iter()
                .map(|(k, v)| (interner.intern(k), *v))
                .collect(),
            default_penalty: profile.penalties.default,
            access_tags: profile
                .access_tags
                .iter()
                .map(|tag| interner.intern(tag))
                .collect(),
            oneway_tags: profile
                .oneway_tags
                .iter()
                .map(|tag| interner.intern(tag))
                .collect(),
            except_tags: profile
                .except_tags
                .iter()
                .map(|tag| interner.intern(tag))
                .collect(),
        };

        GraphBuilder {
            raw_nodes,
            raw_ways,
            raw_relations,
            interner,
            profile: interned_profile,
            node_map: FxHashMap::default(),
            next_internal_id: 0,
            nodes: Vec::new(),
            temp_edges: FxHashMap::default(),
            processed_ways: Vec::new(),
            phantom_node_counter: 0x0008_0000_0000_0000,
            way_node_map: FxHashMap::default(),
            phantom_via_map: FxHashMap::default(),
        }
    }

    pub fn build(mut self) -> Result<ProcessedGraph> {
        for way in self.raw_ways.values() {
            if self.is_way_usable(way) {
                let valid_nodes: Vec<i64> = way
                    .node_refs
                    .iter()
                    .filter(|&&id| self.raw_nodes.contains_key(&id))
                    .cloned()
                    .collect();
                if valid_nodes.len() < 2 {
                    continue;
                }

                let interned_tags = way
                    .tags
                    .iter()
                    .map(|(k, v)| (self.interner.intern(k), self.interner.intern(v)))
                    .collect();

                self.processed_ways
                    .push((way.id, valid_nodes.clone(), interned_tags));
                self.way_node_map.insert(way.id, valid_nodes.clone());
                for &osm_node_id in &valid_nodes {
                    self.get_or_create_internal_node(osm_node_id, osm_node_id);
                }
            }
        }

        for way in self.raw_ways.values() {
            self.add_way(way);
        }

        for relation in self.raw_relations.values() {
            if let Err(e) = self.add_relation(relation) {
                log::warn!(
                    "Skipping turn restriction {}: {}",
                    relation.id,
                    e.to_string()
                );
            }
        }

        self.finalize_graph()
    }

    fn finalize_graph(self) -> Result<ProcessedGraph> {
        let mut graph = ProcessedGraph::new();
        graph.ways = self
            .processed_ways
            .into_iter()
            .map(|(osm_id, node_refs, tags)| WayInfo {
                osm_id,
                node_ids: node_refs
                    .iter()
                    .map(|osm_node_id| *self.node_map.get(osm_node_id).unwrap())
                    .collect(),
                tags,
            })
            .collect();
        graph.nodes = self.nodes;
        graph.node_id_map = self.node_map;
        graph.string_interner = self.interner.vec;

        let node_count = graph.nodes.len();
        graph.offsets.resize(node_count + 1, 0);

        let mut edge_count = 0;
        for internal_id in 0..node_count as u32 {
            graph.offsets[internal_id as usize] = edge_count;
            if let Some(neighbors) = self.temp_edges.get(&internal_id) {
                for (&target, &cost) in neighbors.iter() {
                    graph.edges.push((target, cost));
                }
                edge_count += neighbors.len();
            }
        }
        graph.offsets[node_count] = edge_count;

        graph.build_indices();
        Ok(graph)
    }

    fn get_or_create_internal_node(&mut self, osm_node_id: i64, external_id: i64) -> u32 {
        if let Some(id) = self.node_map.get(&osm_node_id) {
            return *id;
        }

        let internal_id = self.next_internal_id;
        self.node_map.insert(osm_node_id, internal_id);
        self.next_internal_id += 1;

        let raw_node = self.raw_nodes.get(&external_id).unwrap();
        let interned_tags = raw_node
            .tags
            .iter()
            .map(|(k, v)| (self.interner.intern(k), self.interner.intern(v)))
            .collect();

        self.nodes.push(RouteNode {
            id: internal_id,
            external_id,
            lat: raw_node.lat as f32,
            lon: raw_node.lon as f32,
            tags: interned_tags,
        });

        internal_id
    }

    fn is_way_usable(&mut self, way: &Way) -> bool {
        let interned_tags: FxHashMap<u32, u32> = way
            .tags
            .iter()
            .map(|(k, v)| (self.interner.intern(k), self.interner.intern(v)))
            .collect();

        let penalty = self.get_way_penalty(&interned_tags);
        if penalty.is_none() || !penalty.unwrap().is_finite() || penalty.unwrap() < 1.0 {
            return false;
        }
        let (forward, backward) = self.get_way_direction(&interned_tags);
        !(!forward && !backward)
    }

    fn add_way(&mut self, way: &Way) {
        let interned_tags: FxHashMap<u32, u32> = way
            .tags
            .iter()
            .map(|(k, v)| (self.interner.intern(k), self.interner.intern(v)))
            .collect();

        let penalty = match self.get_way_penalty(&interned_tags) {
            Some(p) if p.is_finite() && p >= 1.0 => p,
            _ => return,
        };
        let (forward, backward) = self.get_way_direction(&interned_tags);
        if !forward && !backward {
            return;
        }

        if let Some(valid_nodes) = self.way_node_map.get(&way.id) {
            for window in valid_nodes.windows(2) {
                let (from_osm, to_osm) = (window[0], window[1]);
                let from_node = self.raw_nodes.get(&from_osm).unwrap();
                let to_node = self.raw_nodes.get(&to_osm).unwrap();
                let distance = distance(
                    from_node.lat as f32,
                    from_node.lon as f32,
                    to_node.lat as f32,
                    to_node.lon as f32,
                );
                let cost = (distance * penalty as f32 * 1000.0) as u16;

                let from_id = *self.node_map.get(&from_osm).unwrap();
                let to_id = *self.node_map.get(&to_osm).unwrap();

                if forward {
                    self.temp_edges
                        .entry(from_id)
                        .or_default()
                        .insert(to_id, cost);
                }
                if backward {
                    self.temp_edges
                        .entry(to_id)
                        .or_default()
                        .insert(from_id, cost);
                }
            }
        }
    }

    fn add_relation(&mut self, rel: &Relation) -> Result<()> {
        let interned_tags: FxHashMap<u32, u32> = rel
            .tags
            .iter()
            .map(|(k, v)| (self.interner.intern(k), self.interner.intern(v)))
            .collect();
        let restriction_type = self.get_restriction_type(&interned_tags);

        if restriction_type == TurnRestriction::Inapplicable {
            return Ok(());
        }

        let mut member_nodes: Vec<Vec<i64>> = Vec::new();
        for m in self.get_ordered_restriction_members(rel)? {
            member_nodes.push(self.restriction_member_to_nodes(rel, m)?);
        }

        let nodes_path = self.flatten_restriction_nodes(rel, member_nodes)?;

        if nodes_path.len() < 2 {
            return Err(GraphError::InvalidOsmData(
                "Restriction path too short".into(),
            ));
        }

        let is_mandatory = restriction_type == TurnRestriction::Mandatory;
        self.store_restriction(&nodes_path, is_mandatory)
    }

    fn store_restriction(&mut self, osm_nodes: &[i64], is_mandatory: bool) -> Result<()> {
        if osm_nodes.len() < 2 {
            return Err(GraphError::InvalidOsmData(
                "Restriction path too short for storing".into(),
            ));
        }

        let mut cloned_path_internal_ids: Vec<u32> = Vec::with_capacity(osm_nodes.len());
        let mut previous_internal_id = *self.node_map.get(&osm_nodes[0]).ok_or_else(|| {
            GraphError::InvalidOsmData("Restriction 'from' node not in graph".into())
        })?;
        cloned_path_internal_ids.push(previous_internal_id);

        for (i, current_osm_id) in osm_nodes.iter().enumerate().skip(1) {
            let (target_internal_id, edge_cost) = self
            .temp_edges
            .get(&previous_internal_id)
            .and_then(|edges| {
                edges.iter().find(|(&id, _)| self.nodes[id as usize].external_id == *current_osm_id)
            })
            .map(|(&id, &cost)| (id, cost))
            .ok_or_else(|| {
                GraphError::InvalidOsmData(format!(
                    "Disjointed restriction: no edge from OSM node {} (internal {}) to OSM node {}",
                    self.nodes[previous_internal_id as usize].external_id, previous_internal_id, current_osm_id
                ))
            })?;

            let target_node = &self.nodes[target_internal_id as usize];
            let target_is_phantom =
                self.node_map.get(&target_node.external_id) != Some(&target_node.id);
            let is_last_node_in_path = i == osm_nodes.len() - 1;

            let new_node_id;
            if !is_last_node_in_path && !target_is_phantom {
                let from_for_key = *cloned_path_internal_ids.last().unwrap();
                let via_for_key = target_node.external_id;

                if let Some(existing_phantom_id) =
                    self.phantom_via_map.get(&(from_for_key, via_for_key))
                {
                    new_node_id = *existing_phantom_id;
                } else {
                    let cloned_node_id = self.create_phantom_node(*current_osm_id)?;

                    if let Some(original_edges) = self.temp_edges.get(&target_internal_id).cloned()
                    {
                        self.temp_edges.insert(cloned_node_id, original_edges);
                    }

                    self.phantom_via_map
                        .insert((from_for_key, via_for_key), cloned_node_id);
                    new_node_id = cloned_node_id;
                }

                let prev_edges = self.temp_edges.get_mut(&previous_internal_id).unwrap();
                prev_edges.remove(&target_internal_id);
                prev_edges.insert(new_node_id, edge_cost);
            } else {
                new_node_id = target_internal_id;
            }

            cloned_path_internal_ids.push(new_node_id);
            previous_internal_id = new_node_id;
        }

        if is_mandatory {
            for i in 1..cloned_path_internal_ids.len() - 1 {
                let via_node_id = cloned_path_internal_ids[i];
                let mandated_target_id = cloned_path_internal_ids[i + 1];

                if let Some(edges) = self.temp_edges.get_mut(&via_node_id) {
                    edges.retain(|&target, _| target == mandated_target_id);
                }
            }
        } else {
            if cloned_path_internal_ids.len() >= 2 {
                let from_id = cloned_path_internal_ids[cloned_path_internal_ids.len() - 2];
                let to_id = cloned_path_internal_ids[cloned_path_internal_ids.len() - 1];

                if let Some(edges) = self.temp_edges.get_mut(&from_id) {
                    edges.remove(&to_id);
                }
            }
        }

        Ok(())
    }

    fn create_phantom_node(&mut self, original_node_id: i64) -> Result<u32> {
        self.phantom_node_counter += 1;
        let phantom_osm_id = self.phantom_node_counter;
        let phantom_internal_id =
            self.get_or_create_internal_node(phantom_osm_id, original_node_id);
        Ok(phantom_internal_id)
    }

    fn get_way_penalty(&self, tags: &FxHashMap<u32, u32>) -> Option<f64> {
        if !self.is_way_accessible(tags) {
            return None;
        }
        tags.get(&self.profile.key)
            .and_then(|val_id| self.profile.penalties.get(val_id))
            .copied()
            .or_else(|| self.profile.default_penalty.map(|p| p as f64))
    }

    fn is_way_accessible(&self, tags: &FxHashMap<u32, u32>) -> bool {
        let yes_id = self.interner.map.get("yes").copied();
        let designated_id = self.interner.map.get("designated").copied();
        let permissive_id = self.interner.map.get("permissive").copied();

        let no_id = self.interner.map.get("no").copied();
        let private_id = self.interner.map.get("private").copied();
        let false_id = self.interner.map.get("false").copied();

        let mut has_specific_permission = false;
        let mut has_specific_prohibition = false;

        for tag_id in self.profile.access_tags.iter() {
            if let Some(val_id) = tags.get(tag_id) {
                let val_id = Some(*val_id);

                if val_id == yes_id || val_id == designated_id || val_id == permissive_id {
                    has_specific_permission = true;
                    break;
                }

                if val_id == no_id || val_id == private_id || val_id == false_id {
                    has_specific_prohibition = true;
                }
            }
        }

        if has_specific_permission {
            return true;
        }

        if has_specific_prohibition {
            return false;
        }

        true
    }

    fn get_way_direction(&self, tags: &FxHashMap<u32, u32>) -> (bool, bool) {
        if let Some(j_id) = self.interner.map.get("junction") {
            if let Some(val_id) = tags.get(j_id) {
                if Some(*val_id) == self.interner.map.get("roundabout").copied()
                    || Some(*val_id) == self.interner.map.get("circular").copied()
                {
                    return (true, false);
                }
            }
        }
        let yes_id = self.interner.map.get("yes").copied();
        let true_id = self.interner.map.get("true").copied();
        let one_id = self.interner.map.get("1").copied();
        let reverse_id = self.interner.map.get("reverse").copied();
        let minus_one_id = self.interner.map.get("-1").copied();
        let no_id = self.interner.map.get("no").copied();

        for tag_id in self.profile.oneway_tags.iter() {
            if let Some(val_id) = tags.get(tag_id).copied() {
                if Some(val_id) == yes_id || Some(val_id) == true_id || Some(val_id) == one_id {
                    return (true, false);
                }
                if Some(val_id) == reverse_id || Some(val_id) == minus_one_id {
                    return (false, true);
                }
                if Some(val_id) == no_id {
                    return (true, true);
                }
            }
        }
        (true, true)
    }

    fn get_restriction_type(&self, tags: &FxHashMap<u32, u32>) -> TurnRestriction {
        let type_id = self.interner.map.get("type").copied();
        let restriction_id = self.interner.map.get("restriction").copied();

        if type_id.is_none() || restriction_id.is_none() {
            return TurnRestriction::Inapplicable;
        }
        if tags.get(&type_id.unwrap()) != Some(&restriction_id.unwrap()) {
            return TurnRestriction::Inapplicable;
        }
        if self.is_exempted(tags) {
            return TurnRestriction::Inapplicable;
        }

        let mut restriction_value_id = None;
        for mode_id in self.profile.access_tags.iter().rev() {
            let mode_str = &self.interner.vec[*mode_id as usize];
            let key_str = format!("restriction:{}", mode_str);
            if let Some(key_id) = self.interner.map.get(&key_str) {
                if let Some(val_id) = tags.get(key_id) {
                    restriction_value_id = Some(*val_id);
                    break;
                }
            }
        }
        if restriction_value_id.is_none() {
            if let Some(val_id) = tags.get(&restriction_id.unwrap()) {
                restriction_value_id = Some(*val_id);
            }
        }

        if let Some(val_id) = restriction_value_id {
            let value_str = &self.interner.vec[val_id as usize];
            if value_str.starts_with("no_") {
                return TurnRestriction::Prohibitory;
            }
            if value_str.starts_with("only_") {
                return TurnRestriction::Mandatory;
            }
        }
        TurnRestriction::Inapplicable
    }

    fn is_exempted(&self, tags: &FxHashMap<u32, u32>) -> bool {
        if let Some(except_key_id) = self.interner.map.get("except") {
            if let Some(except_val_id) = tags.get(except_key_id) {
                let except_str = &self.interner.vec[*except_val_id as usize];
                for e_str in except_str.split(';') {
                    if let Some(e_id) = self.interner.map.get(e_str.trim()) {
                        if self.profile.except_tags.contains(e_id) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn get_ordered_restriction_members<'b>(
        &self,
        r: &'b Relation,
    ) -> Result<Vec<&'b RelationMember>> {
        let mut from: Option<&'b RelationMember> = None;
        let mut to: Option<&'b RelationMember> = None;
        let mut via: Vec<&'b RelationMember> = Vec::new();

        for m in &r.members {
            match m.role.as_str() {
                "from" => from = Some(m),
                "to" => to = Some(m),
                "via" => via.push(m),
                _ => {}
            }
        }

        let from =
            from.ok_or_else(|| GraphError::InvalidOsmData("Missing 'from' member".into()))?;
        let to = to.ok_or_else(|| GraphError::InvalidOsmData("Missing 'to' member".into()))?;
        if via.is_empty() {
            return Err(GraphError::InvalidOsmData("Missing 'via' member".into()));
        }

        let mut ordered = vec![from];
        ordered.extend(via);
        ordered.push(to);
        Ok(ordered)
    }

    fn restriction_member_to_nodes(&self, _r: &Relation, m: &RelationMember) -> Result<Vec<i64>> {
        match m.member_type.as_str() {
            "node" if m.role == "via" => {
                if !self.raw_nodes.contains_key(&m.ref_id) {
                    return Err(GraphError::InvalidOsmData(format!(
                        "Unknown node in restriction: {}",
                        m.ref_id
                    )));
                }
                Ok(vec![m.ref_id])
            }
            "way" => self.way_node_map.get(&m.ref_id).cloned().ok_or_else(|| {
                GraphError::InvalidOsmData(format!(
                    "Unknown or unusable way in restriction: {}",
                    m.ref_id
                ))
            }),
            _ => Err(GraphError::InvalidOsmData(format!(
                "Invalid member type/role combo: {}/{}",
                m.member_type, m.role
            ))),
        }
    }

    fn flatten_restriction_nodes(
        &self,
        _r: &Relation,
        mut members_nodes: Vec<Vec<i64>>,
    ) -> Result<Vec<i64>> {
        if members_nodes.len() < 2 {
            return Err(GraphError::InvalidOsmData(
                "Not enough members to form a path".into(),
            ));
        }

        let mut path: Vec<i64>;
        let mut from_way = members_nodes.remove(0);

        if members_nodes.is_empty() {
            return Err(GraphError::InvalidOsmData(
                "Restriction needs at least 'from' and 'to'/'via' members".into(),
            ));
        }

        let next_way_start = *members_nodes[0].first().unwrap();
        let next_way_end = *members_nodes[0].last().unwrap();

        if *from_way.last().unwrap() != next_way_start && *from_way.last().unwrap() != next_way_end
        {
            if *from_way.first().unwrap() == next_way_start
                || *from_way.first().unwrap() == next_way_end
            {
                from_way.reverse();
            } else {
                return Err(GraphError::InvalidOsmData(
                    "Disjoined 'from' member in restriction".into(),
                ));
            }
        }

        path = from_way;

        for i in 0..members_nodes.len() {
            let mut current_way = members_nodes[i].clone();

            if *path.last().unwrap() == *current_way.last().unwrap() {
                current_way.reverse();
            }

            if *path.last().unwrap() != *current_way.first().unwrap() {
                return Err(GraphError::InvalidOsmData(
                    "Disjoined 'via' or 'to' member in restriction".into(),
                ));
            }

            path.extend_from_slice(&current_way[1..]);
        }

        Ok(path)
    }
}
