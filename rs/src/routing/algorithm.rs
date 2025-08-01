use crate::core::errors::{GraphError, Result};
use crate::graph::{ProcessedGraph, RouteNode};
use crate::routing::distance;
use rustc_hash::FxHashMap;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

type PrecomputedCosts = FxHashMap<u32, u32>;

#[derive(Copy, Clone, Eq, PartialEq)]
struct State {
    cost: u32,
    estimated_total_cost: u32,
    node_id: u32,
    prev_external_id: Option<i64>,
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

#[derive(Copy, Clone, Eq, PartialEq)]
struct BackwardState {
    cost: u32,
    estimated_total_cost: u32,
    node_id: u32,
}

impl Ord for BackwardState {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimated_total_cost
            .cmp(&self.estimated_total_cost)
            .then_with(|| self.cost.cmp(&other.cost))
    }
}

impl PartialOrd for BackwardState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
struct VisitedKey {
    node_id: u32,
    prev_external_id: Option<i64>,
}

fn precompute_costs_backward(
    graph: &ProcessedGraph,
    target_node_id: u32,
) -> Result<PrecomputedCosts> {
    let target_node = graph.nodes.get(target_node_id as usize).ok_or_else(|| {
        GraphError::RoutingError("Target node for precomputation not found".to_string())
    })?;

    let mut costs: PrecomputedCosts = FxHashMap::default();
    let mut heap = BinaryHeap::new();

    costs.insert(target_node_id, 0);
    heap.push(BackwardState {
        cost: 0,
        estimated_total_cost: 0,
        node_id: target_node_id,
    });

    while let Some(current) = heap.pop() {
        if current.cost > *costs.get(&current.node_id).unwrap_or(&u32::MAX) {
            continue;
        }

        for &(neighbor_id, cost) in graph.reversed_neighbors(current.node_id) {
            let new_cost = current.cost.saturating_add(cost as u32);
            if new_cost < *costs.get(&neighbor_id).unwrap_or(&u32::MAX) {
                costs.insert(neighbor_id, new_cost);
                let neighbor_node = &graph.nodes[neighbor_id as usize];
                let h_cost = (distance(
                    neighbor_node.lat,
                    neighbor_node.lon,
                    target_node.lat,
                    target_node.lon,
                ) / 13.8
                    * 1000.0) as u32;

                heap.push(BackwardState {
                    cost: new_cost,
                    estimated_total_cost: new_cost.saturating_add(h_cost),
                    node_id: neighbor_id,
                });
            }
        }
    }

    Ok(costs)
}

pub fn find_route_through_waypoints(
    graph: &ProcessedGraph,
    waypoints: &[i64],
) -> Result<Option<Vec<i64>>> {
    if waypoints.len() < 2 {
        return Err(GraphError::RoutingError(
            "At least 2 points are required".to_string(),
        ));
    }

    let end_osm_id = *waypoints.last().unwrap();
    let end_node_id = *graph.node_id_map.get(&end_osm_id).ok_or_else(|| {
        GraphError::RoutingError(format!("End waypoint node {} not in graph", end_osm_id))
    })?;
    let precomputed_costs = precompute_costs_backward(graph, end_node_id)?;

    let mut full_path: Vec<i64> = Vec::new();
    let mut last_predecessor_id: Option<i64> = None;

    for i in 0..waypoints.len() - 1 {
        let start_osm_id = waypoints[i];
        let end_osm_id = waypoints[i + 1];

        match find_route_segment_astar(
            graph,
            start_osm_id,
            end_osm_id,
            last_predecessor_id,
            &precomputed_costs,
        )? {
            Some(segment_path) => {
                if segment_path.len() < 2 {
                    if full_path.is_empty() {
                        full_path.push(start_osm_id);
                    }
                    continue;
                }

                last_predecessor_id = Some(segment_path[segment_path.len() - 2]);

                if full_path.is_empty() {
                    full_path.extend(segment_path);
                } else {
                    full_path.extend_from_slice(&segment_path[1..]);
                }
            }
            None => {
                return Ok(None);
            }
        }
    }

    Ok(Some(full_path))
}

