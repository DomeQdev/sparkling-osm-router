use crate::core::errors::Result;
use crate::core::types::{Graph, Node, WayEnvelope};
use crate::routing::{RouteEdge, RouteGraph, TurnRestriction, TurnRestrictionData};
use crate::spatial::precomputation::{compute_distance_cache, precompute_landmarks};
use rstar::{RTree, AABB};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

thread_local! {
    pub static GRAPH_NODES: RefCell<HashMap<i64, Node>> = RefCell::new(HashMap::new());
    pub static RESTRICTED_NODES: RefCell<HashSet<i64>> = RefCell::new(HashSet::new());
}

impl Graph {
    pub fn index_rtree(&mut self) -> Result<()> {
        let mut envelopes: Vec<WayEnvelope> = Vec::new();

        for way in self.ways.values() {
            if let Some(envelope) = calculate_way_envelope(way, &self.nodes) {
                envelopes.push(WayEnvelope {
                    way_id: way.id,
                    envelope,
                });
            }
        }

        self.way_rtree = RTree::bulk_load(envelopes);

        Ok(())
    }
}

pub fn index_graph(mut graph: Graph) -> Result<Graph> {
    filter_graph(&mut graph);
    process_turn_restrictions(&mut graph);

    graph.index_rtree()?;

    graph.route_graph = Some(build_routing_graph(&graph));

    if let Some(route_graph) = &graph.route_graph {
        let landmark_count = determine_landmark_count(&graph);
        let landmarks = precompute_landmarks(&graph, landmark_count);

        graph.landmarks = Some(landmarks.clone());

        let distance_matrix = compute_distance_cache(route_graph, &landmarks);
        graph.landmark_distances = Some(distance_matrix);
    }

    update_graph_nodes(&graph);
    index_restricted_nodes(&graph);

    Ok(graph)
}

fn determine_landmark_count(graph: &Graph) -> usize {
    let node_count = graph.nodes.len();

    if node_count < 10_000 {
        return 8;
    } else if node_count < 50_000 {
        return 16;
    } else if node_count < 200_000 {
        return 24;
    } else if node_count < 500_000 {
        return 32;
    } else {
        return 48;
    }
}

fn build_routing_graph(graph: &Graph) -> RouteGraph {
    let mut adjacency_list: FxHashMap<i64, Vec<RouteEdge>> = FxHashMap::default();
    let mut turn_restrictions = Vec::new();

    crate::routing::thread_local_turn_restrictions_mut(|tr| {
        turn_restrictions = tr.clone();
    });

    for way in graph.ways.values() {
        let is_roundabout = way
            .tags
            .get("junction")
            .map_or(false, |v| v == "roundabout");

        let is_oneway = if way.tags.get("oneway").map_or(false, |v| v == "no") {
            false
        } else if is_roundabout {
            true
        } else {
            way.tags.get("oneway").map_or(false, |v| v == "yes")
        };
        let way_id = way.id;

        let base_cost = {
            let profile = graph.profile.as_ref().expect("Profile must be set");
            if let Some(tag_value) = way.tags.get(&profile.key) {
                match profile.penalties.penalties.get(tag_value) {
                    Some(cost) => *cost,
                    None => match profile.penalties.default {
                        Some(default_cost) => default_cost,
                        None => continue,
                    },
                }
            } else {
                match profile.penalties.default {
                    Some(default_cost) => default_cost,
                    None => continue,
                }
            }
        };

        for i in 0..way.node_refs.len().saturating_sub(1) {
            let from_node = way.node_refs[i];
            let to_node = way.node_refs[i + 1];

            let cost = if let (Some(node1), Some(node2)) =
                (graph.nodes.get(&from_node), graph.nodes.get(&to_node))
            {
                let distance = crate::spatial::geometry::haversine_distance(
                    node1.lat, node1.lon, node2.lat, node2.lon,
                );

                (distance * 1000.0 * (base_cost as f64)).round() as i64
            } else {
                base_cost * 1000
            };

            adjacency_list
                .entry(from_node)
                .or_default()
                .push(RouteEdge {
                    to_node,
                    way_id,
                    cost,
                });

            if !is_oneway {
                adjacency_list.entry(to_node).or_default().push(RouteEdge {
                    to_node: from_node,
                    way_id,
                    cost,
                });
            }
        }
    }

    for node_id in graph.nodes.keys() {
        adjacency_list.entry(*node_id).or_default();
    }

    for relation in graph.relations.values() {
        if let Some(restriction_type) = relation.tags.get("type") {
            if restriction_type == "restriction" {
                if let Some(restriction_value) = relation.tags.get("restriction") {
                    let restriction_type = if restriction_value.starts_with("no_") {
                        TurnRestriction::Prohibitory
                    } else if restriction_value.starts_with("only_") {
                        TurnRestriction::Mandatory
                    } else {
                        TurnRestriction::Inapplicable
                    };

                    if restriction_type != TurnRestriction::Inapplicable {
                        let mut from_way: Option<i64> = None;
                        let mut via_node: Option<i64> = None;
                        let mut to_way: Option<i64> = None;

                        for member in &relation.members {
                            match member.role.as_str() {
                                "from" if member.member_type == "way" => {
                                    from_way = Some(member.ref_id);
                                }
                                "via" if member.member_type == "node" => {
                                    via_node = Some(member.ref_id);
                                }
                                "to" if member.member_type == "way" => {
                                    to_way = Some(member.ref_id);
                                }
                                _ => {}
                            }
                        }

                        if let (Some(from), Some(via), Some(to)) = (from_way, via_node, to_way) {
                            turn_restrictions.push(TurnRestrictionData {
                                restriction_type,
                                from_way: from,
                                via_node: via,
                                to_way: to,
                            });
                        }
                    }
                }
            }
        }
    }

    RouteGraph {
        adjacency_list,
        turn_restrictions,
        nodes_map: FxHashMap::from_iter(graph.nodes.clone()),
        ways_map: FxHashMap::from_iter(graph.ways.clone()),
        profile: graph.profile.clone(),
        landmarks: None,
        landmark_distances: None,
    }
}

