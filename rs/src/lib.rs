use routx::builder::build_memory_mapped_graph;
use routx::flat_graph::{FlatNode, MemoryMappedGraph};
use routx::osm::{NodeTagFilter, Penalty, Profile};
use routx::{find_route_without_turn_around, Graph, KDTree};
use serde::Deserialize;
use std::ffi::{c_char, CStr};
use std::fs::File;
use std::io::BufWriter;

#[derive(Deserialize)]
struct JsonPenalty {
    key: String,
    value: String,
    penalty: f32,
}

#[derive(Deserialize)]
struct JsonProfile {
    name: String,
    penalties: Vec<JsonPenalty>,
    access: Vec<String>,
    disallow_motorroad: bool,
    disable_restrictions: bool,
}

#[derive(Deserialize)]
struct JsonNodeTagFilter {
    key: String,
    value: String,
    tags_to_save: Vec<String>,
}

#[derive(Deserialize)]
struct JsonBuildOptions {
    osm_path: String,
    out_path: String,
    profile: JsonProfile,
    bbox: [f32; 4],
    tag_filters: Vec<JsonNodeTagFilter>,
}

#[no_mangle]
pub extern "C" fn sparkling_build_graph(json_options_ptr: *const c_char) -> bool {
    if json_options_ptr.is_null() {
        return false;
    }

    let json_str = unsafe { CStr::from_ptr(json_options_ptr).to_string_lossy() };
    let opts: JsonBuildOptions = match serde_json::from_str(&json_str) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("JSON parse error: {}", e);
            return false;
        }
    };

    let access_refs: Vec<&str> = opts.profile.access.iter().map(|s| s.as_str()).collect();
    let penalties_refs: Vec<Penalty> = opts
        .profile
        .penalties
        .iter()
        .map(|p| Penalty {
            key: &p.key,
            value: &p.value,
            penalty: p.penalty,
        })
        .collect();

    let profile = Profile {
        name: &opts.profile.name,
        penalties: &penalties_refs,
        access: &access_refs,
        disallow_motorroad: opts.profile.disallow_motorroad,
        disable_restrictions: opts.profile.disable_restrictions,
    };

    let tag_filters: Vec<NodeTagFilter> = opts
        .tag_filters
        .into_iter()
        .map(|f| NodeTagFilter {
            key: f.key,
            value: f.value,
            tags_to_save: f.tags_to_save,
        })
        .collect();

    let osm_options = routx::osm::Options {
        profile: &profile,
        file_format: routx::osm::FileFormat::Unknown,
        bbox: opts.bbox,
        node_tag_filters: &tag_filters,
    };

    let mut graph = Graph::default();
    if let Err(e) = routx::osm::add_features_from_file(&mut graph, &osm_options, &opts.osm_path) {
        eprintln!("Error adding features: {}", e);
        return false;
    }

    let kd_tree = KDTree::build_from_graph(&graph);

    let out_file = match File::create(&opts.out_path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let writer = BufWriter::new(out_file);
    build_memory_mapped_graph(&graph, kd_tree.as_ref(), writer).is_ok()
}

