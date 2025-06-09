use crate::routing::RouteGraph;
use neon::prelude::*;
use rstar::{PointDistance, RTree, RTreeObject, AABB};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Node {
    pub id: i64,
    pub lat: f64,
    pub lon: f64,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Way {
    pub id: i64,
    pub node_refs: Vec<i64>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RelationMember {
    pub member_type: String,
    pub ref_id: i64,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Relation {
    pub id: i64,
    pub members: Vec<RelationMember>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ProfilePenalties {
    #[serde(default)]
    pub default: Option<f64>,
    #[serde(flatten)]
    pub penalties: HashMap<String, f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Profile {
    pub key: String,
    pub penalties: ProfilePenalties,
    #[serde(default)]
    pub access_tags: Option<Vec<String>>,
    #[serde(default)]
    pub oneway_tags: Option<Vec<String>>,
    #[serde(default)]
    pub except_tags: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Graph {
    pub nodes: HashMap<i64, Node>,
    pub ways: HashMap<i64, Way>,
    pub relations: HashMap<i64, Relation>,
    #[serde(skip)]
    pub way_rtree: RTree<WayEnvelope>,
    #[serde(skip)]
    pub profile: Option<Profile>,
    #[serde(skip)]
    pub route_graph: Option<Arc<RouteGraph>>,
}

impl Finalize for Graph {}

impl Graph {
    pub fn new() -> Self {
        Graph {
            nodes: Default::default(),
            ways: Default::default(),
            relations: Default::default(),
            way_rtree: rstar::RTree::new(),
            profile: None,
            route_graph: None,
        }
    }

    pub fn set_profile(&mut self, profile: Profile) {
        self.profile = Some(profile);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WayEnvelope {
    pub way_id: i64,
    pub envelope: AABB<[f64; 2]>,
}

impl PointDistance for WayEnvelope {
    fn distance_2(&self, point: &[f64; 2]) -> f64 {
        self.envelope.distance_2(point)
    }
}

impl RTreeObject for WayEnvelope {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        self.envelope.clone()
    }
}
