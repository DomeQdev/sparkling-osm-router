pub mod algorithm;

use crate::core::errors::Result;
use crate::graph::GraphContainer;
use algorithm::find_route_astar;

impl GraphContainer {
    pub fn route(
        &self,
        profile_id: &str,
        start_node_id: i64,
        end_node_id: i64,
    ) -> Result<Option<Vec<i64>>> {
        let route_graph = self.profiles.get(profile_id).ok_or_else(|| {
            crate::core::errors::GraphError::ProfileNotFound(profile_id.to_string())
        })?;

        find_route_astar(route_graph, start_node_id, end_node_id)
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
