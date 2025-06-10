mod core;
mod parser;
mod queue;
mod routing;
mod spatial;

use core::types::{Graph, Profile};
use lazy_static::lazy_static;
use neon::prelude::*;
use parser::parse_osm_xml;
use queue::{RouteQueue, RouteRequest};
use routing::{init_routing_thread_pool, ROUTING_THREAD_POOL};
use spatial::indexer::{index_graph, GRAPH_NODES};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::runtime::Runtime;

lazy_static! {
    static ref TOKIO_RUNTIME: Runtime = Runtime::new().expect("Failed to create Tokio runtime");
    static ref GRAPH_STORAGE: Mutex<HashMap<i32, Arc<RwLock<Graph>>>> = Mutex::new(HashMap::new());
    static ref PROFILE_STORAGE: Mutex<HashMap<i32, Arc<Profile>>> = Mutex::new(HashMap::new());
    static ref ROUTE_QUEUES: Mutex<HashMap<i32, Arc<RouteQueue>>> = Mutex::new(HashMap::new());
}

static mut NEXT_GRAPH_ID: i32 = 1;
static mut NEXT_PROFILE_ID: i32 = 1;
static mut NEXT_QUEUE_ID: i32 = 1;

fn load_graph_rust(mut cx: FunctionContext) -> JsResult<JsNumber> {
    let file_path_js = cx.argument::<JsString>(0)?;
    let file_path = file_path_js.value(&mut cx);

    let graph = Graph::new();
    let graph_arc = Arc::new(RwLock::new(graph));

    let graph_id = unsafe {
        let id = NEXT_GRAPH_ID;
        NEXT_GRAPH_ID += 1;
        id
    };

    GRAPH_STORAGE
        .lock()
        .unwrap()
        .insert(graph_id, graph_arc.clone());

    let parsed_graph = match parse_osm_xml(&file_path) {
        Ok(g) => g,
        Err(parse_err) => {
            GRAPH_STORAGE.lock().unwrap().remove(&graph_id);
            return cx.throw_error(&format!("OSM XML parsing failed: {}", parse_err));
        }
    };

    GRAPH_NODES.with(|gn| {
        *gn.borrow_mut() = parsed_graph.nodes.clone();
    });

    let index_result = index_graph(parsed_graph);
    match index_result {
        Ok(indexed_graph) => {
            let mut graph_write_guard = graph_arc.write().unwrap();
            *graph_write_guard = indexed_graph;
            Ok(cx.number(graph_id as f64))
        }
        Err(index_err) => {
            GRAPH_STORAGE.lock().unwrap().remove(&graph_id);
            cx.throw_error(&format!("Graph indexing failed: {}", index_err))
        }
    }
}

fn create_profile_rust(mut cx: FunctionContext) -> JsResult<JsNumber> {
    let profile_options_json = cx.argument::<JsString>(0)?.value(&mut cx);

    match serde_json::from_str::<Profile>(&profile_options_json) {
        Ok(profile) => {
            let profile_id = unsafe {
                let id = NEXT_PROFILE_ID;
                NEXT_PROFILE_ID += 1;
                id
            };
            PROFILE_STORAGE
                .lock()
                .unwrap()
                .insert(profile_id, Arc::new(profile));
            Ok(cx.number(profile_id as f64))
        }
        Err(e) => {
            return cx.throw_error(&format!("Invalid profile JSON: {}", e));
        }
    }
}

fn get_nearest_nodes_rust(mut cx: FunctionContext) -> JsResult<JsArray> {
    let graph_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let profile_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as i32;
    let lon = cx.argument::<JsNumber>(2)?.value(&mut cx);
    let lat = cx.argument::<JsNumber>(3)?.value(&mut cx);
    let limit = cx.argument::<JsNumber>(4)?.value(&mut cx) as usize;
    let distance_threshold_multiplier = cx.argument::<JsNumber>(5)?.value(&mut cx);

    let graph_store = match GRAPH_STORAGE.lock().unwrap().get(&graph_id) {
        Some(graph) => graph.clone(),
        None => return cx.throw_error(&format!("Graph with ID {} does not exist", graph_id)),
    };

    let profile_store = PROFILE_STORAGE.lock().unwrap();
    let profile = match profile_store.get(&profile_id) {
        Some(p) => p.clone(),
        None => return cx.throw_error(&format!("Profile with ID {} does not exist", profile_id)),
    };
    drop(profile_store);

    let nearest_result = graph_store.read().unwrap().find_nearest_ways_and_nodes(
        lon,
        lat,
        limit,
        distance_threshold_multiplier,
        &profile,
    );

    match nearest_result {
        Ok(node_ids) => {
            let result = JsArray::new(&mut cx, node_ids.len());

            for (i, node_id) in node_ids.iter().enumerate() {
                let js_value = cx.number(*node_id as f64);
                result.set(&mut cx, i as u32, js_value)?;
            }

            Ok(result)
        }
        Err(e) => cx.throw_error(&format!("Error finding nearest ways and nodes: {}", e)),
    }
}

