use crate::core::errors::{GraphError, Result};
use rstar::{PointDistance, RTree, RTreeObject, AABB};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteNode {
    pub id: i64,
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
    pub nodes: HashMap<i64, RouteNode>,
    pub edges: HashMap<i64, HashMap<i64, u32>>,
    #[serde(skip)]
    pub spatial_index: RTree<RouteNode>,
}

impl ProcessedGraph {
    pub fn new() -> Self {
        ProcessedGraph {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            spatial_index: RTree::new(),
        }
    }

    pub fn build_spatial_index(&mut self) {
        let points: Vec<RouteNode> = self
            .nodes
            .values()
            .filter(|n| n.id == n.external_id)
            .cloned()
            .collect();
        self.spatial_index = RTree::bulk_load(points);
    }

    pub fn find_nearest_node(&self, lon: f64, lat: f64) -> Result<i64> {
        self.spatial_index
            .nearest_neighbor(&[lon, lat])
            .map(|node| node.id)
            .ok_or_else(|| GraphError::RoutingError("No nodes found in spatial index.".to_string()))
    }
}

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

    pub fn build_all_spatial_indices(&mut self) {
        for graph in self.profiles.values_mut() {
            graph.build_spatial_index();
        }
    }
}
