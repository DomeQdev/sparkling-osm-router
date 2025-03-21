use crate::core::errors::{GraphError, Result};
use crate::core::types::Node;
use crate::routing::{RouteGraph, RouteResult, TurnRestriction, TurnRestrictionData};
use crate::spatial::geometry::{bearing_difference, calculate_bearing, haversine_distance};
use rustc_hash::FxHashMap;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

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

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
struct VisitKey {
    node_id: i64,
    prev_node: i64,
    prev_way: i64,
}

impl VisitKey {
    fn new(node_id: i64, prev_node: Option<i64>, prev_way: Option<i64>) -> Self {
        Self {
            node_id,
            prev_node: prev_node.unwrap_or(0),
            prev_way: prev_way.unwrap_or(0),
        }
    }
}

pub fn find_route_bidirectional_astar(
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

    if start_node_id == end_node_id {
        return Ok(Some(RouteResult {
            nodes: vec![start_node_id],
            ways: vec![],
        }));
    }

    let mut open_set_forward = BinaryHeap::new();
    let mut came_from_forward = FxHashMap::default();
    let mut g_score_forward = FxHashMap::default();
    let mut visited_nodes_forward = FxHashMap::default();
    let mut best_node = None;

    let mut visited_nodes_backward: FxHashMap<VisitKey, bool> = FxHashMap::default();

    let mut best_cost = i64::MAX;

    let mut open_set_backward = BinaryHeap::new();
    let mut came_from_backward = FxHashMap::default();
    let mut g_score_backward = FxHashMap::default();

    g_score_forward.insert(start_node_id, 0);
    g_score_backward.insert(end_node_id, 0);

    open_set_forward.push(NodeWithPrevious {
        node_id: start_node_id,
        previous_node_id: None,
        previous_way_id: None,
        cost: 0,
        estimated_total_cost: heuristic_cost(start_node, end_node),
    });

    open_set_backward.push(NodeWithPrevious {
        node_id: end_node_id,
        previous_node_id: None,
        previous_way_id: None,
        cost: 0,
        estimated_total_cost: heuristic_cost(end_node, start_node),
    });

    let max_iterations = 250_000;
    let mut iterations = 0;
    let mut forward_turn = true;

    while !open_set_forward.is_empty() && !open_set_backward.is_empty() {
        iterations += 1;

        if iterations > max_iterations {
            return Ok(None);
        }

        if forward_turn {
            if let Some(current) = open_set_forward.pop() {
                let current_node_id = current.node_id;
                let current_g_score = *g_score_forward.get(&current_node_id).unwrap_or(&i64::MAX);

                let found_in_backward = visited_nodes_backward
                    .keys()
                    .any(|visit_key| visit_key.node_id == current_node_id);
                if found_in_backward {
                    let backward_cost =
                        *g_score_backward.get(&current_node_id).unwrap_or(&i64::MAX);
                    let total_cost = current_g_score + backward_cost;

                    if total_cost < best_cost {
                        best_cost = total_cost;
                        best_node = Some(current_node_id);
                    }
                }

                if best_node.is_some() && current.cost > best_cost {
                    break;
                }

                let visit_key = VisitKey::new(
                    current_node_id,
                    current.previous_node_id,
                    current.previous_way_id,
                );

                if visited_nodes_forward.contains_key(&visit_key) {
                    forward_turn = !forward_turn;
                    continue;
                }

                visited_nodes_forward.insert(visit_key, true);

                let using_initial_bearing =
                    current_node_id == start_node_id && initial_bearing.is_some();

                if let Some(edges) = graph.adjacency_list.get(&current_node_id) {
                    if using_initial_bearing {
                        process_edges_with_bearing(
                            graph,
                            edges,
                            current,
                            initial_bearing.unwrap(),
                            end_node,
                            &mut open_set_forward,
                            &mut came_from_forward,
                            &mut g_score_forward,
                            current_g_score,
                        );
                    } else {
                        process_edges(
                            graph,
                            edges,
                            current,
                            end_node,
                            &mut open_set_forward,
                            &mut came_from_forward,
                            &mut g_score_forward,
                            current_g_score,
                        );
                    }
                }
            }
        } else {
            if let Some(current) = open_set_backward.pop() {
                let current_node_id = current.node_id;
                let current_g_score = *g_score_backward.get(&current_node_id).unwrap_or(&i64::MAX);

                if visited_nodes_forward.contains_key(&VisitKey::new(current_node_id, None, None)) {
                    let forward_cost = *g_score_forward.get(&current_node_id).unwrap_or(&i64::MAX);
                    let total_cost = current_g_score + forward_cost;

                    if total_cost < best_cost {
                        best_cost = total_cost;
                        best_node = Some(current_node_id);
                    }
                }

                if best_node.is_some() && current.cost > best_cost {
                    break;
                }

                let visit_key = VisitKey::new(
                    current_node_id,
                    current.previous_node_id,
                    current.previous_way_id,
                );

                if visited_nodes_backward.contains_key(&visit_key) {
                    forward_turn = !forward_turn;
                    continue;
                }

                visited_nodes_backward.insert(visit_key, true);

                if let Some(_edges) = graph.adjacency_list.get(&current_node_id) {
                    process_edges_reverse(
                        graph,
                        current,
                        start_node,
                        &mut open_set_backward,
                        &mut came_from_backward,
                        &mut g_score_backward,
                        current_g_score,
                    );
                }
            }
        }

        forward_turn = !forward_turn;
    }

    if let Some(meeting_node) = best_node {
        let (forward_nodes, forward_ways) =
            reconstruct_path_forward(&came_from_forward, start_node_id, meeting_node);
        let (mut backward_nodes, backward_ways) =
            reconstruct_path_backward(&came_from_backward, end_node_id, meeting_node);

        let mut result_nodes = forward_nodes;
        backward_nodes.remove(0);
        result_nodes.extend(backward_nodes);

        let mut result_ways = forward_ways;
        result_ways.extend(backward_ways);

        return Ok(Some(RouteResult {
            nodes: result_nodes,
            ways: result_ways,
        }));
    }

    Ok(None)
}