fn get_route_rust(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let graph_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let profile_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as i32;
    let start_node = cx.argument::<JsNumber>(2)?.value(&mut cx) as i64;
    let end_node = cx.argument::<JsNumber>(3)?.value(&mut cx) as i64;

    let graph_store = match GRAPH_STORAGE.lock().unwrap().get(&graph_id) {
        Some(graph) => graph.clone(),
        None => return cx.throw_error(&format!("Graph with ID {} does not exist", graph_id)),
    };

    let profile_store = PROFILE_STORAGE.lock().unwrap();
    let profile = match profile_store.get(&profile_id) {
        Some(p) => p.clone(),
        None => return cx.throw_error(&format!("Profile with ID {} does not exist", profile_id)),
    };
    drop(profile_store);

    let channel = cx.channel();
    let (deferred, promise) = cx.promise();

    init_routing_thread_pool();

    ROUTING_THREAD_POOL.get().unwrap().spawn(move || {
        let graph_read_guard = graph_store.read().unwrap();
        let async_result = TOKIO_RUNTIME
            .block_on(async { graph_read_guard.route(start_node, end_node, &profile).await });

        deferred.settle_with(&channel, move |mut cx| match async_result {
            Ok(Some(route_result)) => {
                let js_result = JsObject::new(&mut cx);

                let nodes = route_result.nodes;
                let js_nodes = JsArray::new(&mut cx, nodes.len());
                for (i, node_id) in nodes.iter().enumerate() {
                    let js_number = cx.number(*node_id as f64);
                    js_nodes.set(&mut cx, i as u32, js_number)?;
                }
                js_result.set(&mut cx, "nodes", js_nodes)?;

                let ways = route_result.ways;
                let js_ways = JsArray::new(&mut cx, ways.len());
                for (i, way_id) in ways.iter().enumerate() {
                    let js_number = cx.number(*way_id as f64);
                    js_ways.set(&mut cx, i as u32, js_number)?;
                }
                js_result.set(&mut cx, "ways", js_ways)?;

                Ok(js_result)
            }
            Ok(None) => {
                let js_result = JsObject::new(&mut cx);
                let js_nodes = JsArray::new(&mut cx, 0);
                let js_ways = JsArray::new(&mut cx, 0);
                js_result.set(&mut cx, "nodes", js_nodes)?;
                js_result.set(&mut cx, "ways", js_ways)?;
                Ok(js_result)
            }
            Err(e) => cx.throw_error(&format!("Error during async routing: {}", e)),
        });
    });

    Ok(promise)
}

fn get_node_rust(mut cx: FunctionContext) -> JsResult<JsValue> {
    let graph_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let node_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as i64;

    let graph_store = match GRAPH_STORAGE.lock().unwrap().get(&graph_id) {
        Some(graph) => graph.clone(),
        None => return cx.throw_error(&format!("Graph with ID {} does not exist", graph_id)),
    };

    let graph = graph_store.read().unwrap();

    if let Some(node) = graph.nodes.get(&node_id) {
        let js_object = cx.empty_object();

        let id_js = cx.number(node.id as f64);
        js_object.set(&mut cx, "id", id_js)?;

        let lat_js = cx.number(node.lat);
        let lon_js = cx.number(node.lon);
        js_object.set(&mut cx, "lat", lat_js)?;
        js_object.set(&mut cx, "lon", lon_js)?;

        let tags_obj = cx.empty_object();
        for (key, value) in &node.tags {
            let value_js = cx.string(value);
            tags_obj.set(&mut cx, key.as_str(), value_js)?;
        }
        js_object.set(&mut cx, "tags", tags_obj)?;

        return Ok(js_object.upcast());
    }

    Ok(cx.null().upcast())
}

fn get_way_rust(mut cx: FunctionContext) -> JsResult<JsValue> {
    let graph_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let way_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as i64;

    let graph_store = match GRAPH_STORAGE.lock().unwrap().get(&graph_id) {
        Some(graph) => graph.clone(),
        None => return cx.throw_error(&format!("Graph with ID {} does not exist", graph_id)),
    };

    let graph = graph_store.read().unwrap();

    if let Some(way) = graph.ways.get(&way_id) {
        let js_object = cx.empty_object();

        let id_js = cx.number(way.id as f64);
        js_object.set(&mut cx, "id", id_js)?;

        let node_refs = &way.node_refs;
        let node_refs_js = JsArray::new(&mut cx, node_refs.len());
        for (i, node_id) in node_refs.iter().enumerate() {
            let node_id_js = cx.number(*node_id as f64);
            node_refs_js.set(&mut cx, i as u32, node_id_js)?;
        }
        js_object.set(&mut cx, "nodes", node_refs_js)?;

        let tags_obj = cx.empty_object();
        for (key, value) in &way.tags {
            let value_js = cx.string(value);
            tags_obj.set(&mut cx, key.as_str(), value_js)?;
        }
        js_object.set(&mut cx, "tags", tags_obj)?;

        return Ok(js_object.upcast());
    }

    Ok(cx.null().upcast())
}

