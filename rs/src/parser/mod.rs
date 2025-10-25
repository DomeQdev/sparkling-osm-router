use crate::core::errors::{GraphError, Result};
use crate::core::types::{Node, ProtobufOptions, Relation, RelationMember, Way};
use bytes::Bytes; // <-- NOWY IMPORT
use osmpbf::{Element, ElementReader};
use std::collections::HashMap;
use std::io::Cursor; // <-- NOWY IMPORT
use xml::attribute::OwnedAttribute;
use xml::reader::{EventReader, XmlEvent};

pub fn fetch_from_overpass(
    query: &str,
    server: &str,
    retries: u32,
    retry_delay: u64,
) -> Result<String> {
    let client = reqwest::blocking::Client::new();
    let mut attempts = 0;

    while attempts < retries {
        let response = client
            .post(format!("{}/api/interpreter", server))
            .body(query.to_string())
            .send()
            .map_err(|e| GraphError::OverpassError(e.to_string()))?;

        if response.status().is_success() {
            return response
                .text()
                .map_err(|e| GraphError::OverpassError(e.to_string()));
        }

        attempts += 1;
        std::thread::sleep(std::time::Duration::from_millis(retry_delay));
    }

    Err(GraphError::OverpassError(format!(
        "Failed after {} retries",
        retries
    )))
}

pub fn fetch_pbf_to_memory(options: &ProtobufOptions) -> Result<Bytes> {
    let client = reqwest::blocking::Client::new();
    let mut attempts = 0;

    while attempts < options.retries {
        let response = client
            .get(&options.url)
            .send()
            .map_err(|e| GraphError::DownloadError(e.to_string()))?;

        if response.status().is_success() {
            return response
                .bytes()
                .map_err(|e| GraphError::DownloadError(e.to_string()));
        }

        attempts += 1;
        std::thread::sleep(std::time::Duration::from_millis(options.retry_delay));
    }

    Err(GraphError::DownloadError(format!(
        "Failed to download PBF from {} after {} retries",
        options.url, options.retries
    )))
}

pub fn parse_osm_pbf(
    pbf_data: &[u8],
) -> Result<(
    HashMap<i64, Node>,
    HashMap<i64, Way>,
    HashMap<i64, Relation>,
)> {
    let cursor = Cursor::new(pbf_data);
    let reader = ElementReader::new(cursor);

    let (nodes, ways, relations) = reader.par_map_reduce(
        |element| {
            let mut thread_nodes = HashMap::new();
            let mut thread_ways = HashMap::new();
            let mut thread_relations = HashMap::new();

            match element {
                Element::Node(n) => {
                    thread_nodes.insert(
                        n.id(),
                        Node {
                            id: n.id(),
                            lat: n.lat(),
                            lon: n.lon(),
                            tags: n
                                .tags()
                                .map(|(k, v)| (k.to_string(), v.to_string()))
                                .collect(),
                        },
                    );
                }
                Element::DenseNode(n) => {
                    thread_nodes.insert(
                        n.id(),
                        Node {
                            id: n.id(),
                            lat: n.lat(),
                            lon: n.lon(),
                            tags: n
                                .tags()
                                .map(|(k, v)| (k.to_string(), v.to_string()))
                                .collect(),
                        },
                    );
                }
                Element::Way(w) => {
                    thread_ways.insert(
                        w.id(),
                        Way {
                            id: w.id(),
                            node_refs: w.refs().collect(),
                            tags: w
                                .tags()
                                .map(|(k, v)| (k.to_string(), v.to_string()))
                                .collect(),
                        },
                    );
                }
                Element::Relation(r) => {
                    thread_relations.insert(
                        r.id(),
                        Relation {
                            id: r.id(),
                            members: r
                                .members()
                                .map(|m| RelationMember {
                                    member_type: match m.member_type {
                                        osmpbf::RelMemberType::Node => "node".to_string(),
                                        osmpbf::RelMemberType::Way => "way".to_string(),
                                        osmpbf::RelMemberType::Relation => {
                                            "relation".to_string()
                                        }
                                    },
                                    ref_id: m.member_id,
                                    role: m.role().unwrap_or("").to_string(),
                                })
                                .collect(),
                            tags: r
                                .tags()
                                .map(|(k, v)| (k.to_string(), v.to_string()))
                                .collect(),
                        },
                    );
                }
            }
            (thread_nodes, thread_ways, thread_relations)
        },
        || (HashMap::new(), HashMap::new(), HashMap::new()),
        |mut a, b| {
            a.0.extend(b.0);
            a.1.extend(b.1);
            a.2.extend(b.2);
            a
        },
    )?;

    Ok((nodes, ways, relations))
}

