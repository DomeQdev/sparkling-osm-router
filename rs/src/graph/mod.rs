use crate::core::errors::{GraphError, Result};
use rstar::{PointDistance, RTree, RTreeObject, AABB};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteNode {
    pub id: u32,
    pub external_id: i64,
    pub lat: f64,
    pub lon: f64,
    pub tags: HashMap<String, String>,
}

impl RTreeObject for RouteNode {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_point([self.lon, self.lat])
    }
}

impl PointDistance for RouteNode {
    fn distance_2(&self, point: &[f64; 2]) -> f64 {
        let dx = self.lon - point[0];
        let dy = self.lat - point[1];
        dx * dx + dy * dy
    }
}

#[derive(Serialize, Deserialize)]
pub struct ProcessedGraph {
    pub nodes: Vec<RouteNode>,
    pub offsets: Vec<usize>,
    pub edges: Vec<(u32, u32)>,

    #[serde(skip)]
    pub node_id_map: FxHashMap<i64, u32>,

    #[serde(skip)]
    pub spatial_index: RTree<RouteNode>,
}

impl ProcessedGraph {
    pub fn new() -> Self {
        ProcessedGraph {
            nodes: Vec::new(),
            offsets: Vec::new(),
            edges: Vec::new(),
            node_id_map: FxHashMap::default(),
            spatial_index: RTree::new(),
        }
    }

    pub fn neighbors(&self, node_id: u32) -> &[(u32, u32)] {
        let start = self.offsets[node_id as usize];
        let end = self.offsets[(node_id as usize) + 1];
        &self.edges[start..end]
    }

    pub fn build_indices(&mut self) {
        self.node_id_map = self.nodes.iter().map(|n| (n.external_id, n.id)).collect();

        let points: Vec<RouteNode> = self
            .nodes
            .iter()
            .filter(|n| n.external_id < MAX_NODE_ID)
            .cloned()
            .collect();
        self.spatial_index = RTree::bulk_load(points);
    }

    pub fn find_nearest_node(&self, lon: f64, lat: f64) -> Result<i64> {
        self.spatial_index
            .nearest_neighbor(&[lon, lat])
            .map(|node| node.external_id)
            .ok_or_else(|| GraphError::RoutingError("No nodes found in spatial index.".to_string()))
    }

    pub fn find_nodes_within_radius(
        &self,
        lon: f64,
        lat: f64,
        radius_meters: f64,
    ) -> Vec<&RouteNode> {
        let radius_degrees = radius_meters / 111_100.0;
        let radius_degrees_sq = radius_degrees * radius_degrees;

        self.spatial_index
            .locate_within_distance([lon, lat], radius_degrees_sq)
            .collect()
    }
}

pub const MAX_NODE_ID: i64 = 0x0008_0000_0000_0000;

#[derive(Serialize, Deserialize)]
pub struct GraphContainer {
    pub profiles: HashMap<String, ProcessedGraph>,
}

impl GraphContainer {
    pub fn new() -> Self {
        GraphContainer {
            profiles: HashMap::new(),
        }
    }

    pub fn build_all_indices(&mut self) {
        for graph in self.profiles.values_mut() {
            graph.build_indices();
        }
    }
}
