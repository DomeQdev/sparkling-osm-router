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

    way_node_map: FxHashMap<i64, Vec<i64>>,
    via_node_clones: FxHashMap<(u32, i64), u32>,
}

struct GraphChange<'a, 'b> {
    builder: &'a mut GraphBuilder<'b>,
}

impl<'a, 'b> GraphChange<'a, 'b> {
    fn new(builder: &'a mut GraphBuilder<'b>) -> Self {
        Self { builder }
    }

    fn get_or_create_clone(&mut self, from_node_id: u32, target_osm_id: i64) -> Result<u32> {
        if let Some(clone_id) = self
            .builder
            .via_node_clones
            .get(&(from_node_id, target_osm_id))
        {
            return Ok(*clone_id);
        }

        let original_target_id = *self.builder.node_map.get(&target_osm_id).ok_or_else(|| {
            GraphError::InvalidOsmData(format!(
                "Target node {} for cloning not found",
                target_osm_id
            ))
        })?;

        let original_target_node = &self.builder.nodes[original_target_id as usize];

        let cloned_internal_id = self.builder.next_internal_id;
        self.builder.next_internal_id += 1;

        self.builder.nodes.push(RouteNode {
            id: cloned_internal_id,
            external_id: original_target_node.external_id,
            lat: original_target_node.lat,
            lon: original_target_node.lon,
            tags: original_target_node.tags.clone(),
        });

        if let Some(edges_to_clone) = self.builder.temp_edges.get(&original_target_id).cloned() {
            self.builder
                .temp_edges
                .insert(cloned_internal_id, edges_to_clone);
        }

        self.builder
            .via_node_clones
            .insert((from_node_id, target_osm_id), cloned_internal_id);

        Ok(cloned_internal_id)
    }

    fn apply_restriction(
        &mut self,
        restriction_path: &[i64],
        restriction_type: TurnRestriction,
    ) -> Result<()> {
        if restriction_path.len() < 2 {
            return Ok(());
        }

        let mut current_node_id = *self
            .builder
            .node_map
            .get(&restriction_path[0])
            .ok_or_else(|| GraphError::InvalidOsmData("Restriction start node not found".into()))?;

        for i in 1..restriction_path.len() - 1 {
            let via_osm_id = restriction_path[i];

            let cloned_via_id = self.get_or_create_clone(current_node_id, via_osm_id)?;

            let original_via_id = *self.builder.node_map.get(&via_osm_id).unwrap();
            let edges = self.builder.temp_edges.entry(current_node_id).or_default();

            if let Some(cost) = edges.remove(&original_via_id) {
                edges.insert(cloned_via_id, cost);
            } else {
                return Err(GraphError::InvalidOsmData(format!(
                    "Edge from {} to {} does not exist for restriction",
                    restriction_path[i - 1],
                    via_osm_id
                )));
            }

            current_node_id = cloned_via_id;
        }

        let final_to_osm_id = *restriction_path.last().unwrap();
        let final_to_id = *self.builder.node_map.get(&final_to_osm_id).unwrap();

        if restriction_type == TurnRestriction::Prohibitory {
            if let Some(edges) = self.builder.temp_edges.get_mut(&current_node_id) {
                edges.remove(&final_to_id);
            }
        } else if restriction_type == TurnRestriction::Mandatory {
            if let Some(edges) = self.builder.temp_edges.get_mut(&current_node_id) {
                edges.retain(|&k, _| k == final_to_id);
            }
        }

        Ok(())
    }
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
            way_node_map: FxHashMap::default(),
            via_node_clones: FxHashMap::default(),
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
                    self.get_or_create_internal_node(osm_node_id);
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
        graph.edges.clear();

        let mut edge_count: usize = 0;
        for node_id in 0..node_count as u32 {
            graph.offsets[node_id as usize] = edge_count;

            if let Some(neighbors) = self.temp_edges.get(&node_id) {
                let mut sorted_neighbors: Vec<_> = neighbors.iter().collect();
                sorted_neighbors.sort_unstable_by_key(|(k, _v)| **k);

                for (&target, &cost) in sorted_neighbors {
                    graph.edges.push((target, cost));
                }

                edge_count += neighbors.len();
            }
        }
        graph.offsets[node_count] = edge_count;

