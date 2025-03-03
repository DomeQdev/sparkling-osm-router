use crate::errors::{GraphError, Result};
use crate::graph::{Graph, Node, Relation, RelationMember, Way};
use crate::utils::parse_attribute;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use xml::reader::{EventReader, XmlEvent};

pub fn parse_osm_xml(file_path: &str) -> Result<Graph> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);
    let parser = EventReader::new(reader);

    let mut graph = Graph::new();
    let mut current_tags: HashMap<String, String> = HashMap::new();
    let mut current_node_refs: Vec<i64> = Vec::new();
    let mut current_members: Vec<RelationMember> = Vec::new();
    let mut current_feature_id: i64 = 0;
    let mut _current_feature_type: Option<String> = None;

    for event in parser.into_iter() {
        match event {
            Ok(event) => match event {
                XmlEvent::StartElement {
                    name, attributes, ..
                } => match name.local_name.as_str() {
                    "node" => {
                        _current_feature_type = Some("node".to_string());
                        current_feature_id =
                            parse_attribute(&attributes, "id", "Node ID missing or invalid")?;
                        let lat: f64 = parse_attribute(
                            &attributes,
                            "lat",
                            "Node latitude missing or invalid",
                        )?;
                        let lon: f64 = parse_attribute(
                            &attributes,
                            "lon",
                            "Node longitude missing or invalid",
                        )?;

                        graph.nodes.insert(
                            current_feature_id,
                            Node {
                                id: current_feature_id,
                                lat,
                                lon,
                                tags: HashMap::new(),
                            },
                        );
                        current_tags.clear();
                    }
                    "way" => {
                        _current_feature_type = Some("way".to_string());
                        current_feature_id =
                            parse_attribute(&attributes, "id", "Way ID missing or invalid")?;
                        graph.ways.insert(
                            current_feature_id,
                            Way {
                                id: current_feature_id,
                                node_refs: Vec::new(),
                                tags: HashMap::new(),
                            },
                        );
                        current_tags.clear();
                        current_node_refs.clear();
                    }
                    "relation" => {
                        _current_feature_type = Some("relation".to_string());
                        current_feature_id =
                            parse_attribute(&attributes, "id", "Relation ID missing or invalid")?;
                        graph.relations.insert(
                            current_feature_id,
                            Relation {
                                id: current_feature_id,
                                members: Vec::new(),
                                tags: HashMap::new(),
                            },
                        );
                        current_tags.clear();
                        current_members.clear();
                    }
                    "tag" => {
                        let k = attributes
                            .iter()
                            .find(|attr| attr.name.local_name == "k")
                            .map(|attr| attr.value.clone())
                            .unwrap_or_default();
                        let v = attributes
                            .iter()
                            .find(|attr| attr.name.local_name == "v")
                            .map(|attr| attr.value.clone())
                            .unwrap_or_default();
                        current_tags.insert(k, v);
                    }
                    "nd" => {
                        let node_ref: i64 = parse_attribute(
                            &attributes,
                            "ref",
                            "Node ref missing or invalid in way",
                        )?;
                        current_node_refs.push(node_ref);
                    }
                    "member" => {
                        let member_type = attributes
                            .iter()
                            .find(|attr| attr.name.local_name == "type")
                            .map(|attr| attr.value.clone())
                            .unwrap_or_default();
                        let ref_id: i64 = parse_attribute(
                            &attributes,
                            "ref",
                            "Member ref missing or invalid in relation",
                        )?;
                        let role = attributes
                            .iter()
                            .find(|attr| attr.name.local_name == "role")
                            .map(|attr| attr.value.clone())
                            .unwrap_or_default();
                        current_members.push(RelationMember {
                            member_type,
                            ref_id,
                            role,
                        });
                    }
                    _ => {}
                },
                XmlEvent::EndElement { name } => match name.local_name.as_str() {
                    "node" => {
                        if let Some(node) = graph.nodes.get_mut(&current_feature_id) {
                            node.tags = current_tags.clone();
                        }
                        _current_feature_type = None;
                    }
                    "way" => {
                        if let Some(way) = graph.ways.get_mut(&current_feature_id) {
                            way.tags = current_tags.clone();
                            way.node_refs = current_node_refs.clone();
                        }
                        _current_feature_type = None;
                    }
                    "relation" => {
                        if let Some(relation) = graph.relations.get_mut(&current_feature_id) {
                            relation.tags = current_tags.clone();
                            relation.members = current_members.clone();
                        }
                        _current_feature_type = None;
                    }
                    _ => {}
                },
                XmlEvent::Characters(_content) => {}
                _ => {}
            },
            Err(e) => {
                return Err(GraphError::XmlParsing(e));
            }
        }
    }

    Ok(graph)
}
