//! Shared committed chat events for website streams. Delivery always resolves
//! current session, channel policy, participants, and sender visibility.
use crate::{ApiError, ApiResult, AppState, auth, chat, website_chat};
use axum::{
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgListener};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, broadcast},
    task::JoinHandle,
    time::{Instant, MissedTickBehavior},
};

const EVENT_BUFFER: usize = 256;
const TOTAL_CONNECTIONS: usize = 512;
const USER_CONNECTIONS: usize = 4;
const MAX_INCOMING: usize = 2048;
const MAX_OUTGOING: usize = 65_536;
const SEND_TIMEOUT: Duration = Duration::from_secs(2);
const AUTH_INTERVAL: Duration = Duration::from_secs(1);
const REFRESH_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Message {
        id: i64,
        channel: i64,
    },
    Refresh,
    /// A notification gap requires REST recovery before receiving more events.
    Reset,
}

struct Listener {
    events: broadcast::Sender<Event>,
    healthy: Arc<AtomicBool>,
    task: JoinHandle<()>,
}

pub struct ChatRuntime {
    listener: tokio::sync::Mutex<Option<Listener>>,
    slots: Arc<Semaphore>,
    users: Arc<Mutex<HashMap<i64, usize>>>,
}

impl Default for ChatRuntime {
    fn default() -> Self {
        Self {
            listener: tokio::sync::Mutex::new(None),
            slots: Arc::new(Semaphore::new(TOTAL_CONNECTIONS)),
            users: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl ChatRuntime {
    /// Start one listener lazily and subscribe before the upgrade can send ready.
    /// The mutex also makes concurrent first subscribers share initialization.
    pub async fn subscribe(&self, db: &PgPool) -> ApiResult<broadcast::Receiver<Event>> {
        let mut current = self.listener.lock().await;
        if let Some(listener) = current.as_ref()
            && listener.healthy.load(Ordering::Acquire)
            && !listener.task.is_finished()
        {
            let receiver = listener.events.subscribe();
            // Subscribe before checking again: a loss during the check either
            // reaches this receiver or forces creation of a fresh generation.
            if listener.healthy.load(Ordering::Acquire) && !listener.task.is_finished() {
                return Ok(receiver);
            }
        }
        if let Some(listener) = current.take() {
            let _ = listener.events.send(Event::Reset);
            listener.task.abort();
        }

        let (events, receiver) = broadcast::channel(EVENT_BUFFER);
        let mut listener = PgListener::connect_with(db).await?;
        // Read the schema on the listener's own connection, including any
        // connection setup configured on an isolated fixture's pool.
        let schema: String = sqlx::query_scalar("SELECT current_schema()")
            .fetch_one(&mut listener)
            .await?;
        listener.listen("illya_chat").await?;
        // Surface a gap immediately; transparently reconnecting would hide
        // committed events that arrived while the connection was unavailable.
        listener.eager_reconnect(false);
        let healthy = Arc::new(AtomicBool::new(true));
        let listener_health = healthy.clone();
        let sender = events.clone();
        let task = tokio::spawn(async move {
            loop {
                match listener.try_recv().await {
                    Ok(Some(notification)) => {
                        if let Some(event) = committed_event(notification.payload(), &schema) {
                            let _ = sender.send(event);
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        tracing::warn!(%error, "website chat listener disconnected");
                        break;
                    }
                }
            }
            listener_health.store(false, Ordering::Release);
            let _ = sender.send(Event::Reset);
        });
        *current = Some(Listener {
            events,
            healthy,
            task,
        });
        Ok(receiver)
    }

    pub fn reserve(&self, user: i64) -> ApiResult<Permit> {
        let total = self.slots.clone().try_acquire_owned().map_err(|_| {
            ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "chat_stream_capacity_reached",
                "Chat connections are busy. Please try again shortly.".into(),
            )
        })?;
        let mut users = self.users.lock().map_err(ApiError::internal)?;
        let count = users.entry(user).or_default();
        if *count >= USER_CONNECTIONS {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_chat_connections",
                "Close another chat connection before opening this one.".into(),
            ));
        }
        *count += 1;
        Ok(Permit {
            user,
            users: self.users.clone(),
            _total: total,
        })
    }
}

impl Drop for ChatRuntime {
    fn drop(&mut self) {
        if let Some(listener) = self.listener.get_mut().take() {
            let _ = listener.events.send(Event::Reset);
            listener.task.abort();
        }
    }
}

pub struct Permit {
    user: i64,
    users: Arc<Mutex<HashMap<i64, usize>>>,
    _total: OwnedSemaphorePermit,
}

impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut users) = self.users.lock()
            && let Some(count) = users.get_mut(&self.user)
        {
            *count -= 1;
            if *count == 0 {
                users.remove(&self.user);
            }
        }
    }
}

