pub mod algorithm;
pub use algorithm::find_route_astar;

use crate::core::errors::{GraphError, Result};
use crate::core::types::{Graph, Node, Profile, Way};
use rayon;
use rustc_hash::FxHashMap;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct RouteResult {
    pub nodes: Vec<i64>,
    pub ways: Vec<i64>,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub to_node: i64,
    pub way_id: i64,
    pub distance: f64,
}

#[derive(Debug, Clone, Default)]
pub struct RouteGraph {
    pub adjacency_list: FxHashMap<i64, Vec<Edge>>,
    pub nodes: FxHashMap<i64, Node>,
    pub ways: FxHashMap<i64, Way>,
}

pub static ROUTING_THREAD_POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();

pub fn init_routing_thread_pool() {
    ROUTING_THREAD_POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(num_cpus::get())
            .thread_name(|i| format!("routing-worker-{}", i))
            .build()
            .expect("Failed to create routing thread pool")
    });
}

impl Graph {
    pub async fn route(
        &self,
        start_node_id: i64,
        end_node_id: i64,
        profile: &Profile,
    ) -> Result<Option<RouteResult>> {
        let route_graph = self.route_graph.as_ref().ok_or_else(|| {
            GraphError::GraphNotIndexed("Route graph is not available".to_string())
        })?;

        find_route_astar(route_graph, start_node_id, end_node_id, profile)
    }
}
