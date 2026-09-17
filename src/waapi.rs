// GWwiseAgent WAAPI Module — © 2025-2026 william.wang

use chrono::Local;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

const WAMP_HELLO: u64 = 1;
const WAMP_WELCOME: u64 = 2;
const WAMP_ABORT: u64 = 3;
const WAMP_CALL: u64 = 48;
const WAMP_RESULT: u64 = 50;
const WAMP_ERROR: u64 = 8;
const WAMP_SUBSCRIBE: u64 = 32;
const WAMP_SUBSCRIBED: u64 = 33;
const WAMP_UNSUBSCRIBE: u64 = 34;
const WAMP_UNSUBSCRIBED: u64 = 35;
const WAMP_EVENT: u64 = 36;

/// 每个订阅最多缓存多少条事件。超出后丢弃最旧的，并记录丢弃数量，
/// 避免长时间不轮询把内存吃光。
const MAX_BUFFERED_EVENTS: usize = 500;

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// 已订阅 topic 的运行时状态。
struct SubscriptionState {
    topic: String,
    buffer: VecDeque<Value>,
    total_received: u64,
    dropped: u64,
}

impl SubscriptionState {
    fn new(topic: String) -> Self {
        Self {
            topic,
            buffer: VecDeque::new(),
            total_received: 0,
            dropped: 0,
        }
    }

    /// 收录一条事件。缓冲满时丢弃最旧的一条并计数，
    /// 这样长时间不轮询也不会无限占用内存。
    fn record(&mut self, subscription_id: u64, payload: Value) {
        self.total_received += 1;
        if self.buffer.len() >= MAX_BUFFERED_EVENTS {
            self.buffer.pop_front();
            self.dropped += 1;
        }
        self.buffer.push_back(json!({
            "topic": self.topic,
            "subscription_id": subscription_id,
            "sequence": self.total_received,
            "received_at": Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            "payload": payload,
        }));
    }
}

/// 对外暴露的订阅概览。
#[derive(Debug, Clone)]
pub struct SubscriptionInfo {
    pub subscription_id: u64,
    pub topic: String,
    pub buffered: usize,
    pub total_received: u64,
    pub dropped: u64,
}

impl SubscriptionInfo {
    pub fn to_json(&self) -> Value {
        json!({
            "subscription_id": self.subscription_id,
            "topic": self.topic,
            "buffered": self.buffered,
            "total_received": self.total_received,
            "dropped": self.dropped,
        })
    }
}

type SubscriptionMap = Arc<Mutex<HashMap<u64, SubscriptionState>>>;
/// request_id → topic，用于在收到 SUBSCRIBED 时确定这个订阅对应哪个 topic。
/// 由发起方写入、读循环消费，避免"先收到事件、后登记订阅"的竞态。
type PendingSubscribeMap = Arc<Mutex<HashMap<u64, String>>>;
/// WELCOME / ABORT 的一次性回传通道。
type WelcomeSlot = Arc<Mutex<Option<oneshot::Sender<Result<u64, String>>>>>;

pub struct WaapiClient {
    writer: Arc<
        Mutex<
            Option<
                futures_util::stream::SplitSink<
                    tokio_tungstenite::WebSocketStream<
                        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                    >,
                    Message,
                >,
            >,
        >,
    >,
    pending: PendingMap,
    connected: Arc<AtomicBool>,
    subscriptions: SubscriptionMap,
    pending_subscribes: PendingSubscribeMap,
    /// WAMP 会话是否已建立。Wwise 允许不握手就 CALL，但 SUBSCRIBE 必须先 HELLO。
    has_session: Arc<AtomicBool>,
    welcome_slot: WelcomeSlot,
}

impl WaapiClient {
    pub fn new() -> Self {
        Self {
            writer: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            connected: Arc::new(AtomicBool::new(false)),
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
            pending_subscribes: Arc::new(Mutex::new(HashMap::new())),
            has_session: Arc::new(AtomicBool::new(false)),
            welcome_slot: Arc::new(Mutex::new(None)),
        }
    }

