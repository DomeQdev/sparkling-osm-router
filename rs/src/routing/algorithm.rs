use crate::core::errors::{GraphError, Result};
use crate::core::types::{Node, Profile, Way};
use crate::routing::{Edge, RouteGraph, RouteResult};
use crate::spatial::geometry::haversine_distance;
use rustc_hash::FxHashMap;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Copy, Clone, Eq, PartialEq)]
struct State {
    node_id: i64,
    prev_node_id: Option<i64>,
    prev_way_id: Option<i64>,
    cost: i64,
    estimated_total_cost: i64,
}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimated_total_cost
            .cmp(&self.estimated_total_cost)
            .then_with(|| self.cost.cmp(&other.cost))
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
struct VisitedKey {
    node_id: i64,
    prev_way_id: Option<i64>,
}

pub fn find_route_astar(
    graph: &RouteGraph,
    start_node_id: i64,
    end_node_id: i64,
    profile: &Profile,
) -> Result<Option<RouteResult>> {
    if start_node_id == end_node_id {
        return Ok(Some(RouteResult {
            nodes: vec![start_node_id],
            ways: vec![],
        }));
    }

    let start_node = graph.nodes.get(&start_node_id).ok_or_else(|| {
        GraphError::InvalidOsmData(format!("Start node {} not found", start_node_id))
    })?;
    let end_node = graph
        .nodes
        .get(&end_node_id)
        .ok_or_else(|| GraphError::InvalidOsmData(format!("End node {} not found", end_node_id)))?;

    let mut open_set = BinaryHeap::new();
    let mut g_score: FxHashMap<VisitedKey, i64> = FxHashMap::default();
    let mut came_from: FxHashMap<VisitedKey, VisitedKey> = FxHashMap::default();

    let start_key = VisitedKey {
        node_id: start_node_id,
        prev_way_id: None,
    };
    g_score.insert(start_key, 0);
    open_set.push(State {
        node_id: start_node_id,
        prev_node_id: None,
        prev_way_id: None,
        cost: 0,
        estimated_total_cost: heuristic_cost(start_node, end_node),
    });

    let mut iterations = 0;
    const MAX_ITERATIONS: u32 = 15_000_000;

    while let Some(current) = open_set.pop() {
        if iterations > MAX_ITERATIONS {
            return Ok(None);
        }
        iterations += 1;

        if current.node_id == end_node_id {
            let final_key = VisitedKey {
                node_id: current.node_id,
                prev_way_id: current.prev_way_id,
            };
            return Ok(Some(reconstruct_path(final_key, &came_from)));
        }

        let current_key = VisitedKey {
            node_id: current.node_id,
            prev_way_id: current.prev_way_id,
        };
        if current.cost > *g_score.get(&current_key).unwrap_or(&i64::MAX) {
            continue;
        }

        if let Some(edges) = graph.adjacency_list.get(&current.node_id) {
            for edge in edges {
                let way = graph.ways.get(&edge.way_id).unwrap();

                let dir = get_way_directionality(way, profile);
                if dir == -1 {
                    continue;
                }
                if !is_way_accessible(way, profile) {
                    continue;
                }

                let edge_cost = calculate_edge_cost(graph, edge, profile);
                if edge_cost.is_none() {
                    continue;
                }
                let edge_cost = edge_cost.unwrap();
                let new_cost = current.cost.saturating_add(edge_cost);

                let neighbor_key = VisitedKey {
                    node_id: edge.to_node,
                    prev_way_id: Some(edge.way_id),
                };

                if new_cost < *g_score.get(&neighbor_key).unwrap_or(&i64::MAX) {
                    g_score.insert(neighbor_key, new_cost);
                    came_from.insert(neighbor_key, current_key);

                    let neighbor_node = graph.nodes.get(&edge.to_node).unwrap();
                    let h_cost = heuristic_cost(neighbor_node, end_node);

                    open_set.push(State {
                        node_id: edge.to_node,
                        prev_node_id: Some(current.node_id),
                        prev_way_id: Some(edge.way_id),
                        cost: new_cost,
                        estimated_total_cost: new_cost.saturating_add(h_cost),
                    });
                }
            }
        }
    }

    Ok(None)
}

fn reconstruct_path(
    mut current_key: VisitedKey,
    came_from: &FxHashMap<VisitedKey, VisitedKey>,
) -> RouteResult {
    let mut nodes = vec![current_key.node_id];
    let mut ways = Vec::new();

    while let Some(prev_key) = came_from.get(&current_key) {
        if let Some(way_id) = current_key.prev_way_id {
            ways.push(way_id);
        }
        nodes.push(prev_key.node_id);
        current_key = *prev_key;
        if current_key.prev_way_id.is_none() {
            break;
        }
    }

    nodes.reverse();
    ways.reverse();
    RouteResult { nodes, ways }
}

fn calculate_edge_cost(graph: &RouteGraph, edge: &Edge, profile: &Profile) -> Option<i64> {
    let way = graph.ways.get(&edge.way_id).unwrap();
    let penalty = get_way_penalty(way, profile);

    if penalty.is_none() {
        return None;
    }

    return Some(((edge.distance) * penalty.unwrap()) as i64);
}

fn get_way_directionality(way: &Way, profile: &Profile) -> i8 {
    if way
        .tags
        .get("junction")
        .map_or(false, |v| v == "roundabout" || v == "circular")
    {
        return 1;
    }

    if let Some(oneway_tags) = &profile.oneway_tags {
        for tag in oneway_tags {
            if let Some(val) = way.tags.get(tag) {
                return match val.as_str() {
                    "yes" | "true" | "1" => 1,
                    "-1" => -1,
                    "no" | "false" | "0" => 0,
                    _ => continue,
                };
            }
        }
    }
    0
}

pub fn is_way_accessible(way: &Way, profile: &Profile) -> bool {
    if !way.tags.contains_key(&profile.key) {
        return false;
    }

    if let Some(access_tags) = &profile.access_tags {
        for tag in access_tags.iter().rev() {
            if let Some(val) = way.tags.get(tag) {
                match val.as_str() {
                    "no" | "private" | "false" | "use_sidepath" => return false,
                    _ => return true,
                };
            }
        }
    }

    true
}

pub fn get_way_penalty(way: &Way, profile: &Profile) -> Option<f64> {
    if !is_way_accessible(way, profile) {
        return None;
    }

    if let Some(tag_value) = way.tags.get(&profile.key) {
        if let Some(penalty) = profile.penalties.penalties.get(tag_value) {
            return Some(*penalty);
        }
    }

    profile.penalties.default
}

fn heuristic_cost(a: &Node, b: &Node) -> i64 {
    (haversine_distance(a.lat, a.lon, b.lat, b.lon) * 1000.0 * 25.0) as i64
}
