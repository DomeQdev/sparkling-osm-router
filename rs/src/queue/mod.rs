use neon::prelude::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Clone)]
pub struct RouteRequest {
    pub id: String,
    pub start_node: i64,
    pub end_node: i64,
    pub initial_bearing: Option<f64>,
}

pub struct RouteQueue {
    queue: Arc<Mutex<VecDeque<RouteRequest>>>,
    active_count: Arc<Mutex<usize>>,
    pub max_concurrency: usize,
    graph: Arc<std::sync::RwLock<crate::core::types::Graph>>,
}

impl RouteQueue {
    pub fn new(
        graph: Arc<std::sync::RwLock<crate::core::types::Graph>>,
        max_concurrency: Option<usize>,
    ) -> Self {
        let actual_concurrency = max_concurrency.unwrap_or_else(|| {
            let cpu_count = num_cpus::get();
            if cpu_count > 1 {
                cpu_count - 1
            } else {
                1
            }
        });

        RouteQueue {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            active_count: Arc::new(Mutex::new(0)),
            max_concurrency: actual_concurrency,
            graph,
        }
    }

    pub fn enqueue(&self, request: RouteRequest) -> String {
        let id = if request.id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            request.id.clone()
        };

        let mut queue = self.queue.lock().unwrap();
        queue.push_back(request);

        id
    }

    pub fn process_next(&self, channel: Channel, callback: Root<JsFunction>) -> bool {
        let can_process = {
            let active_count = self.active_count.lock().unwrap();
            *active_count < self.max_concurrency
        };

        if !can_process {
            return false;
        }

        let request = {
            let mut queue = self.queue.lock().unwrap();
            queue.pop_front()
        };

        if let Some(request) = request {
            {
                let mut active_count = self.active_count.lock().unwrap();
                *active_count += 1;
            }

            let self_clone = self.clone();
            let graph_clone = self.graph.clone();
            let request_id = request.id.clone();
            let start_node = request.start_node;
            let end_node = request.end_node;
            let initial_bearing = request.initial_bearing;
            let callback_clone = callback;
            let channel_clone = channel;

            crate::routing::ROUTING_THREAD_POOL
                .get()
                .unwrap()
                .spawn(move || {
                    let result = {
                        let graph_guard = graph_clone.read().unwrap();
                        crate::TOKIO_RUNTIME.block_on(async {
                            graph_guard
                                .route(start_node, end_node, initial_bearing)
                                .await
                        })
                    };

                    channel_clone.send(move |mut cx| {
                        let callback = callback_clone.into_inner(&mut cx);
                        let this = cx.undefined();

                        let id_js = cx.string(request_id);

                        let result_value = match result {
                            Ok(Some(route_result)) => {
                                let js_result = cx.empty_object();

                                let nodes = route_result.nodes;
                                let js_nodes = JsArray::new(&mut cx, nodes.len());
                                for (i, node_id) in nodes.iter().enumerate() {
                                    let js_node = cx.number(*node_id as f64);
                                    js_nodes.set(&mut cx, i as u32, js_node).unwrap();
                                }
                                js_result.set(&mut cx, "nodes", js_nodes).unwrap();

                                let ways = route_result.ways;
                                let js_ways = JsArray::new(&mut cx, ways.len());
                                for (i, way_id) in ways.iter().enumerate() {
                                    let js_way = cx.number(*way_id as f64);
                                    js_ways.set(&mut cx, i as u32, js_way).unwrap();
                                }
                                js_result.set(&mut cx, "ways", js_ways).unwrap();

                                js_result.upcast::<JsValue>()
                            }
                            Ok(None) => cx.null().upcast::<JsValue>(),
                            Err(e) => {
                                let err = cx.error(e.to_string()).unwrap();
                                err.upcast::<JsValue>()
                            }
                        };

                        let args: Vec<Handle<JsValue>> = vec![id_js.upcast(), result_value];

                        let _ = callback.call(&mut cx, this, args);

                        {
                            let mut active_count = self_clone.active_count.lock().unwrap();
                            *active_count -= 1;
                        }

                        Ok(())
                    });
                });

            return true;
        }

        false
    }

    pub fn start_processing<'a, C: Context<'a>>(
        &self,
        cx: &mut C,
        channel: Channel,
        callback: Root<JsFunction>,
        count: usize,
    ) -> usize {
        let queue_size = {
            let queue = self.queue.lock().unwrap();
            queue.len()
        };

        let tasks_to_start = count.min(queue_size);

        let mut callbacks = Vec::with_capacity(tasks_to_start);
        for _ in 0..tasks_to_start {
            callbacks.push(callback.clone(cx));
        }

        for callback_clone in callbacks {
            self.process_next(channel.clone(), callback_clone);
        }

        tasks_to_start
    }

    pub fn is_empty(&self) -> bool {
        let queue_size = self.queue.lock().unwrap().len();
        let active_count = *self.active_count.lock().unwrap();

        queue_size == 0 && active_count == 0
    }

    pub fn queue_size(&self) -> usize {
        let queue = self.queue.lock().unwrap();
        queue.len()
    }

    pub fn active_count(&self) -> usize {
        let active_count = self.active_count.lock().unwrap();
        *active_count
    }

    pub fn clear(&self) {
        let mut queue = self.queue.lock().unwrap();
        queue.clear();
    }
}

impl Clone for RouteQueue {
    fn clone(&self) -> Self {
        RouteQueue {
            queue: self.queue.clone(),
            active_count: self.active_count.clone(),
            max_concurrency: self.max_concurrency,
            graph: self.graph.clone(),
        }
    }
}
