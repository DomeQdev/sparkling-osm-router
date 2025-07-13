use crate::graph::GraphContainer;
use neon::prelude::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Clone)]
pub struct RouteRequest {
    pub id: String,
    pub start_node: i64,
    pub end_node: i64,
}

pub struct RouteQueue {
    queue: Arc<Mutex<VecDeque<RouteRequest>>>,
    active_count: Arc<Mutex<usize>>,
    pub max_concurrency: usize,
    graph_container: Arc<std::sync::RwLock<GraphContainer>>,
    profile_id: String,
    callback: Arc<Mutex<Option<Root<JsFunction>>>>,
    pub graph_id: i32,
}

impl Finalize for RouteQueue {}

impl RouteQueue {
    pub fn new(
        graph_id: i32,
        graph_container: Arc<std::sync::RwLock<GraphContainer>>,
        profile_id: String,
        max_concurrency: Option<usize>,
    ) -> Self {
        let actual_concurrency = max_concurrency.unwrap_or_else(|| {
            let cpu_count = num_cpus::get();
            if cpu_count > 1 {
                cpu_count.saturating_sub(1)
            } else {
                1
            }
        });

        RouteQueue {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            active_count: Arc::new(Mutex::new(0)),
            max_concurrency: actual_concurrency,
            graph_container,
            profile_id,
            callback: Arc::new(Mutex::new(None)),
            graph_id,
        }
    }

    pub fn enqueue(&self, request: RouteRequest) -> String {
        let id = if request.id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            request.id.clone()
        };
        let mut queue = self.queue.lock().unwrap();
        queue.push_back(RouteRequest {
            id: id.clone(),
            ..request
        });
        id
    }

    pub fn start_processing(&self, channel: Channel, callback: Root<JsFunction>) {
        *self.callback.lock().unwrap() = Some(callback);

        let tasks_to_start = self.max_concurrency.min(self.queue_size());
        for _ in 0..tasks_to_start {
            self.process_next(channel.clone());
        }
    }

    fn process_next(&self, channel: Channel) {
        let can_process = *self.active_count.lock().unwrap() < self.max_concurrency;
        if !can_process {
            return;
        }

        let request = { self.queue.lock().unwrap().pop_front() };

        if let Some(request) = request {
            *self.active_count.lock().unwrap() += 1;

            let self_clone = self.clone();
            let graph_clone = self.graph_container.clone();

            crate::ROUTING_THREAD_POOL.spawn(move || {
                let result = {
                    let graph_guard = graph_clone.read().unwrap();
                    graph_guard.route(&self_clone.profile_id, request.start_node, request.end_node)
                };

                channel.send(move |mut cx| {
                    *self_clone.active_count.lock().unwrap() -= 1;

                    if let Some(callback) = self_clone.callback.lock().unwrap().as_ref() {
                        let callback = callback.to_inner(&mut cx);
                        let this = cx.undefined();
                        let id_js = cx.string(request.id);

                        let result_value: Handle<JsValue> = match result {
                            Ok(Some(nodes)) => {
                                let js_result = cx.empty_object();
                                let js_nodes = JsArray::new(&mut cx, nodes.len() as usize);
                                for (i, node_id) in nodes.iter().enumerate() {
                                    let js_node = cx.number(*node_id as f64);
                                    js_nodes.set(&mut cx, i as u32, js_node).unwrap();
                                }
                                js_result.set(&mut cx, "nodes", js_nodes).unwrap();
                                js_result.upcast()
                            }
                            Ok(None) => cx.null().upcast(),
                            Err(e) => cx.error(e.to_string())?.upcast(),
                        };

                        let args: Vec<Handle<JsValue>> = vec![id_js.upcast(), result_value];
                        let _ = callback.call(&mut cx, this, args);
                    }
                    self_clone.process_next(cx.channel());

                    Ok(())
                });
            });
        }
    }

    pub fn queue_size(&self) -> usize {
        self.queue.lock().unwrap().len()
    }

    pub fn active_count(&self) -> usize {
        *self.active_count.lock().unwrap()
    }

    pub fn is_empty(&self) -> bool {
        self.queue_size() == 0 && self.active_count() == 0
    }
}

impl Clone for RouteQueue {
    fn clone(&self) -> Self {
        RouteQueue {
            queue: self.queue.clone(),
            active_count: self.active_count.clone(),
            max_concurrency: self.max_concurrency,
            graph_container: self.graph_container.clone(),
            profile_id: self.profile_id.clone(),
            callback: self.callback.clone(),
            graph_id: self.graph_id,
        }
    }
}
