use crate::core::errors::{GraphError, Result};
use rstar::{RTree, RTreeObject, AABB};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteNode {
    pub id: u32,
    pub external_id: i64,
    pub lat: f64,
    pub lon: f64,
    pub tags: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WayInfo {
    pub osm_id: i64,
    pub node_ids: Vec<u32>,
    pub tags: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct SpatialWay {
    pub way_idx: usize,
    pub aabb: AABB<[f64; 2]>,
}

impl RTreeObject for SpatialWay {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.aabb
    }
}

#[derive(Serialize, Deserialize)]
pub struct ProcessedGraph {
    pub nodes: Vec<RouteNode>,
    pub ways: Vec<WayInfo>,
    pub offsets: Vec<usize>,
    pub edges: Vec<(u32, u32)>,

    #[serde(skip)]
    pub node_id_map: FxHashMap<i64, u32>,

    #[serde(skip)]
    pub spatial_index: RTree<SpatialWay>,
}

impl ProcessedGraph {
    pub fn new() -> Self {
        ProcessedGraph {
            nodes: Vec::new(),
            ways: Vec::new(),
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

        let spatial_ways: Vec<SpatialWay> = self
            .ways
            .iter()
            .enumerate()
            .map(|(way_idx, way_info)| {
                let mut min_lon = f64::MAX;
                let mut min_lat = f64::MAX;
                let mut max_lon = f64::MIN;
                let mut max_lat = f64::MIN;

                for &node_id in &way_info.node_ids {
                    let node = &self.nodes[node_id as usize];
                    min_lon = min_lon.min(node.lon);
                    min_lat = min_lat.min(node.lat);
                    max_lon = max_lon.max(node.lon);
                    max_lat = max_lat.max(node.lat);
                }

                let aabb = AABB::from_corners([min_lon, min_lat], [max_lon, max_lat]);
                SpatialWay { way_idx, aabb }
            })
            .collect();

        self.spatial_index = RTree::bulk_load(spatial_ways);
    }

    pub fn find_nearest_node(&self, lon: f64, lat: f64) -> Result<i64> {
        let mut search_radius_deg = 0.001; // Approx 111 meters
        for _ in 0..5 {
            // 5 attempts with increasing radius
            let min_p = [lon - search_radius_deg, lat - search_radius_deg];
            let max_p = [lon + search_radius_deg, lat + search_radius_deg];
            let search_aabb = AABB::from_corners(min_p, max_p);

            let candidate_ways = self
                .spatial_index
                .locate_in_envelope_intersecting(&search_aabb);

            if let Some(closest_node) = candidate_ways
                .flat_map(|spatial_way| &self.ways[spatial_way.way_idx].node_ids)
                .map(|&node_id| &self.nodes[node_id as usize])
                .min_by(|a, b| {
                    let dist_a = (a.lon - lon).powi(2) + (a.lat - lat).powi(2);
                    let dist_b = (b.lon - lon).powi(2) + (b.lat - lat).powi(2);
                    dist_a.partial_cmp(&dist_b).unwrap_or(Ordering::Equal)
                })
            {
                return Ok(closest_node.external_id);
            }

            search_radius_deg *= 2.0;
        }
        Err(GraphError::RoutingError(
            "No nodes found near coordinates".into(),
        ))
    }

    pub fn find_ways_within_radius(&self, lon: f64, lat: f64, radius_meters: f64) -> Vec<&WayInfo> {
        let radius_degrees = radius_meters / 111_100.0;
        let min_p = [lon - radius_degrees, lat - radius_degrees];
        let max_p = [lon + radius_degrees, lat + radius_degrees];
        let search_aabb = AABB::from_corners(min_p, max_p);
        let radius_degrees_sq = radius_degrees * radius_degrees;

        self.spatial_index
            .locate_in_envelope_intersecting(&search_aabb)
            .map(|spatial_way| &self.ways[spatial_way.way_idx])
            .filter(|way_info| {
                way_info.node_ids.iter().any(|&node_id| {
                    let node = &self.nodes[node_id as usize];
                    (node.lon - lon).powi(2) + (node.lat - lat).powi(2) <= radius_degrees_sq
                })
            })
            .collect()
    }

    pub fn find_nodes_within_radius(
        &self,
        lon: f64,
        lat: f64,
        radius_meters: f64,
    ) -> Vec<&RouteNode> {
        let ways = self.find_ways_within_radius(lon, lat, radius_meters);
        let mut node_ids = rustc_hash::FxHashSet::default();
        for way in ways {
            node_ids.extend(way.node_ids.iter().copied());
        }

        let radius_degrees = radius_meters / 111_100.0;
        let radius_degrees_sq = radius_degrees * radius_degrees;

        node_ids
            .iter()
            .map(|&id| &self.nodes[id as usize])
            .filter(|node| {
                let dist_sq = (node.lon - lon).powi(2) + (node.lat - lat).powi(2);
                dist_sq <= radius_degrees_sq
            })
            .collect()
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

    pub fn build_all_indices(&mut self) {
        for graph in self.profiles.values_mut() {
            graph.build_indices();
        }
    }
}
