mod core;
mod graph;
mod parser;
mod processing;
mod queue;
mod routing;

use crate::core::errors::{GraphError, Result};
use crate::core::types::LoadOptions;
use crate::graph::GraphContainer;
use crate::parser::{fetch_from_overpass, parse_osm_xml};
use crate::processing::GraphBuilder;
use crate::queue::{RouteQueue, RouteRequest};
use lazy_static::lazy_static;
use neon::prelude::*;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};
use tokio::runtime::Runtime;

lazy_static! {
    static ref TOKIO_RUNTIME: Runtime = Runtime::new().expect("Failed to create Tokio runtime");
    static ref ROUTING_THREAD_POOL: rayon::ThreadPool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_cpus::get())
        .build()
        .expect("Failed to create routing thread pool");
    static ref GRAPH_STORAGE: Mutex<HashMap<i32, Arc<RwLock<GraphContainer>>>> =
        Mutex::new(HashMap::new());
    static ref ROUTE_QUEUES: Mutex<HashMap<i32, Arc<RouteQueue>>> = Mutex::new(HashMap::new());
}

static mut NEXT_GRAPH_ID: i32 = 1;
static mut NEXT_QUEUE_ID: i32 = 1;

fn load_or_build_graph_sync(options: LoadOptions) -> Result<GraphContainer> {
    let path = Path::new(&options.file_path);
    let ttl = Duration::from_secs(options.ttl_days * 24 * 60 * 60);

    if path.exists() {
        if let Ok(metadata) = fs::metadata(path) {
            if let Ok(modified) = metadata.modified() {
                if SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or_default()
                    < ttl
                {
                    let reader = BufReader::new(File::open(path)?);
                    if let Ok(mut container) =
                        bincode::deserialize_from::<_, GraphContainer>(reader)
                    {
                        container.build_all_indices();
                        return Ok(container);
                    }
                }
            }
        }
    }

    let xml_data = if let Some(overpass_opts) = options.overpass {
        fetch_from_overpass(
            &overpass_opts.query,
            &overpass_opts.server,
            overpass_opts.retries,
            overpass_opts.retry_delay,
        )?
    } else {
        fs::read_to_string(path).map_err(GraphError::FileIO)?
    };

    let (raw_nodes, raw_ways, raw_relations) = parse_osm_xml(&xml_data)?;
    let processed_profiles: Vec<_> = options
        .profiles
        .par_iter()
        .map(|profile| {
            let builder = GraphBuilder::new(profile, &raw_nodes, &raw_ways, &raw_relations);
            builder.build().map(|graph| (profile.id.clone(), graph))
        })
        .collect::<Result<_>>()?;

    let mut container = GraphContainer::new();
    container.profiles = processed_profiles.into_iter().collect();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let writer = BufWriter::new(File::create(path)?);
    bincode::serialize_into(writer, &container)?;

    Ok(container)
}

fn load_graph(mut cx: FunctionContext) -> JsResult<JsNumber> {
    let options_json = cx.argument::<JsString>(0)?.value(&mut cx);
    let options: LoadOptions = serde_json::from_str(&options_json)
        .or_else(|e| cx.throw_error(format!("Invalid options JSON: {}", e)))?;

    let graph_id = unsafe {
        let id = NEXT_GRAPH_ID;
        NEXT_GRAPH_ID += 1;
        id
    };

    match TOKIO_RUNTIME.block_on(async {
        tokio::task::spawn_blocking(move || load_or_build_graph_sync(options))
            .await
            .unwrap()
    }) {
        Ok(container) => {
            GRAPH_STORAGE
                .lock()
                .unwrap()
                .insert(graph_id, Arc::new(RwLock::new(container)));
            Ok(cx.number(graph_id as f64))
        }
        Err(e) => cx.throw_error(format!("Failed to load/build graph: {}", e)),
    }
}
fn get_route(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let graph_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let profile_id = cx.argument::<JsString>(1)?.value(&mut cx);
    let start_node = cx.argument::<JsNumber>(2)?.value(&mut cx) as i64;
    let end_node = cx.argument::<JsNumber>(3)?.value(&mut cx) as i64;

    let graph = match GRAPH_STORAGE.lock().unwrap().get(&graph_id) {
        Some(g) => g.clone(),
        None => return cx.throw_error(GraphError::GraphNotFound(graph_id).to_string()),
    };

    let (deferred, promise) = cx.promise();
    let channel = cx.channel();

    ROUTING_THREAD_POOL.spawn(move || {
        let result = graph
            .read()
            .unwrap()
            .route(&profile_id, start_node, end_node);
        deferred.settle_with(&channel, move |mut cx| match result {
            Ok(Some(nodes)) => {
                let js_result = cx.empty_object();
                let js_nodes = JsArray::new(&mut cx, nodes.len());
                for (i, node_id) in nodes.iter().enumerate() {
                    let js_node_id = cx.number(*node_id as f64);
                    js_nodes.set(&mut cx, i as u32, js_node_id)?;
                }
                js_result.set(&mut cx, "nodes", js_nodes)?;
                Ok(js_result)
            }

            Ok(None) => {
                let js_result = cx.empty_object();
                let js_nodes = JsArray::new(&mut cx, 0);
                js_result.set(&mut cx, "nodes", js_nodes)?;
                Ok(js_result)
            }
            Err(e) => cx.throw_error(e.to_string()),
        });
    });

    Ok(promise)
}

