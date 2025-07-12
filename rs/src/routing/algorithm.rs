use crate::core::errors::{GraphError, Result};
use crate::graph::{ProcessedGraph, RouteNode};
use crate::routing::haversine_distance;
use rustc_hash::FxHashMap;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Copy, Clone, Eq, PartialEq)]
struct State {
    cost: u32,
    estimated_total_cost: u32,
    node_id: u32, // ZMIANA: Praca na wewnętrznych ID
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

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
struct VisitedKey {
    node_id: u32, // ZMIANA: Praca na wewnętrznych ID
    prev_external_id: Option<i64>,
}

pub fn find_route_astar(
    graph: &ProcessedGraph,
    start_osm_id: i64,
    end_osm_id: i64,
) -> Result<Option<Vec<i64>>> {
    // ZMIANA: Konwersja z zewnętrznych OSM ID na wewnętrzne ID
    let start_node_id = *graph
        .node_id_map
        .get(&start_osm_id)
        .ok_or_else(|| GraphError::RoutingError(format!("Start node {} not in graph", start_osm_id)))?;
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
        prev_external_id: None,
    };
    g_score.insert(start_key, 0);

    open_set.push(State {
        cost: 0,
        estimated_total_cost: heuristic_cost(start_node, end_node),
        node_id: start_node_id,
        prev_external_id: None,
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
            // ZMIANA: Konwersja ścieżki z wewnętrznych ID na zewnętrzne OSM ID
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

        // ZMIANA: Użycie nowej, szybkiej metody `neighbors`
        for &(neighbor_id, cost) in graph.neighbors(current.node_id) {
            let neighbor_node = &graph.nodes[neighbor_id as usize];
            if Some(neighbor_node.external_id) == current.prev_external_id {
                continue;
            }

            let new_cost = current.cost.saturating_add(cost);
            let neighbor_key = VisitedKey {
                node_id: neighbor_id,
                prev_external_id: Some(current_node_external_id),
            };

            if new_cost < *g_score.get(&neighbor_key).unwrap_or(&u32::MAX) {
                g_score.insert(neighbor_key, new_cost);
                came_from.insert(neighbor_key, current_key);
                let h_cost = heuristic_cost(neighbor_node, end_node);
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
) -> Vec<u32> { // ZMIANA: Zwraca Vec<u32>
    let mut path = vec![current_key.node_id];
    while let Some(&prev_key) = came_from.get(&current_key) {
        path.push(prev_key.node_id);
        current_key = prev_key;
    }
    path.reverse();
    path
}

fn heuristic_cost(a: &RouteNode, b: &RouteNode) -> u32 {
    (haversine_distance(a.lat, a.lon, b.lat, b.lon) / 13.8 * 1000.0) as u32
}