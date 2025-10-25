use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone)]
pub struct Node {
    pub id: i64,
    pub lat: f64,
    pub lon: f64,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Way {
    pub id: i64,
    pub node_refs: Vec<i64>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct RelationMember {
    pub member_type: String,
    pub ref_id: i64,
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct Relation {
    pub id: i64,
    pub members: Vec<RelationMember>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct ProfilePenalties {
    #[serde(default)]
    pub default: Option<u32>,
    #[serde(flatten)]
    pub penalties: HashMap<String, f64>,
}

impl Eq for ProfilePenalties {}

impl Hash for ProfilePenalties {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.default.hash(state);
        let mut sorted_penalties: Vec<_> = self.penalties.iter().collect();
        sorted_penalties.sort_by_key(|(k, _)| *k);
        for (key, value) in sorted_penalties {
            key.hash(state);
            value.to_bits().hash(state);
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Profile {
    pub id: String,
    pub key: String,
    pub penalties: ProfilePenalties,
    #[serde(default)]
    pub access_tags: Vec<String>,
    #[serde(default)]
    pub oneway_tags: Vec<String>,
    #[serde(default)]
    pub except_tags: Vec<String>,
}

impl Hash for Profile {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.key.hash(state);
        self.penalties.hash(state);
        self.access_tags.hash(state);
        self.oneway_tags.hash(state);
        self.except_tags.hash(state);
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Hash, PartialEq, Eq)]
pub struct OverpassOptions {
    pub query: String,
    pub server: String,
    pub retries: u32,
    pub retry_delay: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Hash, PartialEq, Eq)]
pub struct ProtobufOptions {
    pub url: String,
    pub retries: u32,
    pub retry_delay: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LoadOptions {
    pub file_path: String,
    pub ttl_days: u64,
    pub profiles: Vec<Profile>,
    pub overpass: Option<OverpassOptions>,
    pub protobuf: Option<ProtobufOptions>,
}
