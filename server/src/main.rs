/// HTTP port the server listens on.
const PORT: u16 = 8080;

/// Source of the .osm.pbf file. May be a URL (http/https) — in that case the
/// file is downloaded to `OSM_CACHE_PATH` on first boot. Or a local filesystem
/// path — used directly.
const OSM_SOURCE: &str = "https://download.geofabrik.de/europe/poland-latest.osm.pbf";

/// Where to store the downloaded .osm.pbf (when `OSM_SOURCE` is a URL).
const OSM_CACHE_PATH: &str = "/data/source.osm.pbf";

/// Where to store / read the built binary graph.
const GRAPH_PATH: &str = "/data/graph.bin";

/// Bounding box: [min_lon, min_lat, max_lon, max_lat].
/// All zeros = no clipping (use the entire OSM file).
const BBOX: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

/// Seconds of inactivity (no requests) after which the graph is evicted from RAM.
const GRAPH_TTL_SECS: u64 = 20 * 60;

/// How often the eviction task checks for idle timeout.
const EVICTION_CHECK_SECS: u64 = 30;

// ---------- Profile ----------
const PROFILE_NAME: &str = "bus";
const PROFILE_ACCESS: &[&str] = &["access", "vehicle", "motor_vehicle", "psv", "bus"];
const PROFILE_DISALLOW_MOTORROAD: bool = false;
const PROFILE_DISABLE_RESTRICTIONS: bool = false;

/// Penalties per tag: (key, value, penalty). Lower = preferred. 1.0 = neutral.
const PROFILE_PENALTIES: &[(&str, &str, f32)] = &[
    ("highway", "motorway", 1.0),
    ("highway", "motorway_link", 1.0),
    ("highway", "trunk", 1.0),
    ("highway", "trunk_link", 1.0),
    ("highway", "primary", 1.1),
    ("highway", "primary_link", 1.1),
    ("highway", "secondary", 1.15),
    ("highway", "secondary_link", 1.15),
    ("highway", "tertiary", 1.15),
    ("highway", "tertiary_link", 1.15),
    ("highway", "unclassified", 1.5),
    ("highway", "minor", 1.5),
    ("highway", "residential", 2.5),
    ("highway", "living_street", 2.5),
    ("highway", "track", 5.0),
    ("highway", "service", 5.0),
];

/// Per-node tag filters. (key, value, list of tags to keep on matching nodes)
const TAG_FILTERS: &[(&str, &str, &[&str])] = &[("public_transport", "stop_position", &["name"])];

/// Step limit for A* (safety cap against runaway routes).
const ROUTE_STEP_LIMIT: usize = 5_000_000;

// ---------- Threading ----------
/// Number of tokio worker threads handling async HTTP work.
/// 0 = auto (one per CPU core).
const WORKER_THREADS: usize = 0;

/// Maximum number of tokio blocking threads. Each CPU-bound request (routing,
/// nearest lookup, warmup file load) runs on a blocking thread, so this is the
/// effective concurrency cap for heavy work
const MAX_BLOCKING_THREADS: usize = 1024;

use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::path::Path as FsPath;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;

use routx::builder::build_memory_mapped_graph;
use routx::flat_graph::MemoryMappedGraph;
use routx::osm::{NodeTagFilter, Penalty, Profile};
use routx::{find_route_without_turn_around, Graph, KDTree};

struct LoadedGraph {
    graph: ManuallyDrop<MemoryMappedGraph<'static>>,
    buffer: *mut [u8],
}

unsafe impl Send for LoadedGraph {}
unsafe impl Sync for LoadedGraph {}

impl LoadedGraph {
    fn from_file(path: &str) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
        let boxed: Box<[u8]> = bytes.into_boxed_slice();
        let buffer: *mut [u8] = Box::into_raw(boxed);
        let slice: &'static [u8] = unsafe { &*buffer };
        match MemoryMappedGraph::new(slice) {
            Ok(graph) => Ok(LoadedGraph {
                graph: ManuallyDrop::new(graph),
                buffer,
            }),
            Err(e) => {
                unsafe { drop(Box::from_raw(buffer)) };
                Err(format!("graph init: {e}"))
            }
        }
    }

    fn graph(&self) -> &MemoryMappedGraph<'static> {
        &self.graph
    }
}

impl Drop for LoadedGraph {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.graph);
            drop(Box::from_raw(self.buffer));
        }
    }
}

struct AppState {
    graph: RwLock<Option<LoadedGraph>>,
    last_access: AtomicU64,
    warmup_lock: tokio::sync::Mutex<()>,
}

