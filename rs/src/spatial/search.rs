use crate::core::errors::Result;
use crate::core::types::{Graph, Node, Way};
use crate::spatial::geometry::{point_to_segment_distance, squared_distance};
use std::collections::HashMap;

impl Graph {
    pub fn find_nearest_ways_and_nodes(
        &self,
        lon: f64,
        lat: f64,
        limit: usize,
        max_distance: f64,
    ) -> Result<Vec<i64>> {
        let query_point: [f64; 2] = [lon, lat];
        let actual_limit = limit.max(1);

        let mut candidate_ways = Vec::with_capacity(100);
        for way_envelope in self.way_rtree.nearest_neighbor_iter(&query_point).take(100) {
            candidate_ways.push(way_envelope.way_id);
        }

        let mut candidates = Vec::with_capacity(candidate_ways.len());

        for way_id in candidate_ways {
            if let Some((node_id, distance)) =
                find_nearest_point_on_way(&self.ways, &self.nodes, way_id, query_point)
            {
                candidates.push((node_id, distance));
            }
        }

        candidates
            .sort_unstable_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        if candidates.len() > 1 && actual_limit > 1 {
            let mut valid_count = 0;
            for i in 0..candidates.len() {
                if candidates[i].1 <= max_distance {
                    if i != valid_count {
                        candidates.swap(i, valid_count);
                    }
                    valid_count += 1;
                }
            }
            candidates.truncate(valid_count);
        }

        let result_len = candidates.len().min(actual_limit);
        let mut result = Vec::with_capacity(result_len);

        for i in 0..result_len {
            result.push(candidates[i].0);
        }

        Ok(result)
    }

    pub fn find_nodes_by_tags_and_location(
        &self,
        lon: f64,
        lat: f64,
        search_string: &str,
        max_distance: f64,
    ) -> Result<Option<(i64, f64)>> {
        let query_point: [f64; 2] = [lon, lat];

        let mut candidate_ways = Vec::with_capacity(100);
        for way_envelope in self.way_rtree.nearest_neighbor_iter(&query_point).take(100) {
            candidate_ways.push(way_envelope.way_id);
        }

        let mut candidates = Vec::new();
        for way_id in candidate_ways {
            if let Some(way) = self.ways.get(&way_id) {
                for &node_id in &way.node_refs {
                    if let Some(node) = self.nodes.get(&node_id) {
                        let node_point: [f64; 2] = [node.lon, node.lat];
                        let distance = squared_distance(&query_point, &node_point).sqrt();

                        if distance <= max_distance {
                            candidates.push((node_id, distance));
                        }
                    }
                }
            }
        }

        candidates.sort_unstable_by_key(|&(id, _)| id);
        candidates.dedup_by_key(|&mut (id, _)| id);

        if candidates.is_empty() {
            return Ok(None);
        }

        let lowercase_search = search_string.to_lowercase();
        let search_terms: Vec<&str> = lowercase_search.split_whitespace().collect();

        if search_terms.is_empty() {
            candidates.sort_unstable_by(|a, b| {
                a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
            });
            return Ok(Some((candidates[0].0, candidates[0].1)));
        }

        let mut scored_nodes = Vec::new();
        for &(node_id, distance) in &candidates {
            if let Some(node) = self.nodes.get(&node_id) {
                let score = calculate_node_match_score(node, &search_terms);
                if score > 0.0 {
                    scored_nodes.push((node_id, score, distance));
                }
            }
        }

        if scored_nodes.is_empty() {
            candidates.sort_unstable_by(|a, b| {
                a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
            });
            return Ok(Some((candidates[0].0, candidates[0].1)));
        }

        scored_nodes.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
        });

        Ok(Some((scored_nodes[0].0, scored_nodes[0].2)))
    }
}

fn calculate_node_match_score(node: &Node, search_terms: &[&str]) -> f64 {
    let mut total_score = 0.0;

    for (_key, value) in &node.tags {
        let lowercase_value = value.to_lowercase();

        for &term in search_terms {
            if lowercase_value == term {
                total_score += 1.0;
                continue;
            }

            if lowercase_value.contains(term) {
                total_score += 0.5;
            }
        }
    }

    total_score
}

pub fn find_nearest_point_on_way(
    ways: &HashMap<i64, Way>,
    nodes: &HashMap<i64, Node>,
    way_id: i64,
    query_point: [f64; 2],
) -> Option<(i64, f64)> {
    let way = ways.get(&way_id)?;
    let way_nodes_refs = &way.node_refs;

    let len = way_nodes_refs.len();
    if len == 0 {
        return None;
    }

    if len == 1 {
        let node_id = way_nodes_refs[0];
        let node = nodes.get(&node_id)?;
        let node_point: [f64; 2] = [node.lon, node.lat];
        let distance = squared_distance(&query_point, &node_point).sqrt();
        return Some((node_id, distance));
    }

    let mut min_distance = f64::MAX;
    let mut nearest_node_id = None;

    for i in 0..len - 1 {
        let node1_id = way_nodes_refs[i];
        let node2_id = way_nodes_refs[i + 1];

        let Some(node1) = nodes.get(&node1_id) else {
            continue;
        };
        let Some(node2) = nodes.get(&node2_id) else {
            continue;
        };

        let point1: [f64; 2] = [node1.lon, node1.lat];
        let point2: [f64; 2] = [node2.lon, node2.lat];

        let segment_distance_sq = point_to_segment_distance(&query_point, &point1, &point2);
        let segment_distance = segment_distance_sq.sqrt();

        if segment_distance < min_distance {
            min_distance = segment_distance;

            let dist_to_node1 = squared_distance(&query_point, &point1).sqrt();
            let dist_to_node2 = squared_distance(&query_point, &point2).sqrt();

            nearest_node_id = Some(if dist_to_node1 <= dist_to_node2 {
                node1_id
            } else {
                node2_id
            });
        }
    }

    nearest_node_id.map(|id| (id, min_distance))
}
