use crate::core::errors::{GraphError, Result};
use crate::core::types::{Node, Profile, Relation, RelationMember, Way};
use crate::graph::{ProcessedGraph, RouteNode, MAX_NODE_ID};
use crate::routing::haversine_distance;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::HashMap;

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

    // ZMIANA: Nowe pola do budowy grafu w formacie CSR
    node_map: FxHashMap<i64, u32>,
    next_internal_id: u32,
    nodes: Vec<RouteNode>,
    temp_edges: FxHashMap<u32, FxHashMap<u32, u32>>,

    phantom_node_counter: i64,
    way_node_map: FxHashMap<i64, Vec<i64>>,
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
            node_map: FxHashMap::default(),
            next_internal_id: 0,
            nodes: Vec::new(),
            temp_edges: FxHashMap::default(),
            phantom_node_counter: MAX_NODE_ID,
            way_node_map: FxHashMap::default(),
        }
    }

    pub fn build(mut self) -> Result<ProcessedGraph> {
        // Krok 1: Przejdź przez wszystkie użyteczne drogi i utwórz dla ich węzłów wewnętrzne ID
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
                self.way_node_map.insert(way.id, valid_nodes.clone());
                for &osm_node_id in &valid_nodes {
                    self.get_or_create_internal_node(osm_node_id, osm_node_id);
                }
            }
        }

        // Krok 2: Przetwórz drogi, dodając krawędzie do tymczasowej struktury
        for way in self.raw_ways.values() {
            self.add_way(way);
        }

        // Krok 3: Przetwórz relacje, modyfikując graf
        for relation in self.raw_relations.values() {
            if let Err(e) = self.add_relation(relation) {
                log::warn!(
                    "Skipping turn restriction {}: {}",
                    relation.id,
                    e.to_string()
                );
            }
        }

        // Krok 4: Skonwertuj tymczasową strukturę do finalnego formatu CSR
        self.finalize_graph()
    }

    fn finalize_graph(self) -> Result<ProcessedGraph> {
        let mut graph = ProcessedGraph::new();
        graph.nodes = self.nodes;
        graph.node_id_map = self.node_map;
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
        self.nodes.push(RouteNode {
            id: internal_id,
            external_id,
            lat: raw_node.lat,
            lon: raw_node.lon,
            tags: raw_node.tags.clone(),
        });

        internal_id
    }

    fn is_way_usable(&self, way: &Way) -> bool {
        let penalty = self.get_way_penalty(&way.tags);
        if penalty.is_none() || !penalty.unwrap().is_finite() || penalty.unwrap() < 1.0 {
            return false;
        }
        let (forward, backward) = self.get_way_direction(&way.tags);
        !(!forward && !backward)
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

        if let Some(valid_nodes) = self.way_node_map.get(&way.id) {
            for window in valid_nodes.windows(2) {
                let (from_osm, to_osm) = (window[0], window[1]);
                let from_node = self.raw_nodes.get(&from_osm).unwrap();
                let to_node = self.raw_nodes.get(&to_osm).unwrap();
                let distance =
                    haversine_distance(from_node.lat, from_node.lon, to_node.lat, to_node.lon);
                let cost = (distance * penalty * 1000.0) as u32;

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
        let first_node_id = *self.node_map.get(&osm_nodes[0]).unwrap();
        let mut cloned_nodes = vec![first_node_id];
        let mut edges_to_add: FxHashMap<(u32, u32), u32> = FxHashMap::default();
        let mut edges_to_remove: FxHashSet<(u32, u32)> = FxHashSet::default();

        for window in osm_nodes.windows(2) {
            let (prev_node_id, current_osm_id) = (*cloned_nodes.last().unwrap(), window[1]);
            let (edge_to_clone, cost) = self.find_edge_to_clone(prev_node_id, current_osm_id)?;

            let is_last_segment = current_osm_id == *osm_nodes.last().unwrap();
            let is_phantom = self.nodes[edge_to_clone as usize].external_id != self.node_map.iter().find(|(_, &v)| v == edge_to_clone).map(|(k, _)| *k).unwrap_or(0);


            if !is_phantom && !is_last_segment {
                let new_phantom_id = self.create_phantom_node(current_osm_id)?;
                edges_to_remove.insert((prev_node_id, edge_to_clone));
                edges_to_add.insert((prev_node_id, new_phantom_id), cost);
                cloned_nodes.push(new_phantom_id);
            } else {
                cloned_nodes.push(edge_to_clone);
            }
        }

        for (from, to) in edges_to_remove {
            if let Some(edges) = self.temp_edges.get_mut(&from) {
                edges.remove(&to);
            }
        }
        for ((from, to), cost) in edges_to_add {
            self.temp_edges.entry(from).or_default().insert(to, cost);
        }

        if is_mandatory {
            for window in cloned_nodes.windows(2) {
                let (from_id, to_id) = (window[0], window[1]);
                if let Some(cost) = self
                    .temp_edges
                    .get(&from_id)
                    .and_then(|e| e.get(&to_id))
                    .copied()
                {
                    let edges = self.temp_edges.entry(from_id).or_default();
                    edges.clear();
                    edges.insert(to_id, cost);
                }
            }
        } else {
            let from_id = cloned_nodes[cloned_nodes.len() - 2];
            let to_id = *cloned_nodes.last().unwrap();
            if let Some(edges) = self.temp_edges.get_mut(&from_id) {
                edges.remove(&to_id);
            }
        }
        Ok(())
    }

    fn find_edge_to_clone(&self, from_node_id: u32, to_osm_id: i64) -> Result<(u32, u32)> {
        self.temp_edges
            .get(&from_node_id)
            .ok_or_else(|| {
                GraphError::InvalidOsmData(
                    "Disconnected turn restriction: 'from' node has no outgoing edges".into(),
                )
            })?
            .iter()
            .find(|(&id, _)| self.nodes[id as usize].external_id == to_osm_id)
            .map(|(&id, &cost)| (id, cost))
            .ok_or_else(|| {
                GraphError::InvalidOsmData(format!(
                    "Disconnected turn restriction path: no edge from internal node {} to osm node {}",
                    from_node_id, to_osm_id
                ))
            })
    }

    fn create_phantom_node(&mut self, original_node_id: i64) -> Result<u32> {
        self.phantom_node_counter += 1;
        let phantom_osm_id = self.phantom_node_counter;
        let phantom_internal_id =
            self.get_or_create_internal_node(phantom_osm_id, original_node_id);

        let original_internal_id = *self.node_map.get(&original_node_id).unwrap();
        if let Some(edges) = self.temp_edges.get(&original_internal_id) {
            self.temp_edges.insert(phantom_internal_id, edges.clone());
        }
        Ok(phantom_internal_id)
    }

    // --- Pozostałe funkcje pomocnicze (bez zmian w logice, ale skopiowane dla kompletności) ---

    fn get_way_penalty(&self, tags: &HashMap<String, String>) -> Option<f64> {
        if !self.is_way_accessible(tags) {
            return None;
        }
        tags.get(&self.profile.key)
            .and_then(|val| self.profile.penalties.penalties.get(val))
            .copied()
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
            Some(r) if r.starts_with("no_") => TurnRestriction::Prohibitory,
            Some(r) if r.starts_with("only_") => TurnRestriction::Mandatory,
            _ => TurnRestriction::Inapplicable,
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
                "from" => from = Some(m),
                "to" => to = Some(m),
                "via" => via.push(m),
                _ => {}
            }
        }

        let from = from.ok_or_else(|| GraphError::InvalidOsmData("Missing 'from' member".into()))?;
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

        path.extend_from_slice(if from_way.len() >= 2 {
            &from_way[from_way.len() - 2..]
        } else {
            &from_way
        });

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
            path.extend_from_slice(&current_way[1..]);
        }

        Ok(path)
    }
}