impl AppState {
    fn new() -> Self {
        Self {
            graph: RwLock::new(None),
            last_access: AtomicU64::new(now_secs()),
            warmup_lock: tokio::sync::Mutex::new(()),
        }
    }

    fn touch(&self) {
        self.last_access.store(now_secs(), Ordering::Relaxed);
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ensure_graph_file_exists() -> Result<(), String> {
    if FsPath::new(GRAPH_PATH).exists() {
        eprintln!("[boot] graph file present: {GRAPH_PATH}");
        return Ok(());
    }
    eprintln!("[boot] graph file missing, building...");

    if let Some(parent) = FsPath::new(GRAPH_PATH).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
    }

    let osm_path = if OSM_SOURCE.starts_with("http://") || OSM_SOURCE.starts_with("https://") {
        if !FsPath::new(OSM_CACHE_PATH).exists() {
            if let Some(parent) = FsPath::new(OSM_CACHE_PATH).parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
            }
            eprintln!("[boot] downloading {OSM_SOURCE} -> {OSM_CACHE_PATH}");
            download_to_file(OSM_SOURCE, OSM_CACHE_PATH)?;
        } else {
            eprintln!("[boot] OSM cache present: {OSM_CACHE_PATH}");
        }
        OSM_CACHE_PATH.to_string()
    } else {
        OSM_SOURCE.to_string()
    };

    eprintln!("[boot] building graph from {osm_path}");
    build_graph_to_disk(&osm_path)?;
    eprintln!("[boot] graph saved: {GRAPH_PATH}");
    Ok(())
}

fn download_to_file(url: &str, path: &str) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(60 * 10))
        .build();
    let resp = agent.get(url).call().map_err(|e| format!("HTTP: {e}"))?;
    let mut reader = resp.into_reader();
    let tmp = format!("{path}.partial");
    {
        let mut file = std::fs::File::create(&tmp).map_err(|e| format!("create {tmp}: {e}"))?;
        std::io::copy(&mut reader, &mut file).map_err(|e| format!("download: {e}"))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

fn build_graph_to_disk(osm_path: &str) -> Result<(), String> {
    let mut access: Vec<&str> = PROFILE_ACCESS.to_vec();
    if !access.contains(&"access") {
        access.push("access");
    }

    let penalties: Vec<Penalty> = PROFILE_PENALTIES
        .iter()
        .map(|(k, v, p)| Penalty {
            key: k,
            value: v,
            penalty: *p,
        })
        .collect();

    let profile = Profile {
        name: PROFILE_NAME,
        penalties: &penalties,
        access: &access,
        disallow_motorroad: PROFILE_DISALLOW_MOTORROAD,
        disable_restrictions: PROFILE_DISABLE_RESTRICTIONS,
    };

    let tag_filters: Vec<NodeTagFilter> = TAG_FILTERS
        .iter()
        .map(|(k, v, tags)| NodeTagFilter {
            key: k.to_string(),
            value: v.to_string(),
            tags_to_save: tags.iter().map(|s| s.to_string()).collect(),
        })
        .collect();

    let osm_options = routx::osm::Options {
        profile: &profile,
        file_format: routx::osm::FileFormat::Unknown,
        bbox: BBOX,
        node_tag_filters: &tag_filters,
    };

    let mut graph = Graph::default();
    routx::osm::add_features_from_file(&mut graph, &osm_options, osm_path)
        .map_err(|e| format!("osm parse: {e}"))?;

    let kd_tree = KDTree::build_from_graph(&graph);

    let tmp = format!("{GRAPH_PATH}.partial");
    {
        let out_file = std::fs::File::create(&tmp).map_err(|e| format!("create {tmp}: {e}"))?;
        let writer = std::io::BufWriter::new(out_file);
        build_memory_mapped_graph(&graph, kd_tree.as_ref(), writer)
            .map_err(|e| format!("build graph: {e:?}"))?;
    }
    std::fs::rename(&tmp, GRAPH_PATH).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

async fn eviction_loop(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(Duration::from_secs(EVICTION_CHECK_SECS));
    interval.tick().await;
    loop {
        interval.tick().await;

        let loaded = state.graph.read().is_some();
        if !loaded {
            continue;
        }

        let idle = now_secs().saturating_sub(state.last_access.load(Ordering::Relaxed));
        if idle < GRAPH_TTL_SECS {
            continue;
        }

        let mut guard = state.graph.write();
        let idle = now_secs().saturating_sub(state.last_access.load(Ordering::Relaxed));
        if guard.is_some() && idle >= GRAPH_TTL_SECS {
            eprintln!("[evict] {idle}s idle — dropping graph from RAM");
            *guard = None;
        }
    }
}

async fn health() -> &'static str {
    "ok"
}

async fn status(State(state): State<Arc<AppState>>) -> Response {
    let loaded = state.graph.read().is_some();
    let graph_file_exists = FsPath::new(GRAPH_PATH).exists();
    let last = state.last_access.load(Ordering::Relaxed);
    let idle = now_secs().saturating_sub(last);
    Json(json!({
        "loaded": loaded,
        "graph_file_exists": graph_file_exists,
        "graph_path": GRAPH_PATH,
        "seconds_since_last_access": idle,
        "ttl_seconds": GRAPH_TTL_SECS,
    }))
    .into_response()
}

async fn warmup(State(state): State<Arc<AppState>>) -> Response {
    if state.graph.read().is_some() {
        state.touch();
        return (StatusCode::OK, "already loaded").into_response();
    }

    let _serialize = state.warmup_lock.lock().await;

    if state.graph.read().is_some() {
        state.touch();
        return (StatusCode::OK, "already loaded").into_response();
    }

    if !FsPath::new(GRAPH_PATH).exists() {
        let build = tokio::task::spawn_blocking(ensure_graph_file_exists).await;
        match build {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
            Err(_) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, "build task panicked").into_response()
            }
        }
    }

    let state2 = state.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let loaded = LoadedGraph::from_file(GRAPH_PATH)?;
        let mut guard = state2.graph.write();
        if guard.is_none() {
            *guard = Some(loaded);
        }
        Ok(())
    })
    .await;

    match result {
        Ok(Ok(())) => {
            state.touch();
            (StatusCode::OK, "loaded").into_response()
        }
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "warmup task panicked").into_response(),
    }
}

async fn unload(State(state): State<Arc<AppState>>) -> Response {
    let mut guard = state.graph.write();
    let was = guard.take().is_some();
    if was {
        (StatusCode::OK, "unloaded").into_response()
    } else {
        (StatusCode::OK, "was not loaded").into_response()
    }
}

#[derive(Deserialize)]
struct PreferredTag {
    key: String,
    value: String,
    #[serde(default)]
    strict: bool,
    #[serde(default = "default_tag_reward")]
    reward: f32,
}

#[derive(Deserialize)]
struct NearestRequest {
    lat: f32,
    lon: f32,
    #[serde(default = "default_radius")]
    radius: f32,
    #[serde(default = "default_max_count")]
    max_count: u32,
    #[serde(default)]
    preferred_tags: Vec<PreferredTag>,
}
fn default_radius() -> f32 {
    10_000.0
}
fn default_max_count() -> u32 {
    1
}
fn default_tag_reward() -> f32 {
    1.0
}

#[derive(Serialize)]
struct NearestNode {
    id: u32,
    location: [f32; 2],
}

async fn nearest(State(state): State<Arc<AppState>>, Json(req): Json<NearestRequest>) -> Response {
    let state2 = state.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Vec<NearestNode>, StatusCode> {
        let guard = state2.graph.read();
        let loaded = guard.as_ref().ok_or(StatusCode::NOT_FOUND)?;
        let graph = loaded.graph();

        let candidates = graph.find_nodes_within_radius(req.lat, req.lon, req.radius);
        let has_tags = !req.preferred_tags.is_empty();

        let mut scored: Vec<(u32, f32)> = candidates
            .into_iter()
            .filter_map(|id| {
                let n = graph.get_node(id)?;
                let dist = fast_distance_sq(req.lat, req.lon, n.lat, n.lon).sqrt();
                let mut boost: f32 = 1.0;
                if has_tags {
                    let tags: HashMap<&str, &str> = graph.get_tags(n).collect();
                    for pt in &req.preferred_tags {
                        let Some(&val) = tags.get(pt.key.as_str()) else {
                            continue;
                        };
                        let factor = if pt.strict {
                            if val == pt.value {
                                pt.reward
                            } else {
                                1.0
                            }
                        } else {
                            levenshtein_ratio(val, &pt.value) * pt.reward
                        };
                        if factor > 0.0 {
                            boost *= factor;
                        }
                    }
                }
                let score = if boost > 0.0 { dist / boost } else { dist };
                Some((id, score))
            })
            .collect();

        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if req.max_count > 0 && scored.len() > req.max_count as usize {
            scored.truncate(req.max_count as usize);
        }

        let out = scored
            .into_iter()
            .filter_map(|(id, _)| {
                graph.get_node(id).map(|n| NearestNode {
                    id,
                    location: [n.lon, n.lat],
                })
            })
            .collect();
        Ok(out)
    })
    .await;

    match result {
        Ok(Ok(nodes)) => {
            state.touch();
            Json(nodes).into_response()
        }
        Ok(Err(StatusCode::NOT_FOUND)) => {
            (StatusCode::NOT_FOUND, "graph not loaded").into_response()
        }
        Ok(Err(s)) => (s, "").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "task panicked").into_response(),
    }
}