#[no_mangle]
pub extern "C" fn sparkling_mmap_init(
    data_ptr: *const u8,
    len: u64,
) -> *mut MemoryMappedGraph<'static> {
    let slice = unsafe { std::slice::from_raw_parts(data_ptr, len as usize) };
    match MemoryMappedGraph::new(slice) {
        Ok(g) => Box::into_raw(Box::new(g)),
        Err(e) => {
            eprintln!("MMAP init error: {}", e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn sparkling_mmap_destroy(ptr: *mut MemoryMappedGraph<'static>) {
    if !ptr.is_null() {
        unsafe { drop(Box::from_raw(ptr)) };
    }
}

#[no_mangle]
pub extern "C" fn sparkling_find_nearest_nodes(
    graph_ptr: *const MemoryMappedGraph<'static>,
    lat: f32,
    lon: f32,
    radius: f32,
    max_count: u32,
    out_len: *mut u32,
    out_capacity: *mut u32,
) -> *mut u32 {
    unsafe {
        *out_len = 0;
        *out_capacity = 0;
    }

    let graph = match unsafe { graph_ptr.as_ref() } {
        Some(g) => g,
        None => return std::ptr::null_mut(),
    };

    let mut candidates = graph.find_nodes_within_radius(lat, lon, radius);

    candidates.sort_by(|&a, &b| {
        let na = graph.get_node(a).unwrap();
        let nb = graph.get_node(b).unwrap();

        let da = fast_distance_sq(lat, lon, na.lat, na.lon);
        let db = fast_distance_sq(lat, lon, nb.lat, nb.lon);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Truncate to max_count if specified
    if max_count > 0 && candidates.len() > max_count as usize {
        candidates.truncate(max_count as usize);
    }

    candidates.shrink_to_fit();
    let ptr = candidates.as_mut_ptr();
    unsafe {
        *out_len = candidates.len() as u32;
        *out_capacity = candidates.capacity() as u32;
    }
    std::mem::forget(candidates);

    ptr
}

#[no_mangle]
pub extern "C" fn sparkling_find_route(
    graph_ptr: *const MemoryMappedGraph<'static>,
    from_idx: u32,
    to_idx: u32,
    step_limit: u32,
    out_len: *mut u32,
    out_capacity: *mut u32,
    out_error: *mut u8,
) -> *mut u32 {
    unsafe {
        *out_len = 0;
        *out_capacity = 0;
        *out_error = 0;
    }

    let graph = match unsafe { graph_ptr.as_ref() } {
        Some(g) => g,
        None => {
            unsafe { *out_error = 2 };
            return std::ptr::null_mut();
        }
    };

    match find_route_without_turn_around(graph, from_idx, to_idx, step_limit as usize) {
        Ok(mut nodes) => {
            if nodes.is_empty() {
                unsafe { *out_error = 1 };
                return std::ptr::null_mut();
            }

            nodes.shrink_to_fit();
            let ptr = nodes.as_mut_ptr();
            unsafe {
                *out_len = nodes.len() as u32;
                *out_capacity = nodes.capacity() as u32;
            }
            std::mem::forget(nodes);

            ptr
        }
        Err(routx::AStarError::InvalidReference(_)) => {
            unsafe { *out_error = 2 };
            std::ptr::null_mut()
        }
        Err(routx::AStarError::StepLimitExceeded) => {
            unsafe { *out_error = 3 };
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn sparkling_free_u32_array(ptr: *mut u32, len: u32, capacity: u32) {
    if !ptr.is_null() {
        unsafe { drop(Vec::from_raw_parts(ptr, len as usize, capacity as usize)) };
    }
}

#[no_mangle]
pub extern "C" fn sparkling_get_nodes_base_ptr(
    graph_ptr: *const MemoryMappedGraph<'static>,
) -> *const FlatNode {
    let graph = match unsafe { graph_ptr.as_ref() } {
        Some(g) => g,
        None => return std::ptr::null(),
    };

    graph
        .get_node(0)
        .map(|n| n as *const FlatNode)
        .unwrap_or(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn sparkling_get_nodes_count(graph_ptr: *const MemoryMappedGraph<'static>) -> u32 {
    let graph = match unsafe { graph_ptr.as_ref() } {
        Some(g) => g,
        None => return 0,
    };
    graph.nodes_count()
}

#[no_mangle]
pub extern "C" fn sparkling_get_node_tags_json(
    graph_ptr: *const MemoryMappedGraph<'static>,
    node_idx: u32,
    out_len: *mut u32,
) -> *mut u8 {
    unsafe {
        if !out_len.is_null() {
            *out_len = 0;
        }
    }

    let graph = match unsafe { graph_ptr.as_ref() } {
        Some(g) => g,
        None => return std::ptr::null_mut(),
    };

    let node = match graph.get_node(node_idx) {
        Some(n) => n,
        None => return std::ptr::null_mut(),
    };

    let tags: std::collections::HashMap<&str, &str> = graph.get_tags(node).collect();
    if tags.is_empty() {
        return std::ptr::null_mut();
    }

    match serde_json::to_string(&tags) {
        Ok(mut s) => {
            s.shrink_to_fit();
            let ptr = s.as_mut_ptr();
            unsafe {
                if !out_len.is_null() {
                    *out_len = s.len() as u32;
                }
            }
            std::mem::forget(s);
            ptr
        }
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn sparkling_free_string(ptr: *mut u8, len: u32) {
    if !ptr.is_null() {
        unsafe { drop(String::from_raw_parts(ptr, len as usize, len as usize)) };
    }
}

fn fast_distance_sq(lat1: f32, lon1: f32, lat2: f32, lon2: f32) -> f32 {
    let lat_avg = (lat1 + lat2) * 0.5;
    let cos_lat = lat_avg.to_radians().cos();

    let dlat = lat2 - lat1;
    let dlon = (lon2 - lon1) * cos_lat;

    dlat * dlat + dlon * dlon
}
