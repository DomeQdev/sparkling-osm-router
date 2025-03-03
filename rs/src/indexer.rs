use crate::errors::Result;
use crate::graph::Node;
use crate::graph::{Graph, WayEnvelope};
use crate::routing::TurnRestriction;
use rstar::{RTree, AABB};
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

    update_graph_nodes(&graph);
    index_restricted_nodes(&graph);

    Ok(graph)
}

fn filter_graph(graph: &mut Graph) {
    let profile = graph.profile.clone().expect("Profile must be set");

    graph
        .ways
        .retain(|_, way| way.tags.contains_key(&profile.key));

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
    _restriction_type: TurnRestriction,
) -> bool {
    let mut from_way_id: Option<i64> = None;
    let mut via_node_id: Option<i64> = None;
    let mut to_way_id: Option<i64> = None;

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
