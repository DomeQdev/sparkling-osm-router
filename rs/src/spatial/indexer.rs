use crate::core::errors::Result;
use crate::core::types::{Graph, Node, Way, WayEnvelope};
use crate::routing::{Edge as RouteEdge, RouteGraph};
use crate::spatial::geometry::haversine_distance;
use rstar::{RTree, AABB};
use rustc_hash::FxHashMap;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub fn index_graph(mut graph: Graph) -> Result<Graph> {
    filter_graph(&mut graph);

    let route_graph = build_routing_graph(&graph);

    graph.route_graph = Some(Arc::new(route_graph));
    graph.index_rtree()?;

    Ok(graph)
}

fn build_routing_graph(graph: &Graph) -> RouteGraph {
    let mut adjacency_list: FxHashMap<i64, Vec<RouteEdge>> = FxHashMap::default();

    for way in graph.ways.values() {
        for window in way.node_refs.windows(2) {
            let from_node_id = window[0];
            let to_node_id = window[1];
            let distance = if let (Some(n1), Some(n2)) =
                (graph.nodes.get(&from_node_id), graph.nodes.get(&to_node_id))
            {
                haversine_distance(n1.lat, n1.lon, n2.lat, n2.lon) * 1000.0
            } else {
                0.0
            };

            adjacency_list
                .entry(from_node_id)
                .or_default()
                .push(RouteEdge {
                    to_node: to_node_id,
                    way_id: way.id,
                    distance,
                });
        }
    }

    RouteGraph {
        adjacency_list,
        nodes: FxHashMap::from_iter(graph.nodes.clone()),
        ways: FxHashMap::from_iter(graph.ways.clone()),
        ..Default::default()
    }
}

impl Graph {
    pub fn index_rtree(&mut self) -> Result<()> {
        let mut way_envelopes: Vec<WayEnvelope> = Vec::new();
        for way in self.ways.values() {
            if let Some(envelope) = calculate_way_envelope(way, &self.nodes) {
                way_envelopes.push(WayEnvelope {
                    way_id: way.id,
                    envelope,
                });
            }
        }
        self.way_rtree = RTree::bulk_load(way_envelopes);
        Ok(())
    }
}

fn filter_graph(graph: &mut Graph) {
    let used_node_ids: HashSet<i64> = graph
        .ways
        .values()
        .flat_map(|w| w.node_refs.iter().cloned())
        .collect();
    graph.nodes.retain(|id, _| used_node_ids.contains(id));
    graph
        .relations
        .retain(|_, r| r.tags.get("type") == Some(&"restriction".to_string()));
}

fn calculate_way_envelope(way: &Way, nodes: &HashMap<i64, Node>) -> Option<AABB<[f64; 2]>> {
    if way.node_refs.is_empty() {
        return None;
    }
    let mut min_lon = f64::MAX;
    let mut min_lat = f64::MAX;
    let mut max_lon = f64::MIN;
    let mut max_lat = f64::MIN;
    let mut has_coords = false;
    for node_ref in &way.node_refs {
        if let Some(node) = nodes.get(node_ref) {
            min_lon = min_lon.min(node.lon);
            min_lat = min_lat.min(node.lat);
            max_lon = max_lon.max(node.lon);
            max_lat = max_lat.max(node.lat);
            has_coords = true;
        }
    }
    if !has_coords {
        None
    } else {
        Some(AABB::from_corners([min_lon, min_lat], [max_lon, max_lat]))
    }
}
