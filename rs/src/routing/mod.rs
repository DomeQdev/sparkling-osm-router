pub mod algorithm;

use crate::core::errors::{GraphError, Result};
use crate::graph::GraphContainer;
use algorithm::find_route_through_waypoints;

impl GraphContainer {
    pub fn route(&self, profile_id: &str, waypoints: &[i64]) -> Result<Option<Vec<i64>>> {
        if waypoints.len() < 2 {
            return Err(GraphError::RoutingError(
                "At least two waypoints are required for routing.".to_string(),
            ));
        }

        let route_graph = self.profiles.get(profile_id).ok_or_else(|| {
            crate::core::errors::GraphError::ProfileNotFound(profile_id.to_string())
        })?;

        find_route_through_waypoints(route_graph, waypoints)
    }
}

pub fn distance(lat1: f32, lon1: f32, lat2: f32, lon2: f32) -> f32 {
    let lat1_cos = lat1.to_radians().cos();
    let lat2_cos = lat2.to_radians().cos();

    let d_lat_half_sin = ((lat2 - lat1).to_radians() / 2.0).sin();
    let d_lon_half_sin = ((lon2 - lon1).to_radians() / 2.0).sin();

    let a = d_lat_half_sin * d_lat_half_sin + lat1_cos * lat2_cos * d_lon_half_sin * d_lon_half_sin;

    12742.0 * a.sqrt().asin()
}