fn get_nearest_node(mut cx: FunctionContext) -> JsResult<JsValue> {
    let graph_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let profile_id = cx.argument::<JsString>(1)?.value(&mut cx);
    let lon = cx.argument::<JsNumber>(2)?.value(&mut cx);
    let lat = cx.argument::<JsNumber>(3)?.value(&mut cx);

    let graph = match GRAPH_STORAGE.lock().unwrap().get(&graph_id) {
        Some(g) => g.clone(),
        None => return cx.throw_error(GraphError::GraphNotFound(graph_id).to_string()),
    };

    let graph_guard = graph.read().unwrap();
    let profile_graph = match graph_guard.profiles.get(&profile_id) {
        Some(pg) => pg,
        None => return cx.throw_error(GraphError::ProfileNotFound(profile_id).to_string()),
    };

    match profile_graph.find_nearest_node(lon, lat) {
        Ok(node_id) => Ok(cx.number(node_id as f64).upcast()),
        Err(_) => Ok(cx.null().upcast()),
    }
}

fn get_nodes_in_radius(mut cx: FunctionContext) -> JsResult<JsArray> {
    let graph_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let profile_id = cx.argument::<JsString>(1)?.value(&mut cx);
    let lon = cx.argument::<JsNumber>(2)?.value(&mut cx);
    let lat = cx.argument::<JsNumber>(3)?.value(&mut cx);
    let radius_meters = cx.argument::<JsNumber>(4)?.value(&mut cx);

    let graph = match GRAPH_STORAGE.lock().unwrap().get(&graph_id) {
        Some(g) => g.clone(),
        None => return cx.throw_error(GraphError::GraphNotFound(graph_id).to_string()),
    };

    let graph_guard = graph.read().unwrap();
    let profile_graph = match graph_guard.profiles.get(&profile_id) {
        Some(pg) => pg,
        None => return cx.throw_error(GraphError::ProfileNotFound(profile_id).to_string()),
    };

    let found_nodes = profile_graph.find_nodes_within_radius(lon, lat, radius_meters);

    let js_array = JsArray::new(&mut cx, found_nodes.len() as usize);
    for (i, node) in found_nodes.iter().enumerate() {
        let js_object = cx.empty_object();

        let id_val = cx.number(node.external_id as f64);
        js_object.set(&mut cx, "id", id_val)?;

        let location_array = JsArray::new(&mut cx, 2);
        let lon = cx.number(node.lon);
        let lat = cx.number(node.lat);
        location_array.set(&mut cx, 0, lon)?;
        location_array.set(&mut cx, 1, lat)?;
        js_object.set(&mut cx, "location", location_array)?;

        let tags_obj = cx.empty_object();
        for (key, value) in &node.tags {
            let value_js = cx.string(value);
            tags_obj.set(&mut cx, key.as_str(), value_js)?;
        }
        js_object.set(&mut cx, "tags", tags_obj)?;

        js_array.set(&mut cx, i as u32, js_object)?;
    }

    Ok(js_array)
}

