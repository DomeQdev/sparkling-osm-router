use crate::core::errors::{GraphError, Result};
use crate::core::types::{Node, Profile, Relation, RelationMember, Way};
use crate::graph::{ProcessedGraph, RouteNode};
use crate::routing::haversine_distance;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::HashMap;

const MAX_NODE_ID: i64 = 0x0008_0000_0000_0000;

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
enum TurnRestriction {
    Inapplicable,
    Prohibitory,
    Mandatory,
}

pub struct GraphBuilder<'a> {
    profile: &'a Profile,
    raw_nodes: &'a HashMap<i64, Node>,
    raw_ways: &'a HashMap<i64, Way>,
    raw_relations: &'a HashMap<i64, Relation>,
    processed_graph: ProcessedGraph,
    phantom_node_counter: i64,
    way_node_map: FxHashMap<i64, Vec<i64>>,
    used_nodes: FxHashSet<i64>,
}

impl<'a> GraphBuilder<'a> {
    pub fn new(
        profile: &'a Profile,
        raw_nodes: &'a HashMap<i64, Node>,
        raw_ways: &'a HashMap<i64, Way>,
        raw_relations: &'a HashMap<i64, Relation>,
    ) -> Self {
        GraphBuilder {
            profile,
            raw_nodes,
            raw_ways,
            raw_relations,
            processed_graph: ProcessedGraph::new(),
            phantom_node_counter: MAX_NODE_ID,
            way_node_map: FxHashMap::default(),
            used_nodes: FxHashSet::default(),
        }
    }