fn process_edges_reverse(
    graph: &RouteGraph,
    current: NodeWithPrevious,
    target_node: &Node,
    open_set: &mut BinaryHeap<NodeWithPrevious>,
    came_from: &mut FxHashMap<i64, (i64, i64)>,
    g_score: &mut FxHashMap<i64, i64>,
    current_g_score: i64,
) {
    if let Some(reverse_edges) = graph.adjacency_list_reverse.get(&current.node_id) {
        for edge in reverse_edges {
            let to_node = edge.to_node;

            if !is_turn_allowed_reverse(
                graph,
                current.previous_way_id,
                current.previous_node_id,
                current.node_id,
                edge.way_id,
                to_node,
            ) {
                continue;
            }

            let edge_cost = calculate_edge_cost(graph, edge);
            let tentative_g_score = current_g_score + edge_cost;

            if tentative_g_score < *g_score.get(&to_node).unwrap_or(&i64::MAX) {
                came_from.insert(to_node, (current.node_id, edge.way_id));
                g_score.insert(to_node, tentative_g_score);

                if let Some(node) = graph.nodes_map.get(&to_node) {
                    let h_cost = heuristic_cost(node, target_node);

                    open_set.push(NodeWithPrevious {
                        node_id: to_node,
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

fn is_turn_allowed_reverse(
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
            && restriction.to_way == prev_way_id
            && restriction.from_way == next_way_id
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
                && r.to_way == prev_way_id
                && r.restriction_type == TurnRestriction::Mandatory
        })
        .collect();

    if !mandatory_restrictions.is_empty() {
        for mandatory in &mandatory_restrictions {
            if mandatory.from_way == next_way_id {
                return true;
            }
        }

        return false;
    }

    true
}

fn reconstruct_path_forward(
    came_from: &FxHashMap<i64, (i64, i64)>,
    start_node_id: i64,
    end_node_id: i64,
) -> (Vec<i64>, Vec<i64>) {
    let mut path_nodes = vec![end_node_id];
    let mut path_ways = Vec::new();
    let mut current = end_node_id;

    while current != start_node_id {
        if let Some(&(prev_node, way_id)) = came_from.get(&current) {
            path_nodes.push(prev_node);
            path_ways.push(way_id);
            current = prev_node;
        } else {
            break;
        }
    }

    path_nodes.reverse();
    path_ways.reverse();

    (path_nodes, path_ways)
}

fn reconstruct_path_backward(
    came_from: &FxHashMap<i64, (i64, i64)>,
    end_node_id: i64,
    middle_node_id: i64,
) -> (Vec<i64>, Vec<i64>) {
    let mut path_nodes = vec![middle_node_id];
    let mut path_ways = Vec::new();
    let mut current = middle_node_id;

    while current != end_node_id {
        if let Some(&(prev_node, way_id)) = came_from.get(&current) {
            path_nodes.push(prev_node);
            path_ways.push(way_id);
            current = prev_node;
        } else {
            break;
        }
    }

    (path_nodes, path_ways)
}

fn heuristic_cost(node: &Node, end_node: &Node) -> i64 {
    let distance = estimate_distance(node, end_node);
    let heuristic_factor = 25.0;
    return (distance * 1000.0 * heuristic_factor) as i64;
}

fn estimate_distance(node1: &Node, node2: &Node) -> f64 {
    if (node1.lat - node2.lat).abs() < 0.1 && (node1.lon - node2.lon).abs() < 0.1 {
        let lat_avg = (node1.lat + node2.lat) / 2.0;
        let lat_factor = lat_avg.to_radians().cos();
        let d_lat = (node1.lat - node2.lat).abs();
        let d_lon = (node1.lon - node2.lon).abs() * lat_factor;

        return 111.2 * (d_lat * d_lat + d_lon * d_lon).sqrt();
    }
    haversine_distance(node1.lat, node1.lon, node2.lat, node2.lon)
}

fn process_edges(
    graph: &RouteGraph,
    edges: &[crate::routing::RouteEdge],
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
    edges: &[crate::routing::RouteEdge],
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

    let edges_to_process: Vec<&(&crate::routing::RouteEdge, i64)> = if !good_edges.is_empty() {
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

fn calculate_edge_cost(graph: &RouteGraph, edge: &crate::routing::RouteEdge) -> i64 {
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

fn adjust_cost(base_cost: i64, penalty: f64) -> i64 {
    if penalty == 0.0 {
        return i64::MAX / 2;
    }

    base_cost * (penalty as i64)
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