fn find_route_segment_astar(
    graph: &ProcessedGraph,
    start_osm_id: i64,
    end_osm_id: i64,
    initial_prev_node_osm_id: Option<i64>,
    precomputed_costs: &PrecomputedCosts,
) -> Result<Option<Vec<i64>>> {
    let start_node_id = *graph.node_id_map.get(&start_osm_id).ok_or_else(|| {
        GraphError::RoutingError(format!("Start node {} not in graph", start_osm_id))
    })?;
    let end_node_id = *graph
        .node_id_map
        .get(&end_osm_id)
        .ok_or_else(|| GraphError::RoutingError(format!("End node {} not in graph", end_osm_id)))?;

    let start_node = &graph.nodes[start_node_id as usize];
    let end_node = &graph.nodes[end_node_id as usize];

    let mut open_set = BinaryHeap::new();
    let mut g_score: FxHashMap<VisitedKey, u32> = FxHashMap::default();
    let mut came_from: FxHashMap<VisitedKey, VisitedKey> = FxHashMap::default();

    let start_key = VisitedKey {
        node_id: start_node_id,
        prev_external_id: initial_prev_node_osm_id,
    };
    g_score.insert(start_key, 0);

    let initial_h_cost = heuristic_cost(start_node, end_node, precomputed_costs);
    open_set.push(State {
        cost: 0,
        estimated_total_cost: initial_h_cost,
        node_id: start_node_id,
        prev_external_id: initial_prev_node_osm_id,
    });

    while let Some(current) = open_set.pop() {
        if current.node_id == end_node_id {
            let path_internal = reconstruct_path(
                VisitedKey {
                    node_id: current.node_id,
                    prev_external_id: current.prev_external_id,
                },
                &came_from,
            );

            let path_external = path_internal
                .iter()
                .map(|&id| graph.nodes[id as usize].external_id)
                .collect();
            return Ok(Some(path_external));
        }

        let current_key = VisitedKey {
            node_id: current.node_id,
            prev_external_id: current.prev_external_id,
        };
        if current.cost > *g_score.get(&current_key).unwrap_or(&u32::MAX) {
            continue;
        }

        let current_node_external_id = graph.nodes[current.node_id as usize].external_id;

        for &(neighbor_id, cost) in graph.neighbors(current.node_id) {
            let neighbor_node = &graph.nodes[neighbor_id as usize];
            if Some(neighbor_node.external_id) == current.prev_external_id {
                continue;
            }

            let new_cost = current.cost.saturating_add(cost as u32);
            let neighbor_key = VisitedKey {
                node_id: neighbor_id,
                prev_external_id: Some(current_node_external_id),
            };

            if new_cost < *g_score.get(&neighbor_key).unwrap_or(&u32::MAX) {
                g_score.insert(neighbor_key, new_cost);
                came_from.insert(neighbor_key, current_key);

                let h_cost = heuristic_cost(neighbor_node, end_node, precomputed_costs);

                open_set.push(State {
                    cost: new_cost,
                    estimated_total_cost: new_cost.saturating_add(h_cost),
                    node_id: neighbor_id,
                    prev_external_id: Some(current_node_external_id),
                });
            }
        }
    }
    Ok(None)
}

fn reconstruct_path(
    mut current_key: VisitedKey,
    came_from: &FxHashMap<VisitedKey, VisitedKey>,
) -> Vec<u32> {
    let mut path = vec![current_key.node_id];
    while let Some(&prev_key) = came_from.get(&current_key) {
        path.push(prev_key.node_id);
        current_key = prev_key;
    }
    path.reverse();
    path
}

fn heuristic_cost(a: &RouteNode, b: &RouteNode, precomputed_costs: &PrecomputedCosts) -> u32 {
    let h_segment = (distance(a.lat, a.lon, b.lat, b.lon) / 13.8 * 1000.0) as u32;

    let cost_a_to_z = *precomputed_costs.get(&a.id).unwrap_or(&u32::MAX);
    let cost_b_to_z = *precomputed_costs.get(&b.id).unwrap_or(&u32::MAX);
    let h_final = cost_a_to_z.saturating_sub(cost_b_to_z);

    h_segment.max(h_final)
}
