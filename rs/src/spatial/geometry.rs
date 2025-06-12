use crate::core::errors::Result;
use crate::core::types::{Graph, Profile};
use crate::routing::algorithm::is_way_accessible;

pub fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0;
    let lat1_rad = lat1.to_radians();
    let lon1_rad = lon1.to_radians();
    let lat2_rad = lat2.to_radians();
    let lon2_rad = lon2.to_radians();

    let dlat = lat2_rad - lat1_rad;
    let dlon = lon2_rad - lon1_rad;

    let a =
        (dlat / 2.0).sin().powi(2) + lat1_rad.cos() * lat2_rad.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    r * c
}

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
}
