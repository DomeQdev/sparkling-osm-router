use crate::core::errors::{GraphError, Result};
use crate::core::types::{Node, Profile};
use crate::routing::{Edge, RouteGraph, RouteResult};
use crate::spatial::geometry::haversine_distance;
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
    profile: &crate::core::types::Profile,
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

                if let Some(edges) = graph.adjacency_list.get(&current_node_id) {
                    process_edges(
                        graph,
                        edges,
                        current,
                        end_node,
                        &mut open_set_forward,
                        &mut came_from_forward,
                        &mut g_score_forward,
                        current_g_score,
                        profile,
                    );
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
                        profile,
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

fn calculate_edge_cost(graph: &RouteGraph, edge: &Edge, profile: &Profile) -> i64 {
    let way_info = match graph.ways_info.get(&edge.way_id) {
        Some(info) => info,
        None => return i64::MAX,
    };

    let mut is_accessible = profile.penalties.default.is_some();
    if let Some(access_tags_hierarchy) = &profile.access_tags {
        let mut access_decision_made = false;
        for tag_key in access_tags_hierarchy {
            if let Some(tag_value) = way_info.tags.get(tag_key) {
                if tag_value == "no" || tag_value == "private" || tag_value == "false" {
                    is_accessible = false;
                    access_decision_made = true;
                    break;
                } else if tag_value == "yes"
                    || tag_value == "designated"
                    || tag_value == "true"
                    || tag_value == "permissive"
                {
                    is_accessible = true;
                    access_decision_made = true;
                    break;
                }
            }
        }
        if !access_decision_made {
            if let Some(access_value) = way_info.tags.get("access") {
                if access_value == "no" || access_value == "private" {
                    is_accessible = false;
                } else {
                    is_accessible = true;
                }
            } else {
                is_accessible = profile.penalties.default.is_some();
            }
        }
    } else {
        if let Some(access_value) = way_info.tags.get("access") {
            if access_value == "no" || access_value == "private" {
                is_accessible = false;
            } else {
                is_accessible = true;
            }
        } else {
            is_accessible = profile.penalties.default.is_some();
        }
    }

    if !is_accessible {
        return i64::MAX;
    }

    let speed_kmh = way_info
        .tags
        .get("maxspeed")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(50.0);

    if speed_kmh <= 0.0 {
        return i64::MAX;
    }

    let time_hours = edge.distance / (speed_kmh * 1000.0);
    let mut cost = (time_hours * 3_600_000.0) as i64;

    let mut penalty_applied_specific = false;
    if let Some(way_type_tag_value) = way_info.tags.get(&profile.key) {
        if let Some(penalty_multiplier) = profile.penalties.penalties.get(way_type_tag_value) {
            cost = (cost as f64 * penalty_multiplier) as i64;
            penalty_applied_specific = true;
        }
    }

    if !penalty_applied_specific {
        if let Some(default_penalty_multiplier) = profile.penalties.default {
            cost = (cost as f64 * default_penalty_multiplier) as i64;
        }
    }

    if cost == 0 && edge.distance > 0.0 {
        cost = 1;
    }

    cost
}

fn is_oneway(way_info: &crate::routing::WayInfo, profile: &Profile) -> bool {
    if way_info
        .tags
        .get("junction")
        .map_or(false, |v| v == "roundabout")
    {
        return true;
    }

    if let Some(oneway_tags_config) = &profile.oneway_tags {
        for tag_key in oneway_tags_config {
            if let Some(tag_value) = way_info.tags.get(tag_key) {
                if tag_value == "yes" || tag_value == "true" || tag_value == "1" {
                    return true;
                }
            }
        }
    }

    match way_info.tags.get("oneway") {
        Some(value) => {
            if value == "yes" || value == "true" || value == "1" {
                true
            } else {
                false
            }
        }
        None => false,
    }
}

fn process_edges_reverse(
    graph: &RouteGraph,
    current: NodeWithPrevious,
    target_node: &Node,
    open_set: &mut BinaryHeap<NodeWithPrevious>,
    came_from: &mut FxHashMap<i64, (i64, i64)>,
    g_score: &mut FxHashMap<i64, i64>,
    current_g_score: i64,
    profile: &crate::core::types::Profile,
) {
    if let Some(reverse_edges) = graph.adjacency_list_reverse.get(&current.node_id) {
        for edge in reverse_edges {
            let to_node_id = edge.to_node;

            let way_info = match graph.ways_info.get(&edge.way_id) {
                Some(info) => info,
                None => continue,
            };

            if is_oneway(way_info, profile) {
                if !way_info.tags.get("oneway").map_or(false, |v| v == "-1") {
                    continue;
                }
            }

            if !is_turn_allowed_reverse(
                graph,
                current.previous_way_id,
                current.previous_node_id,
                current.node_id,
                edge.way_id,
                to_node_id,
                profile,
            ) {
                continue;
            }

            let edge_cost = calculate_edge_cost(graph, edge, profile);
            let tentative_g_score = current_g_score + edge_cost;

            if tentative_g_score < *g_score.get(&to_node_id).unwrap_or(&i64::MAX) {
                came_from.insert(to_node_id, (current.node_id, edge.way_id));
                g_score.insert(to_node_id, tentative_g_score);

                if let Some(node) = graph.nodes_map.get(&to_node_id) {
                    let h_cost = heuristic_cost(node, target_node);

                    open_set.push(NodeWithPrevious {
                        node_id: to_node_id,
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
    profile: &crate::core::types::Profile,
) -> bool {
    if previous_way_id.is_none() {
        return true;
    }

    let prev_way_id_unwrapped = previous_way_id.unwrap();

    if let Some(restriction_detail) =
        graph
            .prohibitory_restrictions
            .get(&(next_way_id, current_node_id, prev_way_id_unwrapped))
    {
        if let Some(profile_except_tags) = &profile.except_tags {
            if let Some(restriction_except_tags) = &restriction_detail.except_tags {
                if profile_except_tags
                    .iter()
                    .any(|pet| restriction_except_tags.contains(pet))
                {
                    return true;
                }
            }
        }
        return false;
    }

    if let Some(mandatory_turns) = graph
        .mandatory_to_via
        .get(&(prev_way_id_unwrapped, current_node_id))
    {
        let mut allowed_by_mandatory = false;
        for mandatory_turn in mandatory_turns {
            if mandatory_turn.target_way_id == next_way_id {
                if let Some(profile_except_tags) = &profile.except_tags {
                    if let Some(restriction_except_tags) = &mandatory_turn.except_tags {
                        if profile_except_tags
                            .iter()
                            .any(|pet| restriction_except_tags.contains(pet))
                        {
                            continue;
                        }
                    }
                }
                allowed_by_mandatory = true;
                break;
            }
        }
        return allowed_by_mandatory;
    }

    true
}

fn is_turn_allowed(
    graph: &RouteGraph,
    previous_way_id: Option<i64>,
    _prev_prev_node_id: Option<i64>,
    current_node_id: i64,
    next_way_id: i64,
    _next_node_id: i64,
    profile: &crate::core::types::Profile,
) -> bool {
    if previous_way_id.is_none() {
        return true;
    }

    let prev_way_id_unwrapped = previous_way_id.unwrap();

    if let Some(restriction_detail) =
        graph
            .prohibitory_restrictions
            .get(&(prev_way_id_unwrapped, current_node_id, next_way_id))
    {
        if let Some(profile_except_tags) = &profile.except_tags {
            if let Some(restriction_except_tags) = &restriction_detail.except_tags {
                if profile_except_tags
                    .iter()
                    .any(|pet| restriction_except_tags.contains(pet))
                {
                    return true;
                }
            }
        }
        return false;
    }

    if let Some(mandatory_turns) = graph
        .mandatory_from_via
        .get(&(prev_way_id_unwrapped, current_node_id))
    {
        let mut allowed_by_mandatory = false;
        for mandatory_turn in mandatory_turns {
            if mandatory_turn.target_way_id == next_way_id {
                if let Some(profile_except_tags) = &profile.except_tags {
                    if let Some(restriction_except_tags) = &mandatory_turn.except_tags {
                        if profile_except_tags
                            .iter()
                            .any(|pet| restriction_except_tags.contains(pet))
                        {
                            continue;
                        }
                    }
                }
                allowed_by_mandatory = true;
                break;
            }
        }

        return allowed_by_mandatory;
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
    edges: &[Edge],
    current: NodeWithPrevious,
    end_node: &Node,
    open_set: &mut BinaryHeap<NodeWithPrevious>,
    came_from: &mut FxHashMap<i64, (i64, i64)>,
    g_score: &mut FxHashMap<i64, i64>,
    current_g_score: i64,
    profile: &Profile,
) {
    for edge in edges {
        let to_node_id = edge.to_node;

        let way_info = match graph.ways_info.get(&edge.way_id) {
            Some(info) => info,
            None => continue,
        };

        if is_oneway(way_info, profile) {
            if way_info.tags.get("oneway").map_or(false, |v| v == "-1") {
                continue;
            }
        }

        if !is_turn_allowed(
            graph,
            current.previous_way_id,
            current.previous_node_id,
            current.node_id,
            edge.way_id,
            to_node_id,
            profile,
        ) {
            continue;
        }

        let edge_cost = calculate_edge_cost(graph, edge, profile);
        let tentative_g_score = current_g_score + edge_cost;

        if tentative_g_score < *g_score.get(&edge.to_node).unwrap_or(&i64::MAX) {
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
