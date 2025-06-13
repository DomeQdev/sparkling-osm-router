use crate::core::errors::Result;
use crate::core::types::{Graph, Node, Profile, Way};
use crate::routing::algorithm::is_way_accessible;

impl Graph {
    pub fn find_nearest_ways_and_nodes(
        &self,
        lon: f64,
        lat: f64,
        limit: usize,
        profile: &Profile,
    ) -> Result<Vec<i64>> {
        if self.way_rtree.size() == 0 {
            return Ok(Vec::new());
        }

        let point = [lon, lat];
        let mut nearest_nodes = Vec::new();

        let mut ways_with_distances: Vec<_> = self
            .way_rtree
            .nearest_neighbor_iter_with_distance_2(&point)
            .collect();

        ways_with_distances
            .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        for (way_envelope, _distance_sq) in ways_with_distances {
            if nearest_nodes.len() >= limit {
                break;
            }

            if let Some(way) = self.ways.get(&way_envelope.way_id) {
                if is_way_accessible(way, profile) {
                    for node_id in &way.node_refs {
                        if nearest_nodes.len() < limit {
                            nearest_nodes.push(*node_id);
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        Ok(nearest_nodes)
    }

    pub fn search_nodes_in_radius(
        &self,
        lon: f64,
        lat: f64,
        radius_meters: f64,
        profile: &Profile,
    ) -> Result<Vec<Node>> {
        if self.way_rtree.size() == 0 {
            return Ok(Vec::new());
        }

        let point = [lon, lat];
        let radius_degrees_sq = (radius_meters / 111_320.0).powi(2);

        let mut found_nodes_map = std::collections::HashMap::new();

        for way_envelope in self
            .way_rtree
            .locate_within_distance(point, radius_degrees_sq)
        {
            if let Some(way) = self.ways.get(&way_envelope.way_id) {
                if is_way_accessible(way, profile) {
                    for node_id in &way.node_refs {
                        if let Some(node) = self.nodes.get(node_id) {
                            found_nodes_map.insert(node.id, node.clone());
                        }
                    }
                }
            }
        }

        let found_nodes: Vec<Node> = found_nodes_map.values().cloned().collect();
        Ok(found_nodes)
    }

    pub fn search_ways_in_radius(
        &self,
        lon: f64,
        lat: f64,
        radius_meters: f64,
        profile: &Profile,
    ) -> Result<Vec<Way>> {
        if self.way_rtree.size() == 0 {
            return Ok(Vec::new());
        }

        let point = [lon, lat];
        let radius_degrees_sq = (radius_meters / 111_320.0).powi(2);

        let mut found_ways = Vec::new();
        for way_envelope in self
            .way_rtree
            .locate_within_distance(point, radius_degrees_sq)
        {
            if let Some(way) = self.ways.get(&way_envelope.way_id) {
                if is_way_accessible(way, profile) {
                    found_ways.push(way.clone());
                }
            }
        }
        Ok(found_ways)
    }
}
