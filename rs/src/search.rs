use crate::errors::Result;
use crate::graph::{Graph, Node, Way};
use crate::indexer::RESTRICTED_NODES;
use std::collections::HashMap;

impl Graph {
    pub fn find_nearest_ways_and_nodes(
        &self,
        lon: f64,
        lat: f64,
        limit: usize,
    ) -> Result<Vec<i64>> {
        let query_point: [f64; 2] = [lon, lat];
        let actual_limit = limit.max(1);

        let mut candidate_ways = Vec::new();
        for way_envelope in self.way_rtree.nearest_neighbor_iter(&query_point).take(100) {
            candidate_ways.push(way_envelope.way_id);
        }

        let mut candidates = Vec::new();

        for way_id in candidate_ways {
            if let Some((node_id, distance)) =
                find_nearest_point_on_way(&self.ways, &self.nodes, way_id, query_point)
            {
                if !has_mandatory_restriction_conflicts_indexed(node_id) {
                    candidates.push((node_id, distance, way_id));
                    continue;
                }

                if let Some(alternative_node_id) =
                    find_alternative_node_on_way_indexed(&self.ways, way_id, node_id)
                {
                    candidates.push((alternative_node_id, distance, way_id));
                }
            }
        }

        
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        
        if candidates.len() > 1 && actual_limit > 1 {
            
            let closest_node_distance = candidates[0].1;
            
            
            let distance_threshold = closest_node_distance * 3.0;
            
            
            candidates.retain(|(_, distance, _)| *distance <= distance_threshold);
        }
        
        
        let result = candidates
            .into_iter()
            .take(actual_limit)
            .map(|(node_id, _, _)| node_id)
            .collect();

        Ok(result)
    }
}

fn squared_distance(p1: &[f64; 2], p2: &[f64; 2]) -> f64 {
    (p1[0] - p2[0]).powi(2) + (p1[1] - p2[1]).powi(2)
}

fn point_to_segment_distance(p: &[f64; 2], a: &[f64; 2], b: &[f64; 2]) -> f64 {
    let ab_x = b[0] - a[0];
    let ab_y = b[1] - a[1];

    if ab_x.abs() < 1e-10 && ab_y.abs() < 1e-10 {
        return squared_distance(p, a);
    }

    let ap_x = p[0] - a[0];
    let ap_y = p[1] - a[1];

    let t = (ap_x * ab_x + ap_y * ab_y) / (ab_x * ab_x + ab_y * ab_y);

    let t_clamped = t.max(0.0).min(1.0);

    let closest_x = a[0] + t_clamped * ab_x;
    let closest_y = a[1] + t_clamped * ab_y;

    (p[0] - closest_x).powi(2) + (p[1] - closest_y).powi(2)
}

fn find_nearest_point_on_way(
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

fn has_mandatory_restriction_conflicts_indexed(node_id: i64) -> bool {
    RESTRICTED_NODES.with(|restricted| restricted.borrow().contains(&node_id))
}

fn find_alternative_node_on_way_indexed(
    ways: &HashMap<i64, Way>,
    way_id: i64,
    problematic_node_id: i64,
) -> Option<i64> {
    if let Some(way) = ways.get(&way_id) {
        for node_ref in &way.node_refs {
            if *node_ref != problematic_node_id
                && !has_mandatory_restriction_conflicts_indexed(*node_ref)
            {
                return Some(*node_ref);
            }
        }
    }
    None
}