fn unload_graph(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let graph_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;

    let mut graph_storage = GRAPH_STORAGE.lock().unwrap();
    let removed = graph_storage.remove(&graph_id).is_some();

    Ok(cx.boolean(removed))
}

fn get_shape_rust(mut cx: FunctionContext) -> JsResult<JsArray> {
    let graph_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let nodes = cx.argument::<JsArray>(1)?;

    let graph_store = match GRAPH_STORAGE.lock().unwrap().get(&graph_id) {
        Some(graph) => graph.clone(),
        None => return cx.throw_error(&format!("Graph with ID {} does not exist", graph_id)),
    };

    let mut nodes_vec = Vec::with_capacity(nodes.len(&mut cx) as usize);
    for i in 0..nodes.len(&mut cx) {
        let node_id = nodes.get::<JsNumber, _, u32>(&mut cx, i)?.value(&mut cx) as i64;
        nodes_vec.push(node_id);
    }

    let graph = graph_store.read().unwrap();
    let node_data: Vec<_> = nodes_vec
        .iter()
        .filter_map(|&id| graph.nodes.get(&id))
        .collect();

    let result = JsArray::new(&mut cx, node_data.len());
    for (i, node) in node_data.iter().enumerate() {
        let point_array = JsArray::new(&mut cx, 2);
        let lon = cx.number(node.lon);
        let lat = cx.number(node.lat);
        point_array.set(&mut cx, 0, lon)?;
        point_array.set(&mut cx, 1, lat)?;
        result.set(&mut cx, i as u32, point_array)?;
    }

    Ok(result)
}

fn create_route_queue(mut cx: FunctionContext) -> JsResult<JsNumber> {
    let graph_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let profile_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as i32;

    let max_concurrency = if cx.len() > 2 {
        Some(cx.argument::<JsNumber>(2)?.value(&mut cx) as usize)
    } else {
        None
    };

    let graph_store_lock = GRAPH_STORAGE.lock().unwrap();
    let graph_arc = match graph_store_lock.get(&graph_id) {
        Some(graph) => graph.clone(),
        None => return cx.throw_error(&format!("Graph with ID {} does not exist", graph_id)),
    };
    drop(graph_store_lock);

    let profile_store_lock = PROFILE_STORAGE.lock().unwrap();
    let profile_arc = match profile_store_lock.get(&profile_id) {
        Some(profile) => profile.clone(),
        None => return cx.throw_error(&format!("Profile with ID {} does not exist", profile_id)),
    };
    drop(profile_store_lock);

    let route_queue = RouteQueue::new(graph_arc, profile_arc, max_concurrency);
    let queue_id = unsafe {
        let id = NEXT_QUEUE_ID;
        NEXT_QUEUE_ID += 1;
        id
    };

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

    let queues = ROUTE_QUEUES.lock().unwrap();
    let queue = match queues.get(&queue_id) {
        Some(queue) => queue.clone(),
        None => return cx.throw_error(&format!("RouteQueue with ID {} does not exist", queue_id)),
    };

    let request = RouteRequest {
        id: route_id.clone(),
        start_node,
        end_node,
    };

    let request_id = queue.enqueue(request);

    Ok(cx.string(request_id))
}