#[derive(Deserialize)]
struct RouteRequest {
    from: u32,
    to: u32,
    #[serde(default)]
    step_limit: Option<usize>,
    #[serde(default)]
    include_shape: Option<bool>,
}

#[derive(Serialize)]
struct RouteResponse {
    nodes: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shape: Option<Vec<[f32; 2]>>,
}

enum RouteErr {
    NotLoaded,
    InvalidNode,
    StepLimit,
}

async fn route(State(state): State<Arc<AppState>>, Json(req): Json<RouteRequest>) -> Response {
    let state2 = state.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<RouteResponse, RouteErr> {
        let guard = state2.graph.read();
        let loaded = guard.as_ref().ok_or(RouteErr::NotLoaded)?;
        let graph = loaded.graph();

        let limit = req.step_limit.unwrap_or(ROUTE_STEP_LIMIT);
        let nodes = match find_route_without_turn_around(graph, req.from, req.to, limit) {
            Ok(n) => n,
            Err(routx::AStarError::InvalidReference(_)) => return Err(RouteErr::InvalidNode),
            Err(routx::AStarError::StepLimitExceeded) => return Err(RouteErr::StepLimit),
        };

        let shape = if req.include_shape.unwrap_or(true) {
            Some(
                nodes
                    .iter()
                    .filter_map(|&id| graph.get_node(id).map(|n| [n.lon, n.lat]))
                    .collect(),
            )
        } else {
            None
        };

        Ok(RouteResponse { nodes, shape })
    })
    .await;

    match result {
        Ok(Ok(resp)) => {
            state.touch();
            Json(resp).into_response()
        }
        Ok(Err(RouteErr::NotLoaded)) => (StatusCode::NOT_FOUND, "graph not loaded").into_response(),
        Ok(Err(RouteErr::InvalidNode)) => {
            (StatusCode::BAD_REQUEST, "invalid node reference").into_response()
        }
        Ok(Err(RouteErr::StepLimit)) => {
            (StatusCode::REQUEST_TIMEOUT, "step limit exceeded").into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "task panicked").into_response(),
    }
}

async fn get_node(
    State(state): State<Arc<AppState>>,
    AxumPath(node_id): AxumPath<u32>,
) -> Response {
    let guard = state.graph.read();
    let Some(loaded) = guard.as_ref() else {
        return (StatusCode::NOT_FOUND, "graph not loaded").into_response();
    };
    let graph = loaded.graph();

    let Some(n) = graph.get_node(node_id) else {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    };

    let tags: HashMap<&str, &str> = graph.get_tags(n).collect();
    let body = json!({
        "id": node_id,
        "location": [n.lon, n.lat],
        "tags": tags,
    });
    drop(guard);
    state.touch();
    Json(body).into_response()
}

// -----------------------------------------------------------------------------

fn fast_distance_sq(lat1: f32, lon1: f32, lat2: f32, lon2: f32) -> f32 {
    let lat_avg = (lat1 + lat2) * 0.5;
    let cos_lat = lat_avg.to_radians().cos();
    let dlat = lat2 - lat1;
    let dlon = (lon2 - lon1) * cos_lat;
    dlat * dlat + dlon * dlon
}

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (curr[j - 1] + 1).min(prev[j] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

fn levenshtein_ratio(a: &str, b: &str) -> f32 {
    let max_len = a.chars().count().max(b.chars().count());
    if max_len == 0 {
        return 1.0;
    }
    let dist = levenshtein_distance(a, b);
    1.0 - (dist as f32 / max_len as f32)
}

fn main() {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder
        .enable_all()
        .max_blocking_threads(MAX_BLOCKING_THREADS);
    if WORKER_THREADS > 0 {
        builder.worker_threads(WORKER_THREADS);
    }
    let runtime = builder.build().expect("tokio runtime build failed");

    runtime.block_on(async_main());
}

async fn async_main() {
    let state = Arc::new(AppState::new());

    tokio::spawn(eviction_loop(state.clone()));

    let app = Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/warmup", post(warmup))
        .route("/unload", post(unload))
        .route("/nearest", post(nearest))
        .route("/route", post(route))
        .route("/node/:id", get(get_node))
        .with_state(state);

    let addr = format!("0.0.0.0:{PORT}");
    eprintln!("[boot] listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("bind failed");
    axum::serve(listener, app).await.expect("serve failed");
}
