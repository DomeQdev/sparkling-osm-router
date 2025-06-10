mod algorithm;

use crate::core::errors::{GraphError, Result};
use crate::core::types::{Graph, Node, Profile};
pub use algorithm::find_route_bidirectional_astar;
use rayon;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::OnceLock;

thread_local! {
    static TURN_RESTRICTIONS: RefCell<Vec<TurnRestrictionData>> = RefCell::new(Vec::new());
}

pub fn thread_local_turn_restrictions_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut Vec<TurnRestrictionData>) -> R,
{
    TURN_RESTRICTIONS.with(|tr| f(&mut tr.borrow_mut()))
}

#[derive(Clone, Debug)]
pub struct RouteResult {
    pub nodes: Vec<i64>,
    pub ways: Vec<i64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnRestriction {
    Inapplicable,
    Prohibitory,
    Mandatory,
}

#[derive(Clone, Debug)]
pub struct TurnRestrictionData {
    pub restriction_type: TurnRestriction,
    pub from_way: i64,
    pub via_node: i64,
    pub to_way: i64,
    pub except_tags: Option<HashSet<String>>,
}

#[derive(Clone, Debug)]
pub struct RestrictionDetail {
    pub except_tags: Option<HashSet<String>>,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub to_node: i64,
    pub way_id: i64,
    pub distance: f64,
}

#[derive(Clone, Debug)]
pub struct MandatoryTurnInfo {
    pub target_way_id: i64,
    pub except_tags: Option<HashSet<String>>,
}

#[derive(Debug, Clone)]
pub struct WayInfo {
    pub tags: FxHashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct RouteGraph {
    pub adjacency_list: FxHashMap<i64, Vec<Edge>>,
    pub adjacency_list_reverse: FxHashMap<i64, Vec<Edge>>,
    pub prohibitory_restrictions: FxHashMap<(i64, i64, i64), RestrictionDetail>,
    pub mandatory_from_via: FxHashMap<(i64, i64), Vec<MandatoryTurnInfo>>,
    pub mandatory_to_via: FxHashMap<(i64, i64), Vec<MandatoryTurnInfo>>,
    pub nodes_map: FxHashMap<i64, Node>,
    pub ways_info: FxHashMap<i64, WayInfo>,
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

        find_route_bidirectional_astar(route_graph, start_node_id, end_node_id, profile)
    }
}