fn committed_event(payload: &str, schema: &str) -> Option<Event> {
    let event: Value = serde_json::from_str(payload).ok()?;
    if event["schema"].as_str() != Some(schema) {
        return None;
    }
    Some(match event["kind"].as_str()? {
        "message" => match (event["id"].as_i64(), event["channel"].as_i64()) {
            (Some(id), Some(channel)) if id > 0 && channel > 0 => Event::Message { id, channel },
            _ => Event::Reset,
        },
        "resync" => Event::Reset,
        _ => Event::Refresh,
    })
}

pub async fn stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    upgrade(state, headers, None, ws).await
}

pub async fn channel_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    upgrade(state, headers, Some(id), ws).await
}

async fn upgrade(
    state: AppState,
    headers: HeaderMap,
    channel: Option<i64>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    let user = auth::require(&state, &headers).await?;
    if let Some(id) = channel {
        chat::can_read(&state, &headers, id).await?;
    }
    let permit = state.chat_runtime.reserve(user.id)?;
    let events = state.chat_runtime.subscribe(&state.db).await?;
    Ok(ws
        .max_frame_size(MAX_INCOMING)
        .max_message_size(MAX_INCOMING)
        .write_buffer_size(4096)
        .max_write_buffer_size(MAX_OUTGOING * 2)
        .on_upgrade(move |socket| connection(socket, state, headers, channel, events, permit)))
}

async fn send(socket: &mut WebSocket, message: Message) -> Result<(), ()> {
    tokio::time::timeout(SEND_TIMEOUT, socket.send(message))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}

async fn emit(socket: &mut WebSocket, value: Value) -> Result<(), ()> {
    let text = value.to_string();
    if text.len() > MAX_OUTGOING {
        return Err(());
    }
    send(socket, Message::Text(text.into())).await
}