    /// WAMP 会话是否握手成功。只有握手成功才能订阅。
    pub fn has_wamp_session(&self) -> bool {
        self.has_session.load(Ordering::Relaxed)
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub async fn connect(&self, host: &str, port: u16) -> Result<(), String> {
        if self.is_connected() {
            self.disconnect().await;
        }

        let resolved_host = if host == "localhost" {
            "127.0.0.1"
        } else {
            host
        };

        let ports_to_try: Vec<u16> = {
            let mut p = vec![port];
            for candidate in [8080, 8081, 8082, 8090, 8095, 8078, 8079] {
                if !p.contains(&candidate) {
                    p.push(candidate);
                }
            }
            p
        };

        let mut last_err = String::new();
        let mut ws_stream = None;

        for &p in &ports_to_try {
            let url = format!("ws://{}:{}/waapi", resolved_host, p);
            let ws_result =
                tokio::time::timeout(std::time::Duration::from_secs(2), connect_async(&url)).await;

            match ws_result {
                Ok(Ok(pair)) => {
                    ws_stream = Some(pair);
                    crate::llm::debug_log(&format!("[WAAPI] Connected on port {}", p));
                    break;
                }
                Ok(Err(e)) => {
                    last_err = format!("Port {}: {}", p, e);
                }
                Err(_) => {
                    last_err = format!("Port {}: timed out", p);
                }
            }
        }

        let (ws, _) = ws_stream.ok_or(format!(
            "Failed to connect to Wwise on {}. Tried ports {:?}. Last error: {}. Is Wwise running with WAAPI enabled?",
            resolved_host, ports_to_try, last_err
        ))?;

        let (writer, mut reader) = ws.split();
        *self.writer.lock().await = Some(writer);
        self.connected.store(true, Ordering::Relaxed);

        // 旧 socket 上的订阅已经失效。先记下订阅过哪些 topic，
        // 握手成功后照原样重建，这样重连不会让调用方悄无声息地漏事件。
        let topics_to_restore: Vec<String> = {
            let subs = self.subscriptions.lock().await;
            let mut topics: Vec<String> = subs.values().map(|s| s.topic.clone()).collect();
            topics.sort();
            topics.dedup();
            topics
        };
        self.subscriptions.lock().await.clear();
        self.pending_subscribes.lock().await.clear();
        self.has_session.store(false, Ordering::Relaxed);

        let pending = self.pending.clone();
        let connected = self.connected.clone();
        let subscriptions = self.subscriptions.clone();
        let pending_subscribes = self.pending_subscribes.clone();
        let has_session = self.has_session.clone();
        let welcome_slot = self.welcome_slot.clone();
        tokio::spawn(async move {
            while let Some(msg) = reader.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        crate::llm::debug_log(&format!(
                            "[WAAPI-RECV] {}",
                            &text[..text.len().min(500)]
                        ));

                        let arr = match serde_json::from_str::<Value>(&text) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };

                        let items = match arr.as_array() {
                            Some(a) => a,
                            None => continue,
                        };

                        if items.is_empty() {
                            continue;
                        }
                        let msg_type = items[0].as_u64().unwrap_or(0);

                        match msg_type {
                            WAMP_WELCOME => {
                                // [2, sessionId, details]
                                let session_id = items.get(1).and_then(Value::as_u64).unwrap_or(0);
                                has_session.store(true, Ordering::Relaxed);
                                crate::llm::debug_log(&format!(
                                    "[WAAPI] WAMP session established (id {})",
                                    session_id
                                ));
                                if let Some(tx) = welcome_slot.lock().await.take() {
                                    tx.send(Ok(session_id)).ok();
                                }
                            }
                            WAMP_ABORT => {
                                // [3, details, reason]
                                let reason = items
                                    .get(2)
                                    .and_then(Value::as_str)
                                    .unwrap_or("unknown reason")
                                    .to_string();
                                has_session.store(false, Ordering::Relaxed);
                                if let Some(tx) = welcome_slot.lock().await.take() {
                                    tx.send(Err(reason)).ok();
                                }
                            }
                            WAMP_RESULT => {
                                // [50, requestId, details, args_list?, kwargs?]
                                if items.len() < 3 {
                                    continue;
                                }
                                let req_id = items[1].as_u64().unwrap_or(0);
                                let result = if items.len() >= 5 {
                                    items[4].clone()
                                } else if items.len() >= 4 {
                                    items[3].clone()
                                } else {
                                    Value::Object(Default::default())
                                };

                                let mut map = pending.lock().await;
                                if let Some(tx) = map.remove(&req_id) {
                                    tx.send(Ok(result)).ok();
                                }
                            }
                            WAMP_ERROR => {
                                // [8, CALL_TYPE, requestId, details, error_uri, args_list?, kwargs?]
                                if items.len() < 5 {
                                    continue;
                                }
                                let req_id = items[2].as_u64().unwrap_or(0);
                                let error_uri = items[4].as_str().unwrap_or("unknown error");
                                let details = if items.len() >= 7 {
                                    serde_json::to_string(&items[6]).unwrap_or_default()
                                } else if items.len() >= 6 {
                                    serde_json::to_string(&items[5]).unwrap_or_default()
                                } else {
                                    String::new()
                                };

                                let err_msg = if details.is_empty() {
                                    error_uri.to_string()
                                } else {
                                    format!("{}: {}", error_uri, details)
                                };

                                let mut map = pending.lock().await;
                                if let Some(tx) = map.remove(&req_id) {
                                    tx.send(Err(err_msg)).ok();
                                }
                                // 订阅失败就不要留下悬空的 topic 记录
                                pending_subscribes.lock().await.remove(&req_id);
                            }
                            WAMP_SUBSCRIBED => {
                                // [33, requestId, subscriptionId]
                                if items.len() < 3 {
                                    continue;
                                }
                                let req_id = items[1].as_u64().unwrap_or(0);
                                let sub_id = items[2].as_u64().unwrap_or(0);

                                // 由读循环登记订阅，保证在任何 EVENT 之前完成
                                if let Some(topic) =
                                    pending_subscribes.lock().await.remove(&req_id)
                                {
                                    subscriptions
                                        .lock()
                                        .await
                                        .insert(sub_id, SubscriptionState::new(topic));
                                }

                                let mut map = pending.lock().await;
                                if let Some(tx) = map.remove(&req_id) {
                                    tx.send(Ok(json!({ "subscriptionId": sub_id }))).ok();
                                }
                            }
                            WAMP_UNSUBSCRIBED => {
                                // [35, requestId]
                                if items.len() < 2 {
                                    continue;
                                }
                                let req_id = items[1].as_u64().unwrap_or(0);
                                let mut map = pending.lock().await;
                                if let Some(tx) = map.remove(&req_id) {
                                    tx.send(Ok(json!({}))).ok();
                                }
                            }
                            WAMP_EVENT => {
                                // [36, subscriptionId, publicationId, details, args?, kwargs?]
                                if items.len() < 3 {
                                    continue;
                                }
                                let sub_id = items[1].as_u64().unwrap_or(0);
                                let payload = if items.len() >= 6 {
                                    items[5].clone()
                                } else if items.len() >= 5 {
                                    items[4].clone()
                                } else {
                                    Value::Object(Default::default())
                                };

                                let mut subs = subscriptions.lock().await;
                                if let Some(state) = subs.get_mut(&sub_id) {
                                    state.record(sub_id, payload);
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(Message::Close(_)) | Err(_) => {
                        connected.store(false, Ordering::Relaxed);
                        has_session.store(false, Ordering::Relaxed);
                        break;
                    }
                    _ => {}
                }
            }
        });

        // WAMP 握手。Wwise 对 CALL 宽容，但 SUBSCRIBE 前必须先 HELLO，
        // 所以握手失败只记日志、不阻断连接：普通调用仍然可用。
        match self.wamp_handshake().await {
            Ok(_) => {
                for topic in topics_to_restore {
                    match self.subscribe(&topic, None).await {
                        Ok(id) => crate::llm::debug_log(&format!(
                            "[WAAPI] Restored subscription '{}' as id {}",
                            topic, id
                        )),
                        Err(e) => crate::llm::debug_log(&format!(
                            "[WAAPI] Failed to restore subscription '{}': {}",
                            topic, e
                        )),
                    }
                }
            }
            Err(e) => {
                crate::llm::debug_log(&format!(
                    "[WAAPI] WAMP handshake failed ({}). Calls will work, subscriptions will not.",
                    e
                ));
            }
        }

        Ok(())
    }

    /// 发 WAMP HELLO 并等 WELCOME。Wwise 不校验 realm 名，任何 realm 都会被接受。
    async fn wamp_handshake(&self) -> Result<u64, String> {
        let (tx, rx) = oneshot::channel();
        *self.welcome_slot.lock().await = Some(tx);

        // WAMP HELLO: [1, realm, details]
        let hello = json!([
            WAMP_HELLO,
            "realm1",
            {
                "roles": {
                    "caller": {},
                    "callee": {},
                    "subscriber": {},
                    "publisher": {}
                }
            }
        ]);

        if let Err(e) = self.send_raw(&hello).await {
            self.welcome_slot.lock().await.take();
            return Err(e);
        }

        match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
            Ok(Ok(Ok(session_id))) => Ok(session_id),
            Ok(Ok(Err(reason))) => Err(format!("WAMP session aborted: {}", reason)),
            Ok(Err(_)) => Err("WAMP welcome channel closed unexpectedly".into()),
            Err(_) => {
                self.welcome_slot.lock().await.take();
                Err("no WELCOME received within 5s".into())
            }
        }
    }

