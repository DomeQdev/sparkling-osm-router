use crate::core::types::Graph;
use crate::spatial::geometry::haversine_distance;
use std::sync::{Arc, RwLock};

pub type SharedGraph = Arc<RwLock<Graph>>;

pub fn calculate_route_distance(graph: &Graph, node_ids: &[i64]) -> f64 {
    let mut total_distance = 0.0;

    for i in 0..node_ids.len().saturating_sub(1) {
        if let (Some(node1), Some(node2)) = (
            graph.nodes.get(&node_ids[i]),
            graph.nodes.get(&node_ids[i + 1]),
        ) {
            total_distance += haversine_distance(node1.lat, node1.lon, node2.lat, node2.lon);
        }
    }

    total_distance
}