        graph.build_indices();
        Ok(graph)
    }

    fn get_or_create_internal_node(&mut self, osm_node_id: i64) -> u32 {
        if let Some(id) = self.node_map.get(&osm_node_id) {
            return *id;
        }

        let internal_id = self.next_internal_id;
        self.node_map.insert(osm_node_id, internal_id);
        self.next_internal_id += 1;

        let raw_node = self.raw_nodes.get(&osm_node_id).unwrap();
        let interned_tags = raw_node
            .tags
            .iter()
            .map(|(k, v)| (self.interner.intern(k), self.interner.intern(v)))
            .collect();

        self.nodes.push(RouteNode {
            id: internal_id,
            external_id: osm_node_id,
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
        let nodes_path = self.flatten_restriction_nodes(member_nodes)?;
        let mut change = GraphChange::new(self);

        change.apply_restriction(&nodes_path, restriction_type)
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
            if let Some(val_id) = tags.get(tag_id).copied() {
                let val_id = Some(val_id);
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

    fn find_way_connection(way1: &[i64], way2: &[i64]) -> Result<(i64, usize, usize)> {
        for (i, &node1) in way1.iter().enumerate() {
            if let Some((j, _)) = way2.iter().enumerate().find(|(_, &node2)| node2 == node1) {
                return Ok((node1, i, j));
            }
        }

        Err(GraphError::InvalidOsmData(
            "Could not find connection point between ways in restriction".into(),
        ))
    }

    fn get_node_before(way: &[i64], idx: usize) -> Result<i64> {
        if idx > 0 {
            Ok(way[idx - 1])
        } else {
            Err(GraphError::InvalidOsmData("Connection point is at the start of 'from' or 'via' way, cannot determine previous node".into()))
        }
    }

    fn get_node_after(way: &[i64], idx: usize) -> Result<i64> {
        if idx < way.len() - 1 {
            Ok(way[idx + 1])
        } else {
            Err(GraphError::InvalidOsmData(
                "Connection point is at the end of 'to' or 'via' way, cannot determine next node"
                    .into(),
            ))
        }
    }

    fn flatten_restriction_nodes(&self, members_nodes: Vec<Vec<i64>>) -> Result<Vec<i64>> {
        if members_nodes.is_empty() {
            return Err(GraphError::InvalidOsmData(
                "No members in restriction.".into(),
            ));
        }

        if members_nodes.len() == 3 && members_nodes[1].len() == 1 {
            let from_way = &members_nodes[0];
            let via_node_id = members_nodes[1][0];
            let to_way = &members_nodes[2];

            let from_idx = from_way
                .iter()
                .position(|&n| n == via_node_id)
                .ok_or_else(|| {
                    GraphError::InvalidOsmData("Via node not found in 'from' way.".into())
                })?;
            let to_idx = to_way
                .iter()
                .position(|&n| n == via_node_id)
                .ok_or_else(|| {
                    GraphError::InvalidOsmData("Via node not found in 'to' way.".into())
                })?;

            let from_node = Self::get_node_before(from_way, from_idx)?;
            let to_node = Self::get_node_after(to_way, to_idx)?;

            return Ok(vec![from_node, via_node_id, to_node]);
        }

        let mut final_path: Vec<i64> = Vec::new();
        for (i, pair) in members_nodes.windows(2).enumerate() {
            let way_a = &pair[0];
            let way_b = &pair[1];
            let (connection_node, idx_a, idx_b) = Self::find_way_connection(way_a, way_b)?;
            if final_path.is_empty() {
                let from_node = Self::get_node_before(way_a, idx_a)?;
                final_path.push(from_node);
                final_path.push(connection_node);
            }

            if i == members_nodes.len() - 2 {
                let to_node = Self::get_node_after(way_b, idx_b)?;
                final_path.push(to_node);
            } else {
                let way_c = &members_nodes[i + 2];
                let (end_connection_node, _, _) = Self::find_way_connection(way_b, way_c)?;
                let start_idx_in_b = way_b.iter().position(|&n| n == connection_node).unwrap();
                let end_idx_in_b = way_b
                    .iter()
                    .position(|&n| n == end_connection_node)
                    .unwrap();

                if start_idx_in_b < end_idx_in_b {
                    final_path.extend_from_slice(&way_b[start_idx_in_b + 1..=end_idx_in_b]);
                } else {
                    let mut reversed_segment: Vec<i64> = way_b[end_idx_in_b..start_idx_in_b]
                        .iter()
                        .cloned()
                        .collect();

                    reversed_segment.reverse();
                    final_path.extend(reversed_segment);
                    final_path.push(end_connection_node);
                }
            }
        }
        Ok(final_path)
    }
}
