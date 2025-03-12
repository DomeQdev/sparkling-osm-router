use crate::core::types::Graph;
use crate::routing::RouteGraph;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    pub static DISTANCE_CACHE: RefCell<FxHashMap<(i64, i64), i64>> = RefCell::new(FxHashMap::default());
}

#[derive(Debug, Clone)]
pub struct DistanceMatrix {
    distances: FxHashMap<(i64, i64), i64>,
}

impl DistanceMatrix {
    pub fn new() -> Self {
        DistanceMatrix {
            distances: FxHashMap::default(),
        }
    }

    pub fn add(&mut self, from: i64, to: i64, distance: i64) {
        self.distances.insert((from, to), distance);
    }

    pub fn get_distance(&self, from: i64, to: i64) -> Option<&i64> {
        self.distances.get(&(from, to))
    }
}

pub fn precompute_landmarks(graph: &Graph, landmark_count: usize) -> Vec<i64> {
    let mut candidates: Vec<_> = graph
        .nodes
        .iter()
        .map(|(id, node)| (*id, node))
        .filter(|(_, node)| {
            let is_junction = node.tags.contains_key("highway")
                && (node.tags.get("highway").unwrap() == "motorway_junction"
                    || node.tags.get("highway").unwrap() == "crossing");
            is_junction
        })
        .collect();

    if candidates.len() < landmark_count {
        if let (Some(min_lat), Some(max_lat), Some(min_lon), Some(max_lon)) = (
            graph
                .nodes
                .values()
                .map(|n| n.lat)
                .min_by(|a, b| a.partial_cmp(b).unwrap()),
            graph
                .nodes
                .values()
                .map(|n| n.lat)
                .max_by(|a, b| a.partial_cmp(b).unwrap()),
            graph
                .nodes
                .values()
                .map(|n| n.lon)
                .min_by(|a, b| a.partial_cmp(b).unwrap()),
            graph
                .nodes
                .values()
                .map(|n| n.lon)
                .max_by(|a, b| a.partial_cmp(b).unwrap()),
        ) {
            let lat_range = max_lat - min_lat;
            let lon_range = max_lon - min_lon;

            let grid_size = (landmark_count as f64).sqrt().ceil() as usize;
            let mut grid = vec![vec![None; grid_size]; grid_size];

            for (id, node) in graph.nodes.iter() {
                let grid_x =
                    ((node.lon - min_lon) / lon_range * (grid_size as f64 - 1.0)).round() as usize;
                let grid_y =
                    ((node.lat - min_lat) / lat_range * (grid_size as f64 - 1.0)).round() as usize;

                let grid_x = grid_x.min(grid_size - 1);
                let grid_y = grid_y.min(grid_size - 1);

                if grid[grid_y][grid_x].is_none() {
                    grid[grid_y][grid_x] = Some((*id, node));
                }
            }

            for row in grid.iter() {
                for cell in row.iter() {
                    if let Some(node_data) = cell {
                        if !candidates.iter().any(|(id, _)| id == &node_data.0) {
                            candidates.push(*node_data);
                        }
                    }
                }
            }
        }
    }

    if candidates.len() < landmark_count {
        let mut degree_map: HashMap<i64, usize> = HashMap::new();

        for way in graph.ways.values() {
            for node_id in &way.node_refs {
                *degree_map.entry(*node_id).or_insert(0) += 1;
            }
        }

        let mut degree_nodes: Vec<_> = degree_map.into_iter().collect();
        degree_nodes.sort_by(|a, b| b.1.cmp(&a.1));

        for (id, _) in degree_nodes {
            if candidates.len() >= landmark_count {
                break;
            }

            if let Some(node) = graph.nodes.get(&id) {
                if !candidates.iter().any(|(cand_id, _)| cand_id == &id) {
                    candidates.push((id, node));
                }
            }
        }
    }

    candidates.truncate(landmark_count);
    candidates.into_iter().map(|(id, _)| id).collect()
}

pub fn compute_distance_cache(route_graph: &RouteGraph, landmarks: &[i64]) -> DistanceMatrix {
    let mut matrix = DistanceMatrix::new();

    for &landmark1 in landmarks.iter() {
        let distances = compute_single_source_distances(route_graph, landmark1);

        for &landmark2 in landmarks.iter() {
            if landmark1 != landmark2 {
                if let Some(&distance) = distances.get(&landmark2) {
                    matrix.add(landmark1, landmark2, distance);
                }
            }
        }
    }

    matrix
}

fn compute_single_source_distances(graph: &RouteGraph, source: i64) -> FxHashMap<i64, i64> {
    let mut distances = FxHashMap::default();
    let mut queue = std::collections::BinaryHeap::new();

    distances.insert(source, 0);
    queue.push(std::cmp::Reverse((0, source)));

    while let Some(std::cmp::Reverse((cost, node))) = queue.pop() {
        if cost > *distances.get(&node).unwrap_or(&i64::MAX) {
            continue;
        }

        if let Some(edges) = graph.adjacency_list.get(&node) {
            for edge in edges {
                let new_cost = cost + edge.cost;
                let next_node = edge.to_node;

                if new_cost < *distances.get(&next_node).unwrap_or(&i64::MAX) {
                    distances.insert(next_node, new_cost);
                    queue.push(std::cmp::Reverse((new_cost, next_node)));
                }
            }
        }
    }

    distances
}
