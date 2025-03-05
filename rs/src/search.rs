use crate::errors::Result;
use crate::graph::{Graph, Node, Way};
use crate::indexer::RESTRICTED_NODES;
use std::collections::HashMap;

impl Graph {
    pub fn find_nearest_way_and_node(&self, lon: f64, lat: f64) -> Result<Option<(i64, i64)>> {
        let query_point: [f64; 2] = [lon, lat];
        let nearest_envelopes = self.way_rtree.nearest_neighbor_iter(&query_point);

        for way_envelope in nearest_envelopes.take(55) {
            if let Some(nearest_node_id) = find_nearest_node_on_way_optimized(
                &self.ways,
                &self.nodes,
                way_envelope.way_id,
                lon,
                lat,
            ) {
                if !has_mandatory_restriction_conflicts_indexed(nearest_node_id) {
                    return Ok(Some((way_envelope.way_id, nearest_node_id)));
                }

                if let Some(alternative_node_id) = find_alternative_node_on_way_indexed(
                    &self.ways,
                    way_envelope.way_id,
                    nearest_node_id,
                ) {
                    return Ok(Some((way_envelope.way_id, alternative_node_id)));
                }
            }
        }

        Ok(None)
    }
}

fn squared_distance(p1: &[f64; 2], p2: &[f64; 2]) -> f64 {
    (p1[0] - p2[0]).powi(2) + (p1[1] - p2[1]).powi(2)
}

fn find_nearest_node_on_way_optimized(
    ways: &HashMap<i64, Way>,
    nodes: &HashMap<i64, Node>,
    way_id: i64,
    lon: f64,
    lat: f64,
) -> Option<i64> {
    let way = ways.get(&way_id)?;
    let way_nodes_refs = &way.node_refs;

    let len = way_nodes_refs.len();
    if len == 0 {
        return None;
    }

    if len == 1 {
        return Some(way_nodes_refs[0]);
    }

    let query_point: [f64; 2] = [lon, lat];
    let mut nearest_node_id: Option<i64> = None;
    let mut min_distance_sq = f64::MAX;

    for node_ref in way_nodes_refs {
        let Some(node) = nodes.get(node_ref) else {
            continue;
        };

        let node_point: [f64; 2] = [node.lon, node.lat];
        let distance_sq = squared_distance(&query_point, &node_point);

        if distance_sq < min_distance_sq {
            min_distance_sq = distance_sq;
            nearest_node_id = Some(*node_ref);
        }
    }

    nearest_node_id
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