fn filter_graph(graph: &mut Graph) {
    let profile = graph.profile.clone().expect("Profile must be set");
    let vehicle_type = profile.vehicle_type.clone();

    graph.ways.retain(|_, way| {
        if !way.tags.contains_key(&profile.key) {
            return false;
        }

        if way.tags.get("highway").map_or(false, |h| h == "service")
            && way.tags.contains_key("service")
        {
            return false;
        }

        if vehicle_type.is_none() {
            return true;
        }

        let vehicle = vehicle_type.as_ref().unwrap();

        let has_access = match vehicle.as_str() {
            "foot" => {
                check_access_for_vehicle(way, "foot", &["footway", "pedestrian", "path"], &[])
            }
            "bicycle" => check_access_for_vehicle(way, "bicycle", &["cycleway"], &[]),
            "motorcar" => check_access_for_vehicle(
                way,
                "motorcar",
                &["motorway", "motorroad"],
                &["car", "vehicle", "motor_vehicle"],
            ),
            "motorcycle" => check_access_for_vehicle(
                way,
                "motorcycle",
                &["motorway", "motorroad"],
                &["vehicle", "motor_vehicle"],
            ),
            "psv" => {
                check_access_for_vehicle(way, "psv", &[], &[])
                    || check_access_for_vehicle(way, "bus", &["busway"], &[])
                    || check_access_for_vehicle(way, "minibus", &[], &[])
                    || check_access_for_vehicle(way, "tourist_bus", &[], &[])
                    || check_access_for_vehicle(way, "coach", &[], &[])
            }
            "train" => check_access_for_vehicle(way, "train", &["rail"], &[]),
            "subway" => check_access_for_vehicle(way, "subway", &["subway"], &[]),
            "tram" => check_access_for_vehicle(way, "tram", &["tram"], &[]),
            _ => true,
        };

        has_access
    });

    let used_node_ids: HashSet<i64> = graph
        .ways
        .values()
        .flat_map(|way| way.node_refs.iter().cloned())
        .collect();

    graph
        .nodes
        .retain(|node_id, _| used_node_ids.contains(node_id));

    if let Some(vehicle_type) = &graph.profile.as_ref().and_then(|p| p.vehicle_type.clone()) {
        let vehicle = vehicle_type.as_str();

        graph.relations.retain(|_, relation| {
            if let Some(relation_type) = relation.tags.get("type") {
                if relation_type == "restriction" {
                    if let Some(except_tag) = relation.tags.get("except") {
                        let exceptions: HashSet<&str> =
                            except_tag.split(';').map(|s| s.trim()).collect();

                        match vehicle {
                            "foot" => {
                                !exceptions.contains("foot") && !exceptions.contains("pedestrian")
                            }
                            "bicycle" => !exceptions.contains("bicycle"),
                            "motorcar" => {
                                !exceptions.contains("motorcar")
                                    && !exceptions.contains("car")
                                    && !exceptions.contains("motor_vehicle")
                            }
                            "motorcycle" => {
                                !exceptions.contains("motorcycle")
                                    && !exceptions.contains("motor_vehicle")
                            }
                            "psv" => {
                                !exceptions.contains("psv")
                                    && !exceptions.contains("bus")
                                    && !exceptions.contains("minibus")
                                    && !exceptions.contains("tourist_bus")
                                    && !exceptions.contains("coach")
                            }
                            "train" => !exceptions.contains("train"),
                            "subway" => !exceptions.contains("subway"),
                            "tram" => !exceptions.contains("tram"),
                            _ => true,
                        }
                    } else {
                        true
                    }
                } else {
                    false
                }
            } else {
                false
            }
        });
    } else {
        graph.relations.retain(|_, relation| {
            relation
                .tags
                .get("type")
                .map_or(false, |type_tag| type_tag == "restriction")
        });
    }
}

