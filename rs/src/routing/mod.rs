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

pub fn distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0;
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let dlon_rad = (lon2 - lon1).to_radians();

    let x = dlon_rad * ((lat1_rad + lat2_rad) / 2.0).cos();
    let dlat_rad = lat2_rad - lat1_rad;

    r * (x.powi(2) + dlat_rad.powi(2)).sqrt()
}
