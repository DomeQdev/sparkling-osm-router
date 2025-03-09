use crate::errors::{GraphError, Result};
use crate::graph::{Graph, Node, Profile};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
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
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct NodeWithPrevious {
    node_id: i64,
    previous_node_id: Option<i64>,
    previous_way_id: Option<i64>,
    cost: i64,
    estimated_total_cost: i64,
}

impl Ord for NodeWithPrevious {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimated_total_cost
            .cmp(&self.estimated_total_cost)
            .then_with(|| other.cost.cmp(&self.cost))
    }
}

impl PartialOrd for NodeWithPrevious {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
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
    pub turn_restrictions: Vec<TurnRestrictionData>,
    pub nodes_map: FxHashMap<i64, Node>,
    pub ways_map: FxHashMap<i64, crate::graph::Way>,
    pub profile: Option<Profile>,
}

impl Graph {
    pub async fn route(
        &self,
        start_node_id: i64,
        end_node_id: i64,
        initial_bearing: Option<f64>,
    ) -> Result<Option<RouteResult>> {
        let routing_graph = match &self.route_graph {
            Some(graph) => graph.clone(),
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
            find_route_astar(&routing_graph, start_node_id, end_node_id, initial_bearing)
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

fn find_route_astar(
    graph: &RouteGraph,
    start_node_id: i64,
    end_node_id: i64,
    initial_bearing: Option<f64>,
) -> Result<Option<RouteResult>> {
    if !graph.adjacency_list.contains_key(&start_node_id)
        || !graph.adjacency_list.contains_key(&end_node_id)
    {
        return Ok(None);
    }

    let start_node = match graph.nodes_map.get(&start_node_id) {
        Some(node) => node,
        None => {
            return Err(GraphError::InvalidOsmData(format!(
                "Start node {} not found",
                start_node_id
            )));
        }
    };

    let end_node = match graph.nodes_map.get(&end_node_id) {
        Some(node) => node,
        None => {
            return Err(GraphError::InvalidOsmData(format!(
                "End node {} not found",
                end_node_id
            )));
        }
    };

    let direct_distance =
        haversine_distance(start_node.lat, start_node.lon, end_node.lat, end_node.lon);

    let max_iterations = if direct_distance < 20.0 {
        1_000_000
    } else if direct_distance < 100.0 {
        5_000_000
    } else if direct_distance < 500.0 {
        20_000_000
    } else {
        50_000_000
    };

    let mut open_set = BinaryHeap::new();
    let mut came_from = FxHashMap::default();
    let mut g_score = FxHashMap::default();
    let mut visited_nodes = FxHashMap::default();
    let mut first_segment = true;

    g_score.insert(start_node_id, 0);

    open_set.push(NodeWithPrevious {
        node_id: start_node_id,
        previous_node_id: None,
        previous_way_id: None,
        cost: 0,
        estimated_total_cost: heuristic_cost(start_node, end_node),
    });

    let mut iterations = 0;

    while let Some(current) = open_set.pop() {
        iterations += 1;

        if iterations > max_iterations {
            return Ok(None);
        }

        let current_node_id = current.node_id;
        let current_g_score = *g_score.get(&current_node_id).unwrap_or(&i64::MAX);

        let visit_key = (
            current_node_id,
            current.previous_node_id,
            current.previous_way_id,
        );

        if visited_nodes.contains_key(&visit_key) {
            continue;
        }

        if current_node_id == end_node_id {
            let (nodes, ways) = reconstruct_path_with_ways(&came_from, end_node_id);
            return Ok(Some(RouteResult { nodes, ways }));
        }

        visited_nodes.insert(visit_key, true);

        let using_initial_bearing = first_segment && initial_bearing.is_some();
        first_segment = false;

        if let Some(edges) = graph.adjacency_list.get(&current_node_id) {
            if using_initial_bearing {
                process_edges_with_bearing(
                    graph,
                    edges,
                    current,
                    initial_bearing.unwrap(),
                    end_node,
                    &mut open_set,
                    &mut came_from,
                    &mut g_score,
                    current_g_score,
                );
            } else {
                process_edges(
                    graph,
                    edges,
                    current,
                    end_node,
                    &mut open_set,
                    &mut came_from,
                    &mut g_score,
                    current_g_score,
                );
            }
        }
    }

    Ok(None)
}

fn process_edges(
    graph: &RouteGraph,
    edges: &[RouteEdge],
    current: NodeWithPrevious,
    end_node: &Node,
    open_set: &mut BinaryHeap<NodeWithPrevious>,
    came_from: &mut FxHashMap<i64, (i64, i64)>,
    g_score: &mut FxHashMap<i64, i64>,
    current_g_score: i64,
) {
    for edge in edges {
        if Some(edge.to_node) == current.previous_node_id {
            continue;
        }

        let edge_cost = calculate_edge_cost(graph, &edge);
        let tentative_g_score = current_g_score + edge_cost;

        if tentative_g_score < *g_score.get(&edge.to_node).unwrap_or(&i64::MAX) {
            if is_turn_allowed(
                graph,
                current.previous_way_id,
                current.previous_node_id,
                current.node_id,
                edge.way_id,
                edge.to_node,
            ) {
                came_from.insert(edge.to_node, (current.node_id, edge.way_id));
                g_score.insert(edge.to_node, tentative_g_score);

                if let Some(to_node) = graph.nodes_map.get(&edge.to_node) {
                    let h_cost = heuristic_cost(to_node, end_node);

                    open_set.push(NodeWithPrevious {
                        node_id: edge.to_node,
                        previous_node_id: Some(current.node_id),
                        previous_way_id: Some(edge.way_id),
                        cost: tentative_g_score,
                        estimated_total_cost: tentative_g_score + h_cost,
                    });
                }
            }
        }
    }
}

fn process_edges_with_bearing(
    graph: &RouteGraph,
    edges: &[RouteEdge],
    current: NodeWithPrevious,
    desired_bearing: f64,
    end_node: &Node,
    open_set: &mut BinaryHeap<NodeWithPrevious>,
    came_from: &mut FxHashMap<i64, (i64, i64)>,
    g_score: &mut FxHashMap<i64, i64>,
    current_g_score: i64,
) {
    let current_node = &graph.nodes_map[&current.node_id];
    let mut filtered_edges = Vec::with_capacity(edges.len());

    for edge in edges {
        if let Some(to_node) = graph.nodes_map.get(&edge.to_node) {
            let edge_bearing =
                calculate_bearing(current_node.lat, current_node.lon, to_node.lat, to_node.lon);
            let bearing_diff = bearing_difference(desired_bearing, edge_bearing).abs();

            if bearing_diff <= 90.0 {
                let bearing_score = ((90.0 - bearing_diff) / 90.0 * 100.0) as i64;
                filtered_edges.push((edge, bearing_score));
            } else {
                filtered_edges.push((edge, -1));
            }
        }
    }

    filtered_edges.sort_by(|a, b| b.1.cmp(&a.1));

    let good_edges: Vec<_> = filtered_edges
        .iter()
        .filter(|(_, score)| *score > 0)
        .collect();

    let edges_to_process: Vec<&(&RouteEdge, i64)> = if !good_edges.is_empty() {
        if good_edges.len() > 3 {
            good_edges[0..3].to_vec()
        } else {
            good_edges
        }
    } else {
        filtered_edges.iter().collect()
    };

    for edge_ref in &edges_to_process {
        let (edge, _) = *edge_ref;
        let edge_cost = calculate_edge_cost(graph, &edge);
        let tentative_g_score = current_g_score + edge_cost;

        if tentative_g_score < *g_score.get(&edge.to_node).unwrap_or(&i64::MAX) {
            if is_turn_allowed(
                graph,
                current.previous_way_id,
                current.previous_node_id,
                current.node_id,
                edge.way_id,
                edge.to_node,
            ) {
                came_from.insert(edge.to_node, (current.node_id, edge.way_id));
                g_score.insert(edge.to_node, tentative_g_score);

                if let Some(to_node) = graph.nodes_map.get(&edge.to_node) {
                    let h_cost = heuristic_cost(to_node, end_node);

                    open_set.push(NodeWithPrevious {
                        node_id: edge.to_node,
                        previous_node_id: Some(current.node_id),
                        previous_way_id: Some(edge.way_id),
                        cost: tentative_g_score,
                        estimated_total_cost: tentative_g_score + h_cost,
                    });
                }
            }
        }
    }
}

fn heuristic_cost(node: &Node, end_node: &Node) -> i64 {
    let distance = haversine_distance(node.lat, node.lon, end_node.lat, end_node.lon);

    let heuristic_factor = 25.0;

    return (distance * 1000.0 * heuristic_factor) as i64;
}

fn calculate_edge_cost(graph: &RouteGraph, edge: &RouteEdge) -> i64 {
    let base_cost = edge.cost;

    if graph.profile.is_none() || !graph.ways_map.contains_key(&edge.way_id) {
        return base_cost;
    }

    let profile = graph.profile.as_ref().unwrap();
    let way_id = edge.way_id;

    if let Some(way) = graph.ways_map.get(&way_id) {
        if let Some(tag_value) = way.tags.get(&profile.key) {
            if let Some(penalty) = profile.penalties.penalties.get(tag_value) {
                return adjust_cost(base_cost, *penalty);
            }
        }

        if let Some(default_penalty) = profile.penalties.default {
            return adjust_cost(base_cost, default_penalty);
        }
    }

    base_cost
}

fn adjust_cost(base_cost: i64, penalty: i64) -> i64 {
    if penalty == 0 {
        return i64::MAX / 2;
    }

    base_cost * penalty / 10
}

fn reconstruct_path_with_ways(
    came_from: &FxHashMap<i64, (i64, i64)>,
    end_node_id: i64,
) -> (Vec<i64>, Vec<i64>) {
    let mut path_nodes = vec![end_node_id];
    let mut path_ways = Vec::new();
    let mut current = end_node_id;

    while let Some((prev_node, way_id)) = came_from.get(&current) {
        path_nodes.push(*prev_node);
        path_ways.push(*way_id);
        current = *prev_node;
    }

    path_nodes.reverse();
    path_ways.reverse();

    let mut deduped_ways = Vec::with_capacity(path_ways.len());
    let mut prev_way: Option<i64> = None;

    for way_id in path_ways {
        if prev_way != Some(way_id) {
            deduped_ways.push(way_id);
            prev_way = Some(way_id);
        }
    }

    (path_nodes, deduped_ways)
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

fn calculate_bearing(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1_rad = lat1.to_radians();
    let lon1_rad = lon1.to_radians();
    let lat2_rad = lat2.to_radians();
    let lon2_rad = lon2.to_radians();

    let dlon = lon2_rad - lon1_rad;

    let y = dlon.sin() * lat2_rad.cos();
    let x = lat1_rad.cos() * lat2_rad.sin() - lat1_rad.sin() * lat2_rad.cos() * dlon.cos();

    let bearing_rad = y.atan2(x);
    let mut bearing_deg = bearing_rad.to_degrees();

    if bearing_deg < 0.0 {
        bearing_deg += 360.0;
    }

    bearing_deg
}

fn bearing_difference(bearing1: f64, bearing2: f64) -> f64 {
    let mut diff = bearing2 - bearing1;
    while diff > 180.0 {
        diff -= 360.0;
    }
    while diff < -180.0 {
        diff += 360.0;
    }
    diff
}

fn is_turn_allowed(
    graph: &RouteGraph,
    previous_way_id: Option<i64>,
    _prev_prev_node_id: Option<i64>,
    current_node_id: i64,
    next_way_id: i64,
    _next_node_id: i64,
) -> bool {
    if previous_way_id.is_none() {
        return true;
    }

    let prev_way_id = previous_way_id.unwrap();

    for restriction in &graph.turn_restrictions {
        if restriction.via_node == current_node_id
            && restriction.from_way == prev_way_id
            && restriction.to_way == next_way_id
        {
            if restriction.restriction_type == TurnRestriction::Prohibitory {
                return false;
            }
        }
    }

    let mandatory_restrictions: Vec<&TurnRestrictionData> = graph
        .turn_restrictions
        .iter()
        .filter(|r| {
            r.via_node == current_node_id
                && r.from_way == prev_way_id
                && r.restriction_type == TurnRestriction::Mandatory
        })
        .collect();

    if !mandatory_restrictions.is_empty() {
        for mandatory in &mandatory_restrictions {
            if mandatory.to_way == next_way_id {
                return true;
            }
        }

        return false;
    }

    true
}