fn get_node(mut cx: FunctionContext) -> JsResult<JsValue> {
    let graph_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let profile_id = cx.argument::<JsString>(1)?.value(&mut cx);
    let node_id = cx.argument::<JsNumber>(2)?.value(&mut cx) as i64;

    let graph = match GRAPH_STORAGE.lock().unwrap().get(&graph_id) {
        Some(g) => g.clone(),
        None => return cx.throw_error(GraphError::GraphNotFound(graph_id).to_string()),
    };

    let graph_guard = graph.read().unwrap();
    let profile_graph = match graph_guard.profiles.get(&profile_id) {
        Some(pg) => pg,
        None => return cx.throw_error(GraphError::ProfileNotFound(profile_id).to_string()),
    };

    if let Some(internal_id) = profile_graph.node_id_map.get(&node_id) {
        if let Some(node) = profile_graph.nodes.get(*internal_id as usize) {
            let js_object = cx.empty_object();

            let id_val = cx.number(node.external_id as f64);
            js_object.set(&mut cx, "id", id_val)?;

            let location_array = JsArray::new(&mut cx, 2);
            let lon = cx.number(node.lon);
            let lat = cx.number(node.lat);
            location_array.set(&mut cx, 0, lon)?;
            location_array.set(&mut cx, 1, lat)?;
            js_object.set(&mut cx, "location", location_array)?;

            let tags_obj = cx.empty_object();
            for (key, value) in &node.tags {
                let value_js = cx.string(value);
                tags_obj.set(&mut cx, key.as_str(), value_js)?;
            }
            js_object.set(&mut cx, "tags", tags_obj)?;

            return Ok(js_object.upcast());
        }
    }

    Ok(cx.null().upcast())
}

fn get_shape(mut cx: FunctionContext) -> JsResult<JsArray> {
    let graph_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let profile_id = cx.argument::<JsString>(1)?.value(&mut cx);
    let nodes_js = cx.argument::<JsArray>(2)?;

    let graph = match GRAPH_STORAGE.lock().unwrap().get(&graph_id) {
        Some(g) => g.clone(),
        None => return cx.throw_error(GraphError::GraphNotFound(graph_id).to_string()),
    };

    let graph_guard = graph.read().unwrap();
    let profile_graph = match graph_guard.profiles.get(&profile_id) {
        Some(pg) => pg,
        None => return cx.throw_error(GraphError::ProfileNotFound(profile_id).to_string()),
    };

    let len = nodes_js.len(&mut cx);
    let result = JsArray::new(&mut cx, len as usize);
    for i in 0..len {
        let node_id = nodes_js.get::<JsNumber, _, _>(&mut cx, i)?.value(&mut cx) as i64;

        if let Some(internal_id) = profile_graph.node_id_map.get(&node_id) {
            if let Some(node) = profile_graph.nodes.get(*internal_id as usize) {
                let point_array = JsArray::new(&mut cx, 2);
                let lon = cx.number(node.lon);
                let lat = cx.number(node.lat);
                point_array.set(&mut cx, 0, lon)?;
                point_array.set(&mut cx, 1, lat)?;
                result.set(&mut cx, i, point_array)?;
            }
        }
    }
    Ok(result)
}

fn unload_graph(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let graph_id_to_remove = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let removed_graph = GRAPH_STORAGE
        .lock()
        .unwrap()
        .remove(&graph_id_to_remove)
        .is_some();

    if !removed_graph {
        return Ok(cx.boolean(false));
    }

    let mut queues = ROUTE_QUEUES.lock().unwrap();
    let queue_ids_to_remove: Vec<i32> = queues
        .iter()
        .filter(|(_, queue)| queue.graph_id == graph_id_to_remove)
        .map(|(id, _)| *id)
        .collect();

    for queue_id in queue_ids_to_remove {
        queues.remove(&queue_id);
    }

    Ok(cx.boolean(true))
}

