use crate::core::errors::Result;
use crate::core::types::{Graph, Node, WayEnvelope};
use crate::routing::{
    Edge as RouteEdge, MandatoryTurnInfo, RestrictionDetail, RouteGraph, TurnRestriction,
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

    for way in graph.ways.values() {
        let way_id = way.id;

        for i in 0..way.node_refs.len().saturating_sub(1) {
            let from_node = way.node_refs[i];
            let to_node = way.node_refs[i + 1];

            let distance_meters = if let (Some(node1), Some(node2)) =
                (graph.nodes.get(&from_node), graph.nodes.get(&to_node))
            {
                crate::spatial::geometry::haversine_distance(
                    node1.lat, node1.lon, node2.lat, node2.lon,
                ) * 1000.0
            } else {
                0.0
            };

            adjacency_list
                .entry(from_node)
                .or_default()
                .push(RouteEdge {
                    to_node,
                    way_id,
                    distance: distance_meters,
                });

            adjacency_list_reverse
                .entry(to_node)
                .or_default()
                .push(RouteEdge {
                    to_node: from_node,
                    way_id,
                    distance: distance_meters,
                });
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
        nodes: FxHashMap::from_iter(graph.nodes.clone()),
        ways: FxHashMap::from_iter(graph.ways.clone()),
    }
}

fn filter_graph(graph: &mut Graph) {
    let used_node_ids: HashSet<i64> = graph
        .ways
        .values()
        .flat_map(|way| way.node_refs.iter().cloned())
        .collect();

    graph
        .nodes
        .retain(|node_id, _| used_node_ids.contains(node_id));

    graph.relations.retain(|_, relation| {
        if let Some(relation_type) = relation.tags.get("type") {
            if relation_type == "restriction" {
                true
            } else {
                false
            }
        } else {
            false
        }
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