    /// 直接往 socket 写一条 WAMP 报文，不等待回复。
    async fn send_raw(&self, msg: &Value) -> Result<(), String> {
        let mut writer_guard = self.writer.lock().await;
        let w = writer_guard
            .as_mut()
            .ok_or_else(|| "Not connected to Wwise".to_string())?;
        let text = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        crate::llm::debug_log(&format!("[WAAPI-SEND] {}", &text[..text.len().min(500)]));
        w.send(Message::Text(text.into())).await.map_err(|e| {
            self.connected.store(false, Ordering::Relaxed);
            format!("Send failed (connection lost?): {}", e)
        })
    }

    pub async fn disconnect(&self) {
        self.connected.store(false, Ordering::Relaxed);
        self.has_session.store(false, Ordering::Relaxed);
        self.subscriptions.lock().await.clear();
        self.pending_subscribes.lock().await.clear();
        if let Some(mut w) = self.writer.lock().await.take() {
            w.close().await.ok();
        }
    }

    pub async fn call(
        &self,
        uri: &str,
        args: Value,
        options: Option<Value>,
    ) -> Result<Value, String> {
        if !self.is_connected() {
            return Err(
                "Not connected to Wwise. Click the Wwise button in the status bar to connect."
                    .into(),
            );
        }

        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);