async fn connection(
    mut socket: WebSocket,
    state: AppState,
    headers: HeaderMap,
    channel: Option<i64>,
    mut events: broadcast::Receiver<Event>,
    _permit: Permit,
) {
    // A session or channel policy may change while the upgrade is pending.
    if auth::require(&state, &headers).await.is_err()
        || (if let Some(id) = channel {
            chat::can_read(&state, &headers, id).await.is_err()
        } else {
            false
        })
    {
        let _ = send(&mut socket, Message::Close(None)).await;
        return;
    }
    if channel.is_none() && emit(&mut socket, json!({"type":"ready"})).await.is_err() {
        return;
    }

    let mut auth_timer = tokio::time::interval_at(Instant::now() + AUTH_INTERVAL, AUTH_INTERVAL);
    auth_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut refresh_timer =
        tokio::time::interval_at(Instant::now() + REFRESH_INTERVAL, REFRESH_INTERVAL);
    refresh_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Ping(payload))) => {
                    if send(&mut socket, Message::Pong(payload)).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Pong(_) | Message::Text(_) | Message::Binary(_))) => {}
                _ => break,
            },
            event = events.recv() => {
                // Lag, a closed generation, or listener loss requires recovery.
                let Ok(event) = event else { break; };
                if event == Event::Reset { break; }
                let Ok(user) = auth::require(&state, &headers).await else { break; };
                if let Some(id) = channel
                    && chat::can_read(&state, &headers, id).await.is_err()
                {
                    break;
                }
                match event {
                    Event::Message { id, channel: message_channel } => {
                        if channel.is_some_and(|id| id != message_channel) {
                            continue;
                        }
                        match website_chat::visible_message(&state, &user, id).await {
                            Ok(Some(message)) => {
                                if emit(&mut socket, json!({"type":"messages","items":[message]})).await.is_err() {
                                    break;
                                }
                            }
                            Ok(None) => {}
                            Err(_) => break,
                        }
                    }
                    Event::Refresh if channel.is_none()
                        && emit(&mut socket, json!({"type":"refresh"})).await.is_err() => break,
                    _ => {}
                }
            },
            _ = auth_timer.tick() => {
                if auth::require(&state, &headers).await.is_err() {
                    break;
                }
                if let Some(id) = channel
                    && chat::can_read(&state, &headers, id).await.is_err()
                {
                    break;
                }
            },
            // Commit-triggered refreshes never move this periodic deadline.
            _ = refresh_timer.tick(), if channel.is_none() => {
                if auth::require(&state, &headers).await.is_err()
                    || emit(&mut socket, json!({"type":"refresh"})).await.is_err()
                {
                    break;
                }
            }
        }
    }
    let _ = send(&mut socket, Message::Close(None)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_notifications_are_scoped_to_the_listener_schema() {
        let message = json!({"schema":"website_fixture","kind":"message","id":7,"channel":3});
        assert_eq!(
            committed_event(&message.to_string(), "website_fixture"),
            Some(Event::Message { id: 7, channel: 3 })
        );
        assert_eq!(committed_event(&message.to_string(), "other_fixture"), None);
        assert_eq!(
            committed_event(
                &json!({"schema":"other_fixture","kind":"resync"}).to_string(),
                "website_fixture"
            ),
            None
        );
        for payload in ["not json", "[]", r#"{"kind":"message","id":7,"channel":3}"#] {
            assert_eq!(committed_event(payload, "website_fixture"), None);
        }
    }

    #[test]
    fn unusable_message_ids_require_recovery_instead_of_delivery() {
        for field in ["id", "channel"] {
            for invalid in [
                Value::Null,
                json!(0),
                json!(-1),
                json!("7"),
                json!(false),
                json!(1.5),
                json!(u64::MAX),
            ] {
                let mut message =
                    json!({"schema":"website_fixture","kind":"message","id":7,"channel":3});
                message[field] = invalid;
                assert_eq!(
                    committed_event(&message.to_string(), "website_fixture"),
                    Some(Event::Reset),
                    "invalid {field} must never be delivered: {message}"
                );
            }
        }
    }

    #[test]
    fn policy_notifications_refresh_without_exposing_their_payloads() {
        for kind in ["join", "part", "read", "channel_policy_changed"] {
            let event = json!({
                "schema":"website_fixture", "kind":kind,
                "user":29, "channel":8, "private":"not sent to a client"
            });
            assert_eq!(
                committed_event(&event.to_string(), "website_fixture"),
                Some(Event::Refresh)
            );
        }
        assert_eq!(
            committed_event(
                &json!({"schema":"website_fixture","kind":"resync"}).to_string(),
                "website_fixture"
            ),
            Some(Event::Reset)
        );
    }

    #[test]
    fn per_user_rejections_and_dropped_permits_release_capacity() {
        let runtime = ChatRuntime::default();
        let mut permits = (0..4)
            .map(|_| runtime.reserve(7).expect("four connections are allowed"))
            .collect::<Vec<_>>();
        for _ in 0..8 {
            let Err(error) = runtime.reserve(7) else {
                panic!("the fifth connection for one user must be rejected");
            };
            assert_eq!(error.0, StatusCode::TOO_MANY_REQUESTS);
            assert_eq!(error.1, "too_many_chat_connections");
        }
        drop(permits.pop());
        permits.push(runtime.reserve(7).expect("a dropped user slot is reusable"));
        drop(permits);
        assert!(runtime.users.lock().unwrap().is_empty());

        // Failed per-user reservations must not consume global slots.
        let all_slots = (1..=512)
            .map(|user| runtime.reserve(user).expect("all global slots remain free"))
            .collect::<Vec<_>>();
        drop(all_slots);
        assert!(runtime.users.lock().unwrap().is_empty());
    }

    #[test]
    fn global_limit_is_shared_across_users_and_recovers_after_disconnect() {
        let runtime = ChatRuntime::default();
        let mut permits = (0..512)
            .map(|index| {
                runtime
                    .reserve(index / 4 + 1)
                    .expect("512 slots are allowed")
            })
            .collect::<Vec<_>>();
        let Err(error) = runtime.reserve(999) else {
            panic!("the 513th connection must be rejected across all users");
        };
        assert_eq!(error.0, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.1, "chat_stream_capacity_reached");

        drop(permits.pop());
        let replacement = runtime.reserve(128).expect("both limits recover on drop");
        let Err(error) = runtime.reserve(999) else {
            panic!("the replacement must consume the recovered global slot");
        };
        assert_eq!(error.1, "chat_stream_capacity_reached");
        drop(replacement);
        drop(permits);
        assert!(runtime.users.lock().unwrap().is_empty());
        assert!(runtime.reserve(128).is_ok());
    }
}
