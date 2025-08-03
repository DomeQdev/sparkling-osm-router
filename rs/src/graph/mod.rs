use crate::core::errors::{GraphError, Result};
use rstar::{RTree, RTreeObject, AABB};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteNode {
    pub id: u32,
    pub external_id: i64,
    pub lat: f32,
    pub lon: f32,
    pub tags: FxHashMap<u32, u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WayInfo {
    pub osm_id: i64,
    pub node_ids: Vec<u32>,
    pub tags: FxHashMap<u32, u32>,
}

#[derive(Clone, Debug)]
pub struct SpatialWay {
    pub way_idx: usize,
    pub aabb: AABB<[f32; 2]>,
}

impl RTreeObject for SpatialWay {
    type Envelope = AABB<[f32; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.aabb
    }
}

#[derive(Serialize, Deserialize)]
pub struct ProcessedGraph {
    pub nodes: Vec<RouteNode>,
    pub ways: Vec<WayInfo>,
    pub offsets: Vec<usize>,
    pub edges: Vec<(u32, u16)>,
    pub string_interner: Vec<String>,

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
            string_interner: Vec::new(),
            node_id_map: FxHashMap::default(),
            spatial_index: RTree::new(),
        }
    }

    pub fn neighbors(&self, node_id: u32) -> &[(u32, u16)] {
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
                let mut min_lon = f32::MAX;
                let mut min_lat = f32::MAX;
                let mut max_lon = f32::MIN;
                let mut max_lat = f32::MIN;

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

    fn find_nearest_point_on_way(
        &self,
        way_info: &WayInfo,
        query_point: [f32; 2],
    ) -> Option<(i64, f32)> {
        if way_info.node_ids.is_empty() {
            return None;
        }

        if way_info.node_ids.len() == 1 {
            let node = &self.nodes[way_info.node_ids[0] as usize];
            let node_point = [node.lon, node.lat];
            let distance_sq = squared_distance(&query_point, &node_point);
            return Some((node.external_id, distance_sq));
        }

        let mut min_distance_sq = f32::MAX;
        let mut nearest_node_external_id = None;

        for window in way_info.node_ids.windows(2) {
            let node1 = &self.nodes[window[0] as usize];
            let node2 = &self.nodes[window[1] as usize];

            let p1 = [node1.lon, node1.lat];
            let p2 = [node2.lon, node2.lat];

            let dist_sq = point_to_segment_distance(&query_point, &p1, &p2);

            if dist_sq < min_distance_sq {
                min_distance_sq = dist_sq;

                let dist_to_node1_sq = squared_distance(&query_point, &p1);
                let dist_to_node2_sq = squared_distance(&query_point, &p2);

                nearest_node_external_id = Some(if dist_to_node1_sq <= dist_to_node2_sq {
                    node1.external_id
                } else {
                    node2.external_id
                });
            }
        }

        nearest_node_external_id.map(|id| (id, min_distance_sq))
    }

    pub fn find_nearest_node(&self, lon: f32, lat: f32) -> Result<i64> {
        let query_point = [lon, lat];
        let mut search_radius_deg = 0.001;
        for _ in 0..5 {
            let min_p = [lon - search_radius_deg, lat - search_radius_deg];
            let max_p = [lon + search_radius_deg, lat + search_radius_deg];
            let search_aabb = AABB::from_corners(min_p, max_p);

            let candidate_ways = self
                .spatial_index
                .locate_in_envelope_intersecting(&search_aabb);

            let closest_candidate = candidate_ways
                .filter_map(|spatial_way| {
                    let way_info = &self.ways[spatial_way.way_idx];
                    self.find_nearest_point_on_way(way_info, query_point)
                })
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));

            if let Some((node_external_id, _distance_sq)) = closest_candidate {
                return Ok(node_external_id);
            }

            search_radius_deg *= 2.0;
        }
        Err(GraphError::RoutingError(
            "No nodes found near coordinates".into(),
        ))
    }

    pub fn find_ways_within_radius(&self, lon: f32, lat: f32, radius_meters: f32) -> Vec<&WayInfo> {
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
        lon: f32,
        lat: f32,
        radius_meters: f32,
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
    pub profiles: FxHashMap<String, ProcessedGraph>,
}

impl GraphContainer {
    pub fn new() -> Self {
        GraphContainer {
            profiles: FxHashMap::default(),
        }
    }

    pub fn build_all_indices(&mut self) {
        for graph in self.profiles.values_mut() {
            graph.build_indices();
        }
    }
}

pub fn squared_distance(p1: &[f32; 2], p2: &[f32; 2]) -> f32 {
    (p1[0] - p2[0]).powi(2) + (p1[1] - p2[1]).powi(2)
}

pub fn point_to_segment_distance(p: &[f32; 2], a: &[f32; 2], b: &[f32; 2]) -> f32 {
    let ab_x = b[0] - a[0];
    let ab_y = b[1] - a[1];

    if ab_x.abs() < 1e-9 && ab_y.abs() < 1e-9 {
        return squared_distance(p, a);
    }

    let ap_x = p[0] - a[0];
    let ap_y = p[1] - a[1];

    let t = (ap_x * ab_x + ap_y * ab_y) / (ab_x * ab_x + ab_y * ab_y);

    let t_clamped = t.max(0.0).min(1.0);

    let closest_x = a[0] + t_clamped * ab_x;
    let closest_y = a[1] + t_clamped * ab_y;

    (p[0] - closest_x).powi(2) + (p[1] - closest_y).powi(2)
}