fn start_queue_processing(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let queue_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
    let callback = cx.argument::<JsFunction>(1)?.root(&mut cx);

    let queue = match ROUTE_QUEUES.lock().unwrap().get(&queue_id) {
        Some(queue) => queue.clone(),
        None => return cx.throw_error(&format!("RouteQueue with ID {} does not exist", queue_id)),
    };

    let channel = cx.channel();

    if queue.is_empty() {
        return Ok(cx.undefined());
    }

    let callback_clone = callback.clone(&mut cx);
    let max_concurrency = queue.max_concurrency;
    queue.start_processing(&mut cx, channel.clone(), callback_clone, max_concurrency);

    let process_checker = JsFunction::new(&mut cx, move |mut cx| {
        let queue_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;
        let route_callback = cx.argument::<JsFunction>(1)?.root(&mut cx);

        let queue = match ROUTE_QUEUES.lock().unwrap().get(&queue_id) {
            Some(q) => q.clone(),
            None => return Ok(cx.undefined()),
        };

        if queue.is_empty() {
            return Ok(cx.undefined());
        }

        let channel = cx.channel();
        queue.start_processing(&mut cx, channel, route_callback, queue.max_concurrency);

        Ok(cx.undefined())
    })?;

    let setup_interval = JsFunction::new(&mut cx, move |mut cx| {
        let global = cx.global::<JsObject>("global").unwrap_or_else(|_| {
            cx.global::<JsObject>("window").unwrap_or_else(|_| {
                cx.global::<JsObject>("self")
                    .unwrap_or_else(|_| cx.global::<JsObject>("globalThis").unwrap())
            })
        });

        let set_interval = global
            .get::<JsFunction, _, _>(&mut cx, "setInterval")
            .unwrap();

        let _clear_interval = global
            .get::<JsFunction, _, _>(&mut cx, "clearInterval")
            .unwrap();

        let check_fn = cx.argument::<JsFunction>(0)?;
        let queue_id = cx.argument::<JsNumber>(1)?.value(&mut cx);
        let route_callback = cx.argument::<JsFunction>(2)?;
        let interval = cx.number(100);

        let args: Vec<Handle<JsValue>> = vec![
            check_fn.upcast(),
            interval.upcast(),
            cx.number(queue_id).upcast(),
            route_callback.upcast(),
        ];

        let interval_id = set_interval.call(&mut cx, global, args)?;

        let check_queue = JsFunction::new(&mut cx, move |mut cx| {
            let interval_id = cx.argument::<JsValue>(0)?;
            let queue_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as i32;

            let queue = match ROUTE_QUEUES.lock().unwrap().get(&queue_id) {
                Some(q) => q.clone(),
                None => {
                    let global = cx.global::<JsObject>("globalThis").unwrap();
                    let clear_interval = global
                        .get::<JsFunction, _, _>(&mut cx, "clearInterval")
                        .unwrap();
                    let _ = clear_interval.call(&mut cx, global, [interval_id]);
                    return Ok(cx.undefined());
                }
            };

            if queue.is_empty() {
                let global = cx.global::<JsObject>("globalThis").unwrap();
                let clear_interval = global
                    .get::<JsFunction, _, _>(&mut cx, "clearInterval")
                    .unwrap();
                let _ = clear_interval.call(&mut cx, global, [interval_id]);
            }

            Ok(cx.undefined())
        })?;

        let check_args: Vec<Handle<JsValue>> = vec![
            check_queue.upcast(),
            cx.number(500).upcast(),
            interval_id,
            cx.number(queue_id).upcast(),
        ];

        let _ = set_interval.call(&mut cx, global, check_args)?;

        Ok(cx.undefined())
    })?;

    let undefined = cx.undefined();
    let queue_id_arg = cx.number(queue_id);
    let input_callback_arg = cx.argument::<JsFunction>(1)?;

    let mut call_args: Vec<Handle<JsValue>> = Vec::new();
    call_args.push(process_checker.upcast());
    call_args.push(queue_id_arg.upcast());
    call_args.push(input_callback_arg.upcast());

    setup_interval.call(&mut cx, undefined, call_args)?;

    Ok(cx.undefined())
}

fn get_queue_status(mut cx: FunctionContext) -> JsResult<JsObject> {
    let queue_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;

    let queue = match ROUTE_QUEUES.lock().unwrap().get(&queue_id) {
        Some(queue) => queue.clone(),
        None => return cx.throw_error(&format!("RouteQueue with ID {} does not exist", queue_id)),
    };

    let obj = cx.empty_object();
    let queue_size = cx.number(queue.queue_size() as f64);
    let active_count = cx.number(queue.active_count() as f64);
    let is_empty = cx.boolean(queue.is_empty());

    obj.set(&mut cx, "queuedTasks", queue_size)?;
    obj.set(&mut cx, "activeTasks", active_count)?;
    obj.set(&mut cx, "isEmpty", is_empty)?;

    Ok(obj)
}

fn clear_route_queue(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let queue_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;

    let mut queues = ROUTE_QUEUES.lock().unwrap();
    let removed = queues.remove(&queue_id).is_some();

    Ok(cx.boolean(removed))
}

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function("loadGraph", load_graph_rust)?;
    cx.export_function("createProfile", create_profile_rust)?;
    cx.export_function("getNearestNodes", get_nearest_nodes_rust)?;
    cx.export_function("getRoute", get_route_rust)?;
    cx.export_function("getNode", get_node_rust)?;
    cx.export_function("getWay", get_way_rust)?;
    cx.export_function("getShape", get_shape_rust)?;
    cx.export_function("unloadGraph", unload_graph)?;

    cx.export_function("createRouteQueue", create_route_queue)?;
    cx.export_function("enqueueRoute", enqueue_route)?;
    cx.export_function("startQueueProcessing", start_queue_processing)?;
    cx.export_function("getQueueStatus", get_queue_status)?;
    cx.export_function("clearRouteQueue", clear_route_queue)?;

    Ok(())
}
