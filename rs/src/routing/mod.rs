mod algorithm;

use crate::core::errors::{GraphError, Result};
use crate::core::types::{Graph, Node, Profile, Way};
use crate::spatial::geometry::haversine_distance;
pub use algorithm::find_route_bidirectional_astar;
use rayon;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::OnceLock;
use tokio::time::{timeout, Duration};

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
pub struct MandatoryTurnInfo {
    pub target_way_id: i64,
    pub except_tags: Option<HashSet<String>>,
}

#[derive(Clone, Debug)]
pub struct RouteEdge {
    pub to_node: i64,
    pub way_id: i64,
    pub cost: i64,
}

#[derive(Clone, Debug)]
pub struct RouteGraph {
    pub adjacency_list: FxHashMap<i64, Vec<RouteEdge>>,
    pub adjacency_list_reverse: FxHashMap<i64, Vec<RouteEdge>>,
    pub prohibitory_restrictions: FxHashMap<(i64, i64, i64), RestrictionDetail>,
    pub mandatory_from_via: FxHashMap<(i64, i64), Vec<MandatoryTurnInfo>>,
    pub mandatory_to_via: FxHashMap<(i64, i64), Vec<MandatoryTurnInfo>>,
    pub nodes_map: FxHashMap<i64, Node>,
    pub ways_map: FxHashMap<i64, Way>,
    pub profile: Option<Profile>,
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
        initial_bearing: Option<f64>,
    ) -> Result<Option<RouteResult>> {
        let routing_graph_arc = match &self.route_graph {
            Some(graph_arc) => graph_arc.clone(),
            None => {
                return Err(GraphError::InvalidOsmData(
                    "Routing graph not built".to_string(),
                ))
            }
        };

        let start_node = self.nodes.get(&start_node_id).ok_or_else(|| {
            GraphError::InvalidOsmData(format!("Start node {} not found", start_node_id))
        })?;
        let end_node = self.nodes.get(&end_node_id).ok_or_else(|| {
            GraphError::InvalidOsmData(format!("End node {} not found", end_node_id))
        })?;

        let direct_distance =
            haversine_distance(start_node.lat, start_node.lon, end_node.lat, end_node.lon);

        let timeout_duration = if direct_distance < 20.0 {
            Duration::from_secs(60)
        } else if direct_distance < 100.0 {
            Duration::from_secs(300)
        } else if direct_distance < 500.0 {
            Duration::from_secs(1200)
        } else {
            Duration::from_secs(1800)
        };

        let route_future = tokio::task::spawn_blocking(move || {
            find_route_bidirectional_astar(
                &routing_graph_arc,
                start_node_id,
                end_node_id,
                initial_bearing,
            )
        });

        match timeout(timeout_duration, route_future).await {
            Ok(result) => match result {
                Ok(route_result) => route_result,
                Err(_) => Err(GraphError::InvalidOsmData(
                    "Task panicked during routing".to_string(),
                )),
            },
            Err(_) => Err(GraphError::InvalidOsmData(
                "Routing operation timed out".to_string(),
            )),
        }
    }
}
