use crate::errors::Result;
use crate::graph::Node;
use crate::graph::{Graph, WayEnvelope};
use crate::routing::{RouteEdge, RouteGraph, TurnRestriction, TurnRestrictionData};
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

    update_graph_nodes(&graph);
    index_restricted_nodes(&graph);

    Ok(graph)
}

fn build_routing_graph(graph: &Graph) -> RouteGraph {
    let mut adjacency_list: FxHashMap<i64, Vec<RouteEdge>> = FxHashMap::default();
    let mut turn_restrictions = Vec::new();

    crate::routing::thread_local_turn_restrictions_mut(|tr| {
        turn_restrictions = tr.clone();
    });

    let vehicle_type = graph
        .profile
        .as_ref()
        .and_then(|profile| profile.vehicle_type.clone());

    for way in graph.ways.values() {
        let is_oneway = if way
            .tags
            .get("junction")
            .map_or(false, |v| v == "roundabout")
        {
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
                let distance =
                    crate::routing::haversine_distance(node1.lat, node1.lon, node2.lat, node2.lon);

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
                            let except = relation
                                .tags
                                .get("except")
                                .map(|e| e.split(';').map(String::from).collect())
                                .unwrap_or_else(HashSet::new);

                            turn_restrictions.push(TurnRestrictionData {
                                restriction_type,
                                from_way: from,
                                via_node: via,
                                to_way: to,
                                except,
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
        vehicle_type,
    }
}

fn filter_graph(graph: &mut Graph) {
    let profile = graph.profile.clone().expect("Profile must be set");

    graph.ways.retain(|_, way| {
        let has_profile_key = way.tags.contains_key(&profile.key);

        if profile.vehicle_type.is_none() {
            return has_profile_key;
        }

        let vehicle_type = profile.vehicle_type.as_ref().unwrap();

        let access_value = way.tags.get("access");

        if access_value.map_or(false, |v| v == "no") {
            match vehicle_type.as_str() {
                "foot" => way
                    .tags
                    .get("foot")
                    .map_or(false, |v| v == "yes" || v == "designated"),
                "bicycle" => way
                    .tags
                    .get("bicycle")
                    .map_or(false, |v| v == "yes" || v == "designated"),
                "motorcar" => way
                    .tags
                    .get("motorcar")
                    .map_or(false, |v| v == "yes" || v == "designated"),
                "motorcycle" => way
                    .tags
                    .get("motorcycle")
                    .map_or(false, |v| v == "yes" || v == "designated"),
                "psv" => {
                    way.tags
                        .get("psv")
                        .map_or(false, |v| v == "yes" || v == "designated")
                        || way
                            .tags
                            .get("bus")
                            .map_or(false, |v| v == "yes" || v == "designated")
                        || way
                            .tags
                            .get("minibus")
                            .map_or(false, |v| v == "yes" || v == "designated")
                        || way
                            .tags
                            .get("tourist_bus")
                            .map_or(false, |v| v == "yes" || v == "designated")
                        || way
                            .tags
                            .get("coach")
                            .map_or(false, |v| v == "yes" || v == "designated")
                }
                "train" => way
                    .tags
                    .get("train")
                    .map_or(false, |v| v == "yes" || v == "designated"),
                "subway" => way
                    .tags
                    .get("subway")
                    .map_or(false, |v| v == "yes" || v == "designated"),
                "tram" => way
                    .tags
                    .get("tram")
                    .map_or(false, |v| v == "yes" || v == "designated"),
                _ => false,
            }
        } else {
            let vehicle_access = match vehicle_type.as_str() {
                "foot" => way.tags.get("foot"),
                "bicycle" => way.tags.get("bicycle"),
                "motorcar" => way.tags.get("motorcar"),
                "motorcycle" => way.tags.get("motorcycle"),
                "psv" => way
                    .tags
                    .get("psv")
                    .or(way.tags.get("bus"))
                    .or(way.tags.get("minibus"))
                    .or(way.tags.get("tourist_bus"))
                    .or(way.tags.get("coach")),
                "train" => way.tags.get("train"),
                "subway" => way.tags.get("subway"),
                "tram" => way.tags.get("tram"),
                _ => None,
            };

            if vehicle_access.map_or(false, |v| v == "no" || v == "permissive" || v == "private") {
                return false;
            }

            has_profile_key
        }
    });

    let used_node_ids: HashSet<i64> = graph
        .ways
        .values()
        .flat_map(|way| way.node_refs.iter().cloned())
        .collect();

    graph
        .nodes
        .retain(|node_id, _| used_node_ids.contains(node_id));

    graph.relations.retain(|_, relation| {
        relation
            .tags
            .get("type")
            .map_or(false, |type_tag| type_tag == "restriction")
    });
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
    relation: &crate::graph::Relation,
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
            .map(|s| s.trim())
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
                        except,
                    });
                });

                return true;
            }
        }
    }

    false
}

fn calculate_way_envelope(
    way: &crate::graph::Way,
    nodes: &HashMap<i64, Node>,
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
