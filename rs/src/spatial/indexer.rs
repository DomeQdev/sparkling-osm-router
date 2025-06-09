use crate::core::errors::Result;
use crate::core::types::{Graph, Node, WayEnvelope};
use crate::routing::{
    MandatoryTurnInfo, RestrictionDetail, RouteEdge, RouteGraph, TurnRestriction,
    TurnRestrictionData,
};
use rstar::{RTree, AABB};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

thread_local! {
    pub static GRAPH_NODES: RefCell<HashMap<i64, Node>> = RefCell::new(HashMap::new());
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

    graph.route_graph = Some(Arc::new(build_routing_graph(&graph)));

    update_graph_nodes(&graph);

    graph.index_rtree()?;

    Ok(graph)
}

fn build_routing_graph(graph: &Graph) -> RouteGraph {
    let mut adjacency_list: FxHashMap<i64, Vec<RouteEdge>> = FxHashMap::default();
    let mut adjacency_list_reverse: FxHashMap<i64, Vec<RouteEdge>> = FxHashMap::default();
    let mut turn_restrictions_data: Vec<TurnRestrictionData> = Vec::new();

    let mut prohibitory_restrictions: FxHashMap<(i64, i64, i64), RestrictionDetail> =
        FxHashMap::default();
    let mut mandatory_from_via: FxHashMap<(i64, i64), Vec<MandatoryTurnInfo>> =
        FxHashMap::default();
    let mut mandatory_to_via: FxHashMap<(i64, i64), Vec<MandatoryTurnInfo>> = FxHashMap::default();

    crate::routing::thread_local_turn_restrictions_mut(|tr_data| {
        turn_restrictions_data = tr_data.clone();
    });

    for restriction_data in &turn_restrictions_data {
        let from_way = restriction_data.from_way;
        let via_node = restriction_data.via_node;
        let to_way = restriction_data.to_way;
        let except_tags_clone = restriction_data.except_tags.clone();

        match restriction_data.restriction_type {
            TurnRestriction::Prohibitory => {
                prohibitory_restrictions.insert(
                    (from_way, via_node, to_way),
                    RestrictionDetail {
                        except_tags: except_tags_clone,
                    },
                );
            }
            TurnRestriction::Mandatory => {
                mandatory_from_via
                    .entry((from_way, via_node))
                    .or_default()
                    .push(MandatoryTurnInfo {
                        target_way_id: to_way,
                        except_tags: except_tags_clone.clone(),
                    });

                mandatory_to_via
                    .entry((to_way, via_node))
                    .or_default()
                    .push(MandatoryTurnInfo {
                        target_way_id: from_way,
                        except_tags: except_tags_clone,
                    });
            }
            _ => {}
        }
    }

    let profile = graph.profile.as_ref().expect("Profile must be set");

    for way in graph.ways.values() {
        let is_roundabout = way
            .tags
            .get("junction")
            .map_or(false, |v| v == "roundabout");

        let mut is_oneway = false;
        if let Some(oneway_tags) = &profile.oneway_tags {
            for tag_key in oneway_tags {
                if let Some(tag_value) = way.tags.get(tag_key) {
                    is_oneway = tag_value == "yes" || tag_value == "true" || tag_value == "1";
                    break;
                }
            }
        } else {
            if way.tags.get("oneway").map_or(false, |v| v == "no") {
                is_oneway = false;
            } else if is_roundabout {
                is_oneway = true;
            } else {
                is_oneway = way.tags.get("oneway").map_or(false, |v| v == "yes");
            }
        }

        let way_id = way.id;

        let base_cost = {
            if let Some(tag_value) = way.tags.get(&profile.key) {
                if let Some(cost) = profile.penalties.penalties.get(tag_value) {
                    *cost
                } else if let Some(default_cost) = profile.penalties.default {
                    default_cost
                } else {
                    continue;
                }
            } else if let Some(default_cost) = profile.penalties.default {
                default_cost
            } else {
                continue;
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
                (base_cost * 1000.0) as i64
            };

            adjacency_list
                .entry(from_node)
                .or_default()
                .push(RouteEdge {
                    to_node,
                    way_id,
                    cost,
                });

            adjacency_list_reverse
                .entry(to_node)
                .or_default()
                .push(RouteEdge {
                    to_node: from_node,
                    way_id,
                    cost,
                });

            if !is_oneway {
                adjacency_list.entry(to_node).or_default().push(RouteEdge {
                    to_node: from_node,
                    way_id,
                    cost,
                });

                adjacency_list_reverse
                    .entry(from_node)
                    .or_default()
                    .push(RouteEdge {
                        to_node: to_node,
                        way_id,
                        cost,
                    });
            }
        }
    }

    for node_id in graph.nodes.keys() {
        adjacency_list.entry(*node_id).or_default();
        adjacency_list_reverse.entry(*node_id).or_default();
    }

    RouteGraph {
        adjacency_list,
        adjacency_list_reverse,
        prohibitory_restrictions,
        mandatory_from_via,
        mandatory_to_via,
        nodes_map: FxHashMap::from_iter(graph.nodes.clone()),
        ways_map: FxHashMap::from_iter(graph.ways.clone()),
        profile: graph.profile.clone(),
    }
}

