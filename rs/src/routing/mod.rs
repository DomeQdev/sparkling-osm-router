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
        // ZMIANA: Przekazujemy ID w formacie i64, konwersja nastąpi wewnątrz algorytmu
        find_route_astar(route_graph, start_node_id, end_node_id)
    }
}

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