fn check_access_for_vehicle(
    way: &crate::core::types::Way,
    vehicle_tag: &str,
    highway_values: &[&str],
    additional_tags: &[&str],
) -> bool {
    let general_access = way
        .tags
        .get("access")
        .map_or(true, |v| v != "no" && v != "private");

    let dedicated_highway = if !highway_values.is_empty() {
        way.tags
            .get("highway")
            .map_or(false, |h| highway_values.contains(&h.as_str()))
    } else {
        false
    };

    if dedicated_highway {
        return true;
    }

    if let Some(vehicle_value) = way.tags.get(vehicle_tag) {
        if vehicle_value == "yes" || vehicle_value == "designated" {
            return true;
        }
        if vehicle_value == "no" {
            return false;
        }
    }

    for tag in additional_tags {
        if way.tags.get(*tag).map_or(false, |v| v == "no") {
            return false;
        }
    }

    general_access
}

fn process_turn_restrictions(graph: &mut Graph) {
    let mut node_to_ways: HashMap<i64, HashSet<i64>> = HashMap::new();

    for (way_id, way) in &graph.ways {
        for node_id in &way.node_refs {
            node_to_ways.entry(*node_id).or_default().insert(*way_id);
        }
    }

    for relation in graph.relations.values() {
        if let Some(relation_type) = relation.tags.get("type") {
            if relation_type == "restriction" {
                if let Some(restriction_value) = relation.tags.get("restriction") {
                    let restriction_type = if restriction_value.starts_with("no_") {
                        TurnRestriction::Prohibitory
                    } else if restriction_value.starts_with("only_") {
                        TurnRestriction::Mandatory
                    } else {
                        TurnRestriction::Inapplicable
                    };

                    if restriction_type != TurnRestriction::Inapplicable {
                        process_single_restriction(relation, &mut node_to_ways, restriction_type);
                    }
                }
            }
        }
    }
}

fn process_single_restriction(
    relation: &crate::core::types::Relation,
    node_to_ways: &mut HashMap<i64, HashSet<i64>>,
    restriction_type: TurnRestriction,
) -> bool {
    let mut from_way_id: Option<i64> = None;
    let mut via_node_id: Option<i64> = None;
    let mut to_way_id: Option<i64> = None;
    let mut except = HashSet::new();

    if let Some(except_tag) = relation.tags.get("except") {
        except_tag
            .split(';')
            .filter(|s| !s.is_empty())
            .for_each(|s| {
                except.insert(s.to_string());
            });
    }

    for member in &relation.members {
        match member.role.as_str() {
            "from" if member.member_type == "way" => {
                from_way_id = Some(member.ref_id);
            }
            "via" if member.member_type == "node" => {
                via_node_id = Some(member.ref_id);
            }
            "to" if member.member_type == "way" => {
                to_way_id = Some(member.ref_id);
            }
            _ => {}
        }
    }

    if let (Some(from_id), Some(via_id), Some(to_id)) = (from_way_id, via_node_id, to_way_id) {
        if let Some(ways_at_node) = node_to_ways.get(&via_id) {
            if ways_at_node.contains(&from_id) && ways_at_node.contains(&to_id) {
                if !RESTRICTED_NODES.with(|nodes| nodes.borrow_mut().insert(via_id)) {}

                crate::routing::thread_local_turn_restrictions_mut(|tr| {
                    tr.push(crate::routing::TurnRestrictionData {
                        restriction_type,
                        from_way: from_id,
                        via_node: via_id,
                        to_way: to_id,
                    });
                });

                return true;
            }
        }
    }

    false
}

fn calculate_way_envelope(
    way: &crate::core::types::Way,
    nodes: &HashMap<i64, crate::core::types::Node>,
) -> Option<AABB<[f64; 2]>> {
    if way.node_refs.is_empty() {
        return None;
    }

    let mut min_lon = f64::MAX;
    let mut min_lat = f64::MAX;
    let mut max_lon = f64::MIN;
    let mut max_lat = f64::MIN;

    let mut has_valid_coords = false;

    for node_ref in &way.node_refs {
        if let Some(node) = nodes.get(node_ref) {
            min_lon = min_lon.min(node.lon);
            min_lat = min_lat.min(node.lat);
            max_lon = max_lon.max(node.lon);
            max_lat = max_lat.max(node.lat);
            has_valid_coords = true;
        }
    }

    if !has_valid_coords || min_lon > max_lon || min_lat > max_lat {
        return None;
    }

    Some(AABB::from_corners([min_lon, min_lat], [max_lon, max_lat]))
}

fn update_graph_nodes(graph: &Graph) {
    GRAPH_NODES.with(|nodes| {
        let mut nodes_ref = nodes.borrow_mut();
        *nodes_ref = graph.nodes.clone();
    });
}

fn index_restricted_nodes(graph: &Graph) {
    let mut restricted: HashSet<i64> = HashSet::new();

    for relation in graph.relations.values() {
        if relation
            .tags
            .get("type")
            .map_or(false, |t| t == "restriction")
        {
            if relation
                .tags
                .get("restriction")
                .map_or(false, |r| r.starts_with("only_"))
            {
                for member in &relation.members {
                    if member.role == "via" && member.member_type == "node" {
                        restricted.insert(member.ref_id);
                    }
                }
            }
        }
    }

    RESTRICTED_NODES.with(|nodes| {
        let mut nodes_ref = nodes.borrow_mut();
        *nodes_ref = restricted.clone();
    });
}