fn filter_graph(graph: &mut Graph) {
    let profile = graph.profile.clone().expect("Profile must be set");

    graph.ways.retain(|_, way| {
        let mut is_accessible = profile.penalties.default.is_some();

        if let Some(access_tags_hierarchy) = &profile.access_tags {
            let mut access_decision_made = false;
            for tag_key in access_tags_hierarchy {
                if let Some(tag_value) = way.tags.get(tag_key) {
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
                if let Some(access_value) = way.tags.get("access") {
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
            if let Some(access_value) = way.tags.get("access") {
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
            return false;
        }

        let has_penalty = way
            .tags
            .keys()
            .any(|tag_key| profile.penalties.penalties.contains_key(tag_key))
            || profile.penalties.default.is_some();

        if !has_penalty && !way.tags.contains_key(&profile.key) {
            return false;
        }

        true
    });

    let used_node_ids: HashSet<i64> = graph
        .ways
        .values()
        .flat_map(|way| way.node_refs.iter().cloned())
        .collect();

    graph
        .nodes
        .retain(|node_id, _| used_node_ids.contains(node_id));

    if let Some(profile_except_tags) = &profile.except_tags {
        if !profile_except_tags.is_empty() {
            graph.relations.retain(|_, relation| {
                if let Some(relation_type) = relation.tags.get("type") {
                    if relation_type == "restriction" {
                        if let Some(except_tag_value) = relation.tags.get("except") {
                            let relation_exceptions: HashSet<&str> =
                                except_tag_value.split(';').map(|s| s.trim()).collect();

                            !profile_except_tags
                                .iter()
                                .any(|pet| relation_exceptions.contains(pet.as_str()))
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
        }
    } else {
        graph.relations.retain(|_, relation| {
            relation
                .tags
                .get("type")
                .map_or(false, |type_tag| type_tag == "restriction")
        });
    }
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
    let mut current_relation_except_tags: Option<HashSet<String>> = None;

    if let Some(except_tag_str) = relation.tags.get("except") {
        if !except_tag_str.is_empty() {
            current_relation_except_tags = Some(
                except_tag_str
                    .split(';')
                    .map(|s| s.trim().to_string())
                    .collect(),
            );
        }
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
                crate::routing::thread_local_turn_restrictions_mut(|tr| {
                    tr.push(crate::routing::TurnRestrictionData {
                        restriction_type,
                        from_way: from_id,
                        via_node: via_id,
                        to_way: to_id,
                        except_tags: current_relation_except_tags.clone(),
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