        // WAMP CALL: [48, requestId, options, procedure, [], kwargs]
        let opts = options.unwrap_or(json!({}));
        let msg = json!([WAMP_CALL, id, opts, uri, [], args]);

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        {
            let mut writer_guard = self.writer.lock().await;
            if let Some(ref mut w) = *writer_guard {
                let text = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
                crate::llm::debug_log(&format!("[WAAPI-SEND] {}", &text[..text.len().min(500)]));
                w.send(Message::Text(text.into())).await.map_err(|e| {
                    self.connected.store(false, Ordering::Relaxed);
                    format!("Send failed (connection lost?): {}", e)
                })?;
            } else {
                return Err("Not connected to Wwise".into());
            }
        }

        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("Response channel closed unexpectedly".into()),
            Err(_) => Err(format!("WAAPI call '{}' timed out after 30s", uri)),
        }
    }

    /// 发一条已构造好的 WAMP 报文并等待读循环回填结果。
    async fn send_and_wait(
        &self,
        request_id: u64,
        msg: Value,
        label: &str,
    ) -> Result<Value, String> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(request_id, tx);

        {
            let mut writer_guard = self.writer.lock().await;
            if let Some(ref mut w) = *writer_guard {
                let text = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
                crate::llm::debug_log(&format!("[WAAPI-SEND] {}", &text[..text.len().min(500)]));
                w.send(Message::Text(text.into())).await.map_err(|e| {
                    self.connected.store(false, Ordering::Relaxed);
                    format!("Send failed (connection lost?): {}", e)
                })?;
            } else {
                return Err("Not connected to Wwise".into());
            }
        }

        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("Response channel closed unexpectedly".into()),
            Err(_) => Err(format!("{} timed out after 30s", label)),
        }
    }

    /// 订阅一个 WAAPI topic。事件会进入内部环形缓冲，用 drain_events 取出。
    /// options 可带 `return` 字段来指定每个事件要携带的对象属性。
    pub async fn subscribe(&self, topic: &str, options: Option<Value>) -> Result<u64, String> {
        if !self.is_connected() {
            return Err("Not connected to Wwise.".into());
        }
        if !self.has_wamp_session() {
            return Err(
                "No WAMP session: Wwise rejects SUBSCRIBE before HELLO. Reconnect to Wwise \
                 (wwise_connect) to redo the handshake, then subscribe again."
                    .into(),
            );
        }

        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let opts = options.unwrap_or(json!({}));
        // WAMP SUBSCRIBE: [32, requestId, options, topic]
        let msg = json!([WAMP_SUBSCRIBE, id, opts, topic]);

        // 先登记 topic，读循环收到 SUBSCRIBED 时才能把订阅建起来
        self.pending_subscribes
            .lock()
            .await
            .insert(id, topic.to_string());

        let result = self
            .send_and_wait(id, msg, &format!("Subscribe to '{}'", topic))
            .await;

        if result.is_err() {
            self.pending_subscribes.lock().await.remove(&id);
        }

        let value = result?;
        value
            .get("subscriptionId")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("Subscribe to '{}' returned no subscription id", topic))
    }

    pub async fn unsubscribe(&self, subscription_id: u64) -> Result<(), String> {
        if !self.is_connected() {
            return Err("Not connected to Wwise.".into());
        }

        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        // WAMP UNSUBSCRIBE: [34, requestId, subscriptionId]
        let msg = json!([WAMP_UNSUBSCRIBE, id, subscription_id]);

        self.send_and_wait(id, msg, &format!("Unsubscribe {}", subscription_id))
            .await?;
        self.subscriptions.lock().await.remove(&subscription_id);
        Ok(())
    }

    pub async fn list_subscriptions(&self) -> Vec<SubscriptionInfo> {
        let subs = self.subscriptions.lock().await;
        let mut list: Vec<SubscriptionInfo> = subs
            .iter()
            .map(|(id, state)| SubscriptionInfo {
                subscription_id: *id,
                topic: state.topic.clone(),
                buffered: state.buffer.len(),
                total_received: state.total_received,
                dropped: state.dropped,
            })
            .collect();
        list.sort_by_key(|info| info.subscription_id);
        list
    }

    /// 找出某个 topic 已有的订阅 id（同 topic 多次订阅时返回最早那个）。
    pub async fn find_subscription_by_topic(&self, topic: &str) -> Option<u64> {
        let subs = self.subscriptions.lock().await;
        let mut ids: Vec<u64> = subs
            .iter()
            .filter(|(_, state)| state.topic == topic)
            .map(|(id, _)| *id)
            .collect();
        ids.sort();
        ids.first().copied()
    }

    /// 取出缓冲的事件。subscription_id 为 None 时跨所有订阅取。
    /// consume=true 表示取走（后续不再返回），false 表示只偷看。
    pub async fn drain_events(
        &self,
        subscription_id: Option<u64>,
        max_events: usize,
        consume: bool,
    ) -> Vec<Value> {
        let mut subs = self.subscriptions.lock().await;
        let mut collected: Vec<Value> = Vec::new();

        let mut ids: Vec<u64> = subs.keys().copied().collect();
        ids.sort();

        for id in ids {
            if collected.len() >= max_events {
                break;
            }
            if let Some(target) = subscription_id {
                if id != target {
                    continue;
                }
            }
            if let Some(state) = subs.get_mut(&id) {
                let room = max_events - collected.len();
                if consume {
                    let take = state.buffer.len().min(room);
                    collected.extend(state.buffer.drain(..take));
                } else {
                    collected.extend(state.buffer.iter().take(room).cloned());
                }
            }
        }

        // 跨订阅时按事件到达顺序排，读起来才符合直觉
        collected.sort_by_key(|event| {
            event
                .get("received_at")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        });
        collected
    }

    /// 阻塞等待某个订阅的下一条事件。超时返回 Ok(None)。
    /// 轮询实现：每 100ms 看一次缓冲，不在等待期间持有订阅锁。
    pub async fn wait_for_event(
        &self,
        subscription_id: u64,
        timeout_ms: u64,
    ) -> Result<Option<Value>, String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);

        loop {
            {
                let mut subs = self.subscriptions.lock().await;
                let state = subs
                    .get_mut(&subscription_id)
                    .ok_or_else(|| format!("No active subscription with id {}", subscription_id))?;
                if let Some(event) = state.buffer.pop_front() {
                    return Ok(Some(event));
                }
            }

            if std::time::Instant::now() >= deadline {
                return Ok(None);
            }
            if !self.is_connected() {
                return Err("Connection to Wwise was lost while waiting for an event.".into());
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// 仅供测试：把客户端标记为已连接，用于验证等待/超时路径。
    #[cfg(test)]
    fn mark_connected_for_test(&self) {
        self.connected.store(true, Ordering::Relaxed);
    }

    /// 仅供测试：直接塞入一个订阅及若干事件，免去真连 Wwise。
    #[cfg(test)]
    async fn inject_subscription(&self, subscription_id: u64, topic: &str, event_count: usize) {
        let mut state = SubscriptionState::new(topic.to_string());
        for i in 0..event_count {
            state.record(subscription_id, json!({ "index": i }));
        }
        self.subscriptions
            .lock()
            .await
            .insert(subscription_id, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_events_carry_topic_and_increasing_sequence() {
        let mut state = SubscriptionState::new("ak.wwise.core.object.created".to_string());
        state.record(7, json!({ "name": "A" }));
        state.record(7, json!({ "name": "B" }));

        assert_eq!(state.total_received, 2);
        assert_eq!(state.dropped, 0);
        assert_eq!(state.buffer.len(), 2);

        let first = &state.buffer[0];
        assert_eq!(first["topic"], "ak.wwise.core.object.created");
        assert_eq!(first["subscription_id"], 7);
        assert_eq!(first["sequence"], 1);
        assert_eq!(first["payload"]["name"], "A");
        assert_eq!(state.buffer[1]["sequence"], 2);
    }

    #[test]
    fn buffer_drops_oldest_events_once_full() {
        let mut state = SubscriptionState::new("chatty".to_string());
        for i in 0..(MAX_BUFFERED_EVENTS + 10) {
            state.record(1, json!({ "index": i }));
        }

        assert_eq!(state.buffer.len(), MAX_BUFFERED_EVENTS);
        assert_eq!(state.total_received as usize, MAX_BUFFERED_EVENTS + 10);
        assert_eq!(state.dropped, 10);
        // 最旧的 10 条被丢掉，缓冲里第一条应是 index 10
        assert_eq!(state.buffer[0]["payload"]["index"], 10);
    }

    #[test]
    fn subscription_info_serializes_all_counters() {
        let info = SubscriptionInfo {
            subscription_id: 3,
            topic: "t".to_string(),
            buffered: 2,
            total_received: 9,
            dropped: 1,
        };
        let json = info.to_json();
        assert_eq!(json["subscription_id"], 3);
        assert_eq!(json["topic"], "t");
        assert_eq!(json["buffered"], 2);
        assert_eq!(json["total_received"], 9);
        assert_eq!(json["dropped"], 1);
    }

    #[tokio::test]
    async fn draining_consumes_events_so_they_are_not_seen_twice() {
        let client = WaapiClient::new();
        client.inject_subscription(1, "topic.a", 3).await;

        let first = client.drain_events(None, 10, true).await;
        assert_eq!(first.len(), 3);

        let second = client.drain_events(None, 10, true).await;
        assert!(second.is_empty(), "consumed events must not reappear");
    }

    #[tokio::test]
    async fn peeking_leaves_events_in_the_buffer() {
        let client = WaapiClient::new();
        client.inject_subscription(1, "topic.a", 2).await;

        let peeked = client.drain_events(None, 10, false).await;
        assert_eq!(peeked.len(), 2);

        let again = client.drain_events(None, 10, false).await;
        assert_eq!(again.len(), 2, "peeking must not consume");
    }

    #[tokio::test]
    async fn draining_respects_max_events_and_subscription_filter() {
        let client = WaapiClient::new();
        client.inject_subscription(1, "topic.a", 5).await;
        client.inject_subscription(2, "topic.b", 5).await;

        let limited = client.drain_events(None, 3, false).await;
        assert_eq!(limited.len(), 3);

        let only_b = client.drain_events(Some(2), 10, false).await;
        assert_eq!(only_b.len(), 5);
        assert!(only_b
            .iter()
            .all(|event| event["topic"] == "topic.b"));
    }

    #[tokio::test]
    async fn subscriptions_are_listed_sorted_with_counters() {
        let client = WaapiClient::new();
        client.inject_subscription(5, "topic.late", 1).await;
        client.inject_subscription(2, "topic.early", 4).await;

        let list = client.list_subscriptions().await;
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].subscription_id, 2);
        assert_eq!(list[0].topic, "topic.early");
        assert_eq!(list[0].buffered, 4);
        assert_eq!(list[1].subscription_id, 5);
    }

    #[tokio::test]
    async fn topic_lookup_returns_the_earliest_matching_subscription() {
        let client = WaapiClient::new();
        client.inject_subscription(9, "topic.dup", 0).await;
        client.inject_subscription(4, "topic.dup", 0).await;

        assert_eq!(client.find_subscription_by_topic("topic.dup").await, Some(4));
        assert_eq!(client.find_subscription_by_topic("topic.missing").await, None);
    }

    #[tokio::test]
    async fn waiting_on_an_unknown_subscription_is_an_error() {
        let client = WaapiClient::new();
        let err = client
            .wait_for_event(42, 200)
            .await
            .expect_err("waiting on a non-existent subscription must fail");
        assert!(err.contains("42"));
    }

    #[tokio::test]
    async fn waiting_returns_a_buffered_event_immediately() {
        let client = WaapiClient::new();
        client.inject_subscription(1, "topic.a", 1).await;

        let event = client
            .wait_for_event(1, 200)
            .await
            .expect("wait should succeed")
            .expect("a buffered event should be returned");
        assert_eq!(event["topic"], "topic.a");
        assert_eq!(event["payload"]["index"], 0);
    }

    #[tokio::test]
    async fn waiting_times_out_to_none_when_nothing_arrives() {
        let client = WaapiClient::new();
        client.mark_connected_for_test();
        client.inject_subscription(1, "topic.quiet", 0).await;

        let started = std::time::Instant::now();
        let result = client.wait_for_event(1, 250).await.expect("wait should succeed");

        assert!(result.is_none(), "expected a timeout, got an event");
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(250),
            "wait returned before the timeout elapsed"
        );
    }

    /// 等待过程中连接断开必须立刻报错，而不是静静等到超时。
    #[tokio::test]
    async fn waiting_fails_fast_when_the_connection_is_gone() {
        let client = WaapiClient::new();
        client.inject_subscription(1, "topic.quiet", 0).await;

        let err = client
            .wait_for_event(1, 30_000)
            .await
            .expect_err("a lost connection must surface as an error");
        assert!(err.to_lowercase().contains("connection"));
    }

    #[tokio::test]
    async fn fresh_client_reports_no_wamp_session() {
        let client = WaapiClient::new();
        assert!(!client.has_wamp_session());
        assert!(!client.is_connected());
    }

    #[tokio::test]
    async fn subscribing_without_a_connection_is_rejected() {
        let client = WaapiClient::new();
        let err = client
            .subscribe("ak.wwise.core.object.created", None)
            .await
            .expect_err("subscribe must fail without a connection");
        assert!(err.contains("Not connected"));
    }
}