pub fn parse_osm_xml(
    xml_data: &str,
) -> Result<(
    HashMap<i64, Node>,
    HashMap<i64, Way>,
    HashMap<i64, Relation>,
)> {
    let parser = EventReader::new(xml_data.as_bytes());
    let mut nodes = HashMap::new();
    let mut ways = HashMap::new();
    let mut relations = HashMap::new();

    let mut current_node: Option<Node> = None;
    let mut current_way: Option<Way> = None;
    let mut current_relation: Option<Relation> = None;

    for event in parser.into_iter() {
        match event {
            Ok(XmlEvent::StartElement {
                name, attributes, ..
            }) => match name.local_name.as_str() {
                "node" => {
                    let id = parse_attribute::<i64>(&attributes, "id", "Node ID")?;
                    let lat = parse_attribute::<f64>(&attributes, "lat", "Node lat")?;
                    let lon = parse_attribute::<f64>(&attributes, "lon", "Node lon")?;
                    current_node = Some(Node {
                        id,
                        lat,
                        lon,
                        tags: HashMap::new(),
                    });
                }
                "way" => {
                    let id = parse_attribute::<i64>(&attributes, "id", "Way ID")?;
                    current_way = Some(Way {
                        id,
                        node_refs: Vec::new(),
                        tags: HashMap::new(),
                    });
                }
                "relation" => {
                    let id = parse_attribute::<i64>(&attributes, "id", "Relation ID")?;
                    current_relation = Some(Relation {
                        id,
                        members: Vec::new(),
                        tags: HashMap::new(),
                    });
                }
                "tag" => {
                    let k = get_attribute(&attributes, "k").unwrap_or_default();
                    let v = get_attribute(&attributes, "v").unwrap_or_default();
                    if let Some(node) = &mut current_node {
                        node.tags.insert(k, v);
                    } else if let Some(way) = &mut current_way {
                        way.tags.insert(k, v);
                    } else if let Some(rel) = &mut current_relation {
                        rel.tags.insert(k, v);
                    }
                }
                "nd" => {
                    if let Some(way) = &mut current_way {
                        let node_ref = parse_attribute::<i64>(&attributes, "ref", "Way nd ref")?;
                        way.node_refs.push(node_ref);
                    }
                }
                "member" => {
                    if let Some(rel) = &mut current_relation {
                        let member_type = get_attribute(&attributes, "type").unwrap_or_default();
                        let ref_id =
                            parse_attribute::<i64>(&attributes, "ref", "Relation member ref")?;
                        let role = get_attribute(&attributes, "role").unwrap_or_default();
                        rel.members.push(RelationMember {
                            member_type,
                            ref_id,
                            role,
                        });
                    }
                }
                _ => {}
            },
            Ok(XmlEvent::EndElement { name }) => match name.local_name.as_str() {
                "node" => {
                    if let Some(node) = current_node.take() {
                        nodes.insert(node.id, node);
                    }
                }
                "way" => {
                    if let Some(way) = current_way.take() {
                        ways.insert(way.id, way);
                    }
                }
                "relation" => {
                    if let Some(rel) = current_relation.take() {
                        relations.insert(rel.id, rel);
                    }
                }
                _ => {}
            },
            Err(e) => return Err(GraphError::XmlParsing(e)),
            _ => {}
        }
    }

    Ok((nodes, ways, relations))
}

fn parse_attribute<T: std::str::FromStr>(
    attributes: &[OwnedAttribute],
    name: &str,
    feature: &str,
) -> Result<T> {
    get_attribute(attributes, name)
        .and_then(|v| v.parse::<T>().ok())
        .ok_or_else(|| {
            GraphError::InvalidOsmData(format!(
                "Attribute '{}' missing or invalid for {}",
                name, feature
            ))
        })
}

fn get_attribute(attributes: &[OwnedAttribute], name: &str) -> Option<String> {
    attributes
        .iter()
        .find(|attr| attr.name.local_name == name)
        .map(|attr| attr.value.clone())
}