    pub fn build(mut self) -> Result<ProcessedGraph> {
        for way in self.raw_ways.values() {
            self.add_way(way);
        }

        for node_id in &self.used_nodes {
            if let Some(node) = self.raw_nodes.get(node_id) {
                self.processed_graph.nodes.insert(
                    *node_id,
                    RouteNode {
                        id: *node_id,
                        external_id: *node_id,
                        lat: node.lat,
                        lon: node.lon,
                        tags: node.tags.clone(),
                    },
                );
            }
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

        self.processed_graph.build_spatial_index();
        Ok(self.processed_graph)
    }

    fn add_way(&mut self, way: &Way) {
        let penalty = match self.get_way_penalty(&way.tags) {
            Some(p) if p.is_finite() && p >= 1.0 => p,
            _ => return,
        };
        let (forward, backward) = self.get_way_direction(&way.tags);
        if !forward && !backward {
            return;
        }

        let valid_nodes: Vec<i64> = way
            .node_refs
            .iter()
            .filter(|&&id| self.raw_nodes.contains_key(&id))
            .cloned()
            .collect();
        if valid_nodes.len() < 2 {
            return;
        }

        self.way_node_map.insert(way.id, valid_nodes.clone());
        self.used_nodes.extend(valid_nodes.iter());

        for window in valid_nodes.windows(2) {
            let (from_node_id, to_node_id) = (window[0], window[1]);
            let from_node = self.raw_nodes.get(&from_node_id).unwrap();
            let to_node = self.raw_nodes.get(&to_node_id).unwrap();
            let distance =
                haversine_distance(from_node.lat, from_node.lon, to_node.lat, to_node.lon);
            let cost = (distance * penalty * 1000.0) as u32;

            if forward {
                self.processed_graph
                    .edges
                    .entry(from_node_id)
                    .or_default()
                    .insert(to_node_id, cost);
            }
            if backward {
                self.processed_graph
                    .edges
                    .entry(to_node_id)
                    .or_default()
                    .insert(from_node_id, cost);
            }
        }
    }

    fn add_relation(&mut self, rel: &Relation) -> Result<()> {
        let restriction_type = self.get_restriction_type(&rel.tags);
        if restriction_type == TurnRestriction::Inapplicable {
            return Ok(());
        }

        let members = self.get_ordered_restriction_members(rel)?;
        let mut member_nodes: Vec<Vec<i64>> = Vec::new();
        for m in members {
            member_nodes.push(self.restriction_member_to_nodes(rel, m)?);
        }
        let nodes_path = self.flatten_restriction_nodes(rel, member_nodes)?;

        if nodes_path.len() < 3 {
            return Err(GraphError::InvalidOsmData(
                "Restriction path too short".into(),
            ));
        }

        let is_mandatory = restriction_type == TurnRestriction::Mandatory;
        self.store_restriction(&nodes_path, is_mandatory)?;
        Ok(())
    }

    fn store_restriction(&mut self, osm_nodes: &[i64], is_mandatory: bool) -> Result<()> {
        let mut cloned_nodes = vec![osm_nodes[0]];
        let mut edges_to_add: FxHashMap<(i64, i64), u32> = FxHashMap::default();
        let mut edges_to_remove: FxHashSet<(i64, i64)> = FxHashSet::default();

        for window in osm_nodes.windows(2) {
            let (prev_node_id, osm_node_id) = (*cloned_nodes.last().unwrap(), window[1]);
            let (edge_to_clone, cost) = self.find_edge_to_clone(prev_node_id, osm_node_id)?;

            let is_last_segment = osm_node_id == *osm_nodes.last().unwrap();
            let is_phantom = self
                .processed_graph
                .nodes
                .get(&edge_to_clone)
                .map_or(false, |n| n.external_id != edge_to_clone);

            if !is_phantom && !is_last_segment {
                let new_phantom_id = self.create_phantom_node(osm_node_id)?;
                edges_to_remove.insert((prev_node_id, edge_to_clone));
                edges_to_add.insert((prev_node_id, new_phantom_id), cost);
                cloned_nodes.push(new_phantom_id);
            } else {
                cloned_nodes.push(edge_to_clone);
            }
        }

        for (from, to) in edges_to_remove {
            if let Some(edges) = self.processed_graph.edges.get_mut(&from) {
                edges.remove(&to);
            }
        }
        for ((from, to), cost) in edges_to_add {
            self.processed_graph
                .edges
                .entry(from)
                .or_default()
                .insert(to, cost);
        }

        if is_mandatory {
            for window in cloned_nodes.windows(2) {
                let (from_id, to_id) = (window[0], window[1]);
                if let Some(cost) = self
                    .processed_graph
                    .edges
                    .get(&from_id)
                    .and_then(|e| e.get(&to_id))
                    .copied()
                {
                    let edges = self.processed_graph.edges.entry(from_id).or_default();
                    edges.clear();
                    edges.insert(to_id, cost);
                }
            }
        } else {
            let from_id = cloned_nodes[cloned_nodes.len() - 2];
            let to_id = *cloned_nodes.last().unwrap();
            if let Some(edges) = self.processed_graph.edges.get_mut(&from_id) {
                edges.remove(&to_id);
            }
        }
        Ok(())
    }

    fn find_edge_to_clone(&self, from_node_id: i64, to_osm_id: i64) -> Result<(i64, u32)> {
        self.processed_graph
            .edges
            .get(&from_node_id)
            .ok_or_else(|| {
                GraphError::InvalidOsmData(
                    "Disconnected turn restriction: 'from' node has no outgoing edges".into(),
                )
            })?
            .iter()
            .find(|(&id, _)| {
                self.processed_graph
                    .nodes
                    .get(&id)
                    .map_or(false, |n| n.external_id == to_osm_id)
            })
            .map(|(&id, &cost)| (id, cost))
            .ok_or_else(|| {
                GraphError::InvalidOsmData(format!(
                    "Disconnected turn restriction path: no edge from {} to osm node {}",
                    from_node_id, to_osm_id
                ))
            })
    }

    fn create_phantom_node(&mut self, original_node_id: i64) -> Result<i64> {
        self.phantom_node_counter += 1;
        let phantom_id = self.phantom_node_counter;
        let original_node = self.raw_nodes.get(&original_node_id).unwrap();
        self.processed_graph.nodes.insert(
            phantom_id,
            RouteNode {
                id: phantom_id,
                external_id: original_node_id,
                lat: original_node.lat,
                lon: original_node.lon,
                tags: original_node.tags.clone(),
            },
        );
        if let Some(edges) = self.processed_graph.edges.get(&original_node_id) {
            self.processed_graph.edges.insert(phantom_id, edges.clone());
        }
        Ok(phantom_id)
    }

    fn get_way_penalty(&self, tags: &HashMap<String, String>) -> Option<f64> {
        if !self.is_way_accessible(tags) {
            return None;
        }
        tags.get(&self.profile.key)
            .and_then(|val| self.profile.penalties.penalties.get(val))
            .map(|p| *p as f64)
            .or_else(|| self.profile.penalties.default.map(|p| p as f64))
    }

    fn is_way_accessible(&self, tags: &HashMap<String, String>) -> bool {
        for tag in self.profile.access_tags.iter() {
            if let Some(val) = tags.get(tag) {
                return !matches!(val.as_str(), "no" | "private" | "false");
            }
        }
        true
    }

    fn get_way_direction(&self, tags: &HashMap<String, String>) -> (bool, bool) {
        if let Some(j) = tags.get("junction") {
            if j == "roundabout" || j == "circular" {
                return (true, false);
            }
        }
        for tag in self.profile.oneway_tags.iter() {
            if let Some(val) = tags.get(tag) {
                return match val.as_str() {
                    "yes" | "true" | "1" => (true, false),
                    "-1" | "reverse" => (false, true),
                    "no" => (true, true),
                    _ => continue,
                };
            }
        }
        (true, true)
    }

    fn get_restriction_type(&self, tags: &HashMap<String, String>) -> TurnRestriction {
        if tags.get("type") != Some(&"restriction".to_string()) {
            return TurnRestriction::Inapplicable;
        }
        if self.is_exempted(tags) {
            return TurnRestriction::Inapplicable;
        }

        let restriction_value = self
            .profile
            .access_tags
            .iter()
            .rev()
            .find_map(|mode| tags.get(&format!("restriction:{}", mode)))
            .or_else(|| tags.get("restriction"));

        match restriction_value.map(|s| s.as_str()) {
            Some(r) => {
                if r.starts_with("no_") {
                    TurnRestriction::Prohibitory
                } else if r.starts_with("only_") {
                    TurnRestriction::Mandatory
                } else {
                    TurnRestriction::Inapplicable
                }
            }
            None => TurnRestriction::Inapplicable,
        }
    }

    fn is_exempted(&self, tags: &HashMap<String, String>) -> bool {
        if let Some(except) = tags.get("except") {
            return except
                .split(';')
                .any(|e| self.profile.except_tags.contains(&e.trim().to_string()));
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
                "from" => {
                    if from.is_some() {
                        return Err(GraphError::InvalidOsmData("Multiple 'from' members".into()));
                    }
                    from = Some(m);
                }
                "to" => {
                    if to.is_some() {
                        return Err(GraphError::InvalidOsmData("Multiple 'to' members".into()));
                    }
                    to = Some(m);
                }
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

        let mut path = Vec::new();

        let mut from_way = members_nodes.remove(0);
        let next_way_start = *members_nodes[0].first().unwrap();
        let next_way_end = *members_nodes[0].last().unwrap();
        if *from_way.first().unwrap() == next_way_start
            || *from_way.first().unwrap() == next_way_end
        {
            from_way.reverse();
        }
        if *from_way.last().unwrap() != next_way_start && *from_way.last().unwrap() != next_way_end
        {
            return Err(GraphError::InvalidOsmData("Disjoined 'from' member".into()));
        }

        if from_way.len() >= 2 {
            path.extend_from_slice(&from_way[from_way.len() - 2..]);
        } else {
            path.extend_from_slice(&from_way);
        }

        for i in 0..members_nodes.len() {
            let mut current_way = members_nodes[i].clone();
            if *path.last().unwrap() == *current_way.last().unwrap() {
                current_way.reverse();
            }
            if *path.last().unwrap() != *current_way.first().unwrap() {
                return Err(GraphError::InvalidOsmData(
                    "Disjoined 'via' or 'to' member".into(),
                ));
            }

            if i == members_nodes.len() - 1 {
                if current_way.len() > 1 {
                    path.push(current_way[1]);
                }
            } else {
                path.extend_from_slice(&&current_way[1..]);
            }
        }

        Ok(path)
    }
}