fn create_route_queue(mut cx: FunctionContext) -> JsResult<JsNumber> {
    let graph_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let profile_id = cx.argument::<JsString>(1)?.value(&mut cx);
    let max_concurrency = if cx.len() > 2 {
        Some(cx.argument::<JsNumber>(2)?.value(&mut cx) as usize)
    } else {
        None
    };

    if !GRAPH_STORAGE.lock().unwrap().contains_key(&graph_id) {
        return cx.throw_error(GraphError::GraphNotFound(graph_id).to_string());
    }

    let graph_arc = GRAPH_STORAGE
        .lock()
        .unwrap()
        .get(&graph_id)
        .unwrap()
        .clone();
    let graph_container = graph_arc.read().unwrap();
    if !graph_container.profiles.contains_key(&profile_id) {
        return cx.throw_error(GraphError::ProfileNotFound(profile_id).to_string());
    }

    let queue_id = unsafe {
        let id = NEXT_QUEUE_ID;
        NEXT_QUEUE_ID += 1;
        id
    };

    let route_queue = RouteQueue::new(graph_id, profile_id, max_concurrency);
    ROUTE_QUEUES
        .lock()
        .unwrap()
        .insert(queue_id, Arc::new(route_queue));
    Ok(cx.number(queue_id as f64))
}

fn enqueue_route(mut cx: FunctionContext) -> JsResult<JsString> {
    let queue_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let route_id = cx.argument::<JsString>(1)?.value(&mut cx);
    let start_node = cx.argument::<JsNumber>(2)?.value(&mut cx) as i64;
    let end_node = cx.argument::<JsNumber>(3)?.value(&mut cx) as i64;

    let queue = match ROUTE_QUEUES.lock().unwrap().get(&queue_id) {
        Some(q) => q.clone(),
        None => return cx.throw_error(format!("RouteQueue with ID {} not found", queue_id)),
    };

    let request_id = queue.enqueue(RouteRequest {
        id: route_id,
        start_node,
        end_node,
    });
    Ok(cx.string(request_id))
}

fn process_queue(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let queue_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let callback = cx.argument::<JsFunction>(1)?.root(&mut cx);

    let queue = match ROUTE_QUEUES.lock().unwrap().get(&queue_id) {
        Some(q) => q.clone(),
        None => return cx.throw_error(format!("RouteQueue with ID {} not found", queue_id)),
    };

    let graph_container = match GRAPH_STORAGE.lock().unwrap().get(&queue.graph_id) {
        Some(g) => g.clone(),
        None => return cx.throw_error(GraphError::GraphNotFound(queue.graph_id).to_string()),
    };

    let channel = cx.channel();

    queue.start_processing(channel, callback, graph_container);

    Ok(cx.undefined())
}

fn get_queue_status(mut cx: FunctionContext) -> JsResult<JsObject> {
    let queue_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let queue = match ROUTE_QUEUES.lock().unwrap().get(&queue_id) {
        Some(q) => q.clone(),
        None => return cx.throw_error(format!("RouteQueue with ID {} not found", queue_id)),
    };

    let obj = cx.empty_object();

    let queued_tasks = cx.number(queue.queue_size() as f64);
    let active_tasks = cx.number(queue.active_count() as f64);
    let is_empty = cx.boolean(queue.is_empty());

    obj.set(&mut cx, "queuedTasks", queued_tasks)?;
    obj.set(&mut cx, "activeTasks", active_tasks)?;
    obj.set(&mut cx, "isEmpty", is_empty)?;
    Ok(obj)
}

fn clear_route_queue(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let queue_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let removed = ROUTE_QUEUES.lock().unwrap().remove(&queue_id).is_some();
    Ok(cx.boolean(removed))
}

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function("loadGraph", load_graph)?;
    cx.export_function("unloadGraph", unload_graph)?;
    cx.export_function("getRoute", get_route)?;
    cx.export_function("getNearestNode", get_nearest_node)?;
    cx.export_function("getNodesInRadius", get_nodes_in_radius)?;
    cx.export_function("getNode", get_node)?;
    cx.export_function("getShape", get_shape)?;

    cx.export_function("createRouteQueue", create_route_queue)?;
    cx.export_function("enqueueRoute", enqueue_route)?;
    cx.export_function("processQueue", process_queue)?;
    cx.export_function("getQueueStatus", get_queue_status)?;
    cx.export_function("clearRouteQueue", clear_route_queue)?;

    Ok(())
}
