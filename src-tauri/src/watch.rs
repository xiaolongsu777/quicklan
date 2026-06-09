use crate::{discovery::DiscoveryService, settings::SettingsService, storage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    io::Write,
    net::TcpStream as StdTcpStream,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl,
    WebviewWindowBuilder,
};
use url::Url;
use uuid::Uuid;

const JOIN_TIMEOUT_SECS: u64 = 5;
const VIDEO_DETECT_TIMEOUT_SECS: u64 = 30;
const POLL_INTERVAL_MS: u64 = 500;
const HEARTBEAT_INTERVAL_SECS: u64 = 10;
const SEEK_THRESHOLD_SECS: f64 = 1.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WatchRoomStatus {
    Active,
    Ended,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRoom {
    pub room_id: String,
    pub host_user_id: String,
    pub host_name: String,
    pub title: String,
    pub url: Option<String>,
    pub has_video_url: bool,
    pub is_private: bool,
    pub password_hash: Option<String>,
    pub member_count: usize,
    pub status: WatchRoomStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWatchRoomInput {
    pub title: String,
    pub is_private: bool,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRoomSession {
    pub room: WatchRoom,
    pub is_host: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchJoinResult {
    pub room_id: String,
    pub target_user_id: String,
    pub accepted: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRoomChatMessage {
    pub message_id: String,
    pub room_id: String,
    pub sender_user_id: String,
    pub sender_name: String,
    pub body: String,
    pub created_at: i64,
    pub system: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRoomVideoStatus {
    pub room_id: String,
    pub phase: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchContentBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchSyncMessage {
    pub room_id: String,
    pub host_user_id: String,
    pub action: String,
    pub time: f64,
    pub playback_rate: f64,
    pub is_playing: bool,
    pub sent_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRoomCreateMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub room: WatchRoom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRoomStateMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub room: WatchRoom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRoomJoinRequest {
    #[serde(rename = "type")]
    pub message_type: String,
    pub room_id: String,
    pub user_id: String,
    pub nickname: String,
    pub password_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRoomLeaveMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub room_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRoomEndMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub room_id: String,
    pub host_user_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRoomUrlUpdateMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub room_id: String,
    pub host_user_id: String,
    pub url: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRoomChatEnvelope {
    #[serde(rename = "type")]
    pub message_type: String,
    pub message: WatchRoomChatMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchSyncEnvelope {
    #[serde(rename = "type")]
    pub message_type: String,
    pub room_id: String,
    pub host_user_id: String,
    pub action: String,
    pub time: f64,
    pub playback_rate: f64,
    pub is_playing: bool,
    pub sent_at: i64,
}

#[derive(Clone)]
pub struct WatchRoomService {
    app: AppHandle,
    discovery: Arc<Mutex<Option<DiscoveryService>>>,
    settings: SettingsService,
    local_user_id: String,
    inner: Arc<Mutex<WatchState>>,
}

struct WatchState {
    rooms: HashMap<String, RoomEntry>,
    sessions: HashMap<String, SessionEntry>,
    pending_joins: HashMap<String, mpsc::Sender<WatchJoinResult>>,
}

struct RoomEntry {
    room: WatchRoom,
    members: HashSet<String>,
    messages: Vec<WatchRoomChatMessage>,
}

struct SessionEntry {
    room_id: String,
    window_label: String,
    content_webview_label: String,
    is_host: bool,
    stop: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VideoSnapshot {
    found: bool,
    current_time: f64,
    playback_rate: f64,
    is_playing: bool,
}

impl WatchRoomService {
    pub fn new(app: AppHandle, settings: SettingsService, local_user_id: String) -> Self {
        Self {
            app,
            discovery: Arc::new(Mutex::new(None)),
            settings,
            local_user_id,
            inner: Arc::new(Mutex::new(WatchState {
                rooms: HashMap::new(),
                sessions: HashMap::new(),
                pending_joins: HashMap::new(),
            })),
        }
    }

    pub fn set_discovery(&self, discovery: DiscoveryService) {
        if let Ok(mut slot) = self.discovery.lock() {
            *slot = Some(discovery);
        }
    }

    pub fn list_rooms(&self) -> Vec<WatchRoom> {
        let mut rooms = self
            .inner
            .lock()
            .map(|state| {
                state
                    .rooms
                    .values()
                    .map(|entry| {
                        let mut room = entry.room.clone();
                        if room.host_user_id != self.local_user_id
                            && !matches!(room.status, WatchRoomStatus::Ended)
                            && self
                                .discovery()
                                .and_then(|discovery| discovery.find_device(&room.host_user_id))
                                .is_none()
                        {
                            room.status = WatchRoomStatus::Ended;
                        }
                        room
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        rooms.retain(|room| !matches!(room.status, WatchRoomStatus::Ended));
        rooms.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        rooms
    }

    pub fn get_session(&self, room_id: &str) -> Option<WatchRoomSession> {
        self.inner.lock().ok().and_then(|state| {
            let session = state.sessions.get(room_id)?;
            let room = state.rooms.get(room_id)?.room.clone();
            Some(WatchRoomSession {
                room,
                is_host: session.is_host,
            })
        })
    }

    pub fn open_room_window(&self, room_id: &str) -> Result<(), String> {
        let room = {
            let state = self.lock()?;
            state
                .rooms
                .get(room_id)
                .map(|entry| entry.room.clone())
                .ok_or_else(|| "观影房间不存在".to_string())?
        };
        if matches!(room.status, WatchRoomStatus::Ended) {
            return Err("观影房间已结束".to_string());
        }

        let label = watch_window_label(room_id);
        if let Some(window) = self.app.get_webview_window(&label) {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            return Ok(());
        }

        let url = format!("index.html?mode=watch-room&roomId={}", room.room_id);
        WebviewWindowBuilder::new(&self.app, label, WebviewUrl::App(url.into()))
            .title(format!("QuickLAN 观影 - {}", room.title))
            .inner_size(1320.0, 860.0)
            .min_inner_size(980.0, 660.0)
            .focused(true)
            .build()
            .map_err(|err| format!("独立观影窗口创建失败：{err}"))?;
        Ok(())
    }

    pub fn open_content_webview(
        &self,
        window_label: &str,
        room_id: &str,
        url: &str,
        bounds: WatchContentBounds,
    ) -> Result<(), String> {
        let parsed_url = Url::parse(url).map_err(|_| "视频网页链接无效".to_string())?;
        let label = watch_content_webview_label(room_id);
        if self.app.get_webview(&label).is_some() {
            self.move_content_webview(room_id, bounds)?;
            return Ok(());
        }
        let window = self
            .app
            .get_webview_window(window_label)
            .or_else(|| self.app.get_webview_window("main"))
            .ok_or_else(|| "未找到承载观影内容的窗口".to_string())?;
        let data_directory = quicklan_watch_profile_dir()?;
        let builder = WebviewBuilder::new(&label, WebviewUrl::External(parsed_url))
            .data_directory(data_directory);
        window
            .as_ref()
            .window()
            .add_child(
                builder,
                LogicalPosition::new(bounds.x, bounds.y),
                LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0)),
            )
            .map_err(|err| format!("创建观影网页 WebView 失败: {err}"))?;
        Ok(())
    }

    pub fn move_content_webview(
        &self,
        room_id: &str,
        bounds: WatchContentBounds,
    ) -> Result<(), String> {
        let Some(webview) = self.app.get_webview(&watch_content_webview_label(room_id)) else {
            return Ok(());
        };
        webview
            .set_position(LogicalPosition::new(bounds.x, bounds.y))
            .map_err(|err| format!("移动观影网页 WebView 失败: {err}"))?;
        webview
            .set_size(LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0)))
            .map_err(|err| format!("调整观影网页 WebView 大小失败: {err}"))
    }

    pub fn hide_content_webview(&self, room_id: &str) -> Result<(), String> {
        self.move_content_webview(
            room_id,
            WatchContentBounds {
                x: -10000.0,
                y: -10000.0,
                width: 1.0,
                height: 1.0,
            },
        )
    }

    pub fn close_content_webview(&self, room_id: &str) -> Result<(), String> {
        if let Some(webview) = self.app.get_webview(&watch_content_webview_label(room_id)) {
            webview
                .close()
                .map_err(|err| format!("关闭观影网页 WebView 失败: {err}"))?;
        }
        Ok(())
    }

    pub fn list_messages(&self, room_id: &str) -> Result<Vec<WatchRoomChatMessage>, String> {
        let state = self.lock()?;
        let entry = state
            .rooms
            .get(room_id)
            .ok_or_else(|| "观影房间不存在".to_string())?;
        Ok(entry.messages.clone())
    }

    pub fn create_room(&self, input: CreateWatchRoomInput) -> Result<WatchRoomSession, String> {
        let title = normalize_title(&input.title, &self.settings.nickname());
        let password_hash = if input.is_private {
            Some(hash_required_password(input.password.as_deref())?)
        } else {
            None
        };
        let now = now_secs();
        let room = WatchRoom {
            room_id: Uuid::new_v4().to_string(),
            host_user_id: self.local_user_id.clone(),
            host_name: self.settings.nickname(),
            title,
            url: None,
            has_video_url: false,
            is_private: input.is_private,
            password_hash,
            member_count: 1,
            status: WatchRoomStatus::Active,
            created_at: now,
            updated_at: now,
        };
        {
            let mut state = self.lock()?;
            let mut members = HashSet::new();
            members.insert(self.local_user_id.clone());
            state.rooms.insert(
                room.room_id.clone(),
                RoomEntry {
                    room: room.clone(),
                    members,
                    messages: vec![system_message(
                        &room.room_id,
                        "系统",
                        "房间已创建，房主可以在房间内随时设置视频链接。",
                    )],
                },
            );
        }
        let session = self.open_session(&room.room_id, true)?;
        self.broadcast_create(&room);
        self.emit_room_updated(&room);
        Ok(session)
    }

    pub fn update_room_url(&self, room_id: &str, url: &str) -> Result<WatchRoomSession, String> {
        let url = validate_watch_url(url)?;
        let room = {
            let mut state = self.lock()?;
            let entry = state
                .rooms
                .get_mut(room_id)
                .ok_or_else(|| "观影房间不存在".to_string())?;
            if entry.room.host_user_id != self.local_user_id {
                return Err("只有房主可以更新视频链接".to_string());
            }
            if matches!(entry.room.status, WatchRoomStatus::Ended) {
                return Err("观影房间已结束".to_string());
            }
            entry.room.url = Some(url.clone());
            entry.room.has_video_url = true;
            entry.room.updated_at = now_secs();
            entry.room.clone()
        };
        let status = WatchRoomUrlUpdateMessage {
            message_type: "watch_room_url_update".to_string(),
            room_id: room.room_id.clone(),
            host_user_id: room.host_user_id.clone(),
            url,
            updated_at: room.updated_at,
        };
        self.broadcast_url_update(&status);
        self.broadcast_state(&room);
        self.emit_room_updated(&room);
        let message = self.append_message(
            room_id,
            system_message(
                room_id,
                "系统",
                "房主更新了视频链接，房间将自动切换到新页面。",
            ),
        )?;
        self.broadcast_chat_message(&message);
        self.emit_chat_message(&message);
        self.get_session(room_id)
            .ok_or_else(|| "观影窗口未打开".to_string())
    }

    pub fn join_room(
        &self,
        room_id: &str,
        password: Option<String>,
    ) -> Result<WatchRoomSession, String> {
        if let Some(session) = self.get_session(room_id) {
            return Ok(session);
        }

        let room = {
            let state = self.lock()?;
            state
                .rooms
                .get(room_id)
                .map(|entry| entry.room.clone())
                .ok_or_else(|| "观影房间不存在".to_string())?
        };
        if matches!(room.status, WatchRoomStatus::Ended) {
            return Err("观影房间已结束".to_string());
        }
        if room.host_user_id == self.local_user_id {
            return self.open_session(room_id, true);
        }
        let host = self
            .discovery_required()?
            .find_device(&room.host_user_id)
            .ok_or_else(|| "房主当前不在线".to_string())?;
        let provided_hash = match (room.is_private, password.as_deref()) {
            (true, Some(value)) => Some(hash_text(value)),
            (true, None) => return Err("请输入房间密码".to_string()),
            (false, _) => None,
        };
        if room.is_private && provided_hash != room.password_hash {
            return Err("密码错误".to_string());
        }

        let (tx, rx) = mpsc::channel();
        {
            let mut state = self.lock()?;
            state
                .pending_joins
                .insert(join_key(room_id, &self.local_user_id), tx);
        }
        let request = WatchRoomJoinRequest {
            message_type: "watch_room_join_request".to_string(),
            room_id: room_id.to_string(),
            user_id: self.local_user_id.clone(),
            nickname: self.settings.nickname(),
            password_hash: provided_hash,
        };
        self.post_to_device(
            &host.ip,
            host.api_port,
            "/watch/rooms/join-request",
            &serde_json::to_string(&request).map_err(|err| err.to_string())?,
        );
        let result = rx
            .recv_timeout(Duration::from_secs(JOIN_TIMEOUT_SECS))
            .map_err(|_| "加入观影房间超时".to_string())?;
        if !result.accepted {
            return Err(result.reason.unwrap_or_else(|| "加入观影房间失败".to_string()));
        }
        {
            let mut state = self.lock()?;
            if let Some(entry) = state.rooms.get_mut(room_id) {
                entry.members.insert(self.local_user_id.clone());
            }
        }
        let session = self.open_session(room_id, false)?;
        let message = self.append_message(
            room_id,
            system_message(room_id, "系统", &format!("{} 加入了房间。", self.settings.nickname())),
        )?;
        self.emit_chat_message(&message);
        Ok(session)
    }

    pub fn leave_room(&self, room_id: &str) -> Result<(), String> {
        self.leave_room_internal(room_id, true)
    }

    pub fn end_room(&self, room_id: &str, reason: &str) -> Result<(), String> {
        self.end_room_internal(room_id, reason, true)
    }

    pub fn send_room_message(
        &self,
        room_id: &str,
        body: &str,
    ) -> Result<WatchRoomChatMessage, String> {
        let body = body.trim();
        if body.is_empty() {
            return Err("消息不能为空".to_string());
        }
        {
            let state = self.lock()?;
            let entry = state
                .rooms
                .get(room_id)
                .ok_or_else(|| "观影房间不存在".to_string())?;
            if !self.is_local_member(entry) {
                return Err("你尚未加入该观影房间".to_string());
            }
            if matches!(entry.room.status, WatchRoomStatus::Ended) {
                return Err("观影房间已结束".to_string());
            }
        }
        let message = self.append_message(
            room_id,
            WatchRoomChatMessage {
                message_id: Uuid::new_v4().to_string(),
                room_id: room_id.to_string(),
                sender_user_id: self.local_user_id.clone(),
                sender_name: self.settings.nickname(),
                body: body.to_string(),
                created_at: now_secs(),
                system: false,
            },
        )?;
        self.broadcast_chat_message(&message);
        self.emit_chat_message(&message);
        Ok(message)
    }

    pub fn send_sync(&self, message: WatchSyncMessage) -> Result<(), String> {
        let envelope = WatchSyncEnvelope {
            message_type: "watch_sync".to_string(),
            room_id: message.room_id.clone(),
            host_user_id: message.host_user_id.clone(),
            action: message.action.clone(),
            time: message.time,
            playback_rate: message.playback_rate,
            is_playing: message.is_playing,
            sent_at: message.sent_at,
        };
        let body = serde_json::to_string(&envelope).map_err(|err| err.to_string())?;
        if let Ok(discovery) = self.discovery_required() {
            for device in discovery
                .list_devices()
                .into_iter()
                .filter(|device| device.online && device.id != self.local_user_id)
            {
                self.post_to_device(&device.ip, device.api_port, "/watch/rooms/sync", &body);
            }
        }
        Ok(())
    }

    pub fn accept_remote_create(&self, payload: WatchRoomCreateMessage) -> Result<(), String> {
        {
            let mut state = self.lock()?;
            let mut members = HashSet::new();
            members.insert(payload.room.host_user_id.clone());
            let messages = state
                .rooms
                .get(&payload.room.room_id)
                .map(|entry| entry.messages.clone())
                .unwrap_or_default();
            state.rooms.insert(
                payload.room.room_id.clone(),
                RoomEntry {
                    room: payload.room.clone(),
                    members,
                    messages,
                },
            );
        }
        self.emit_room_updated(&payload.room);
        Ok(())
    }

    pub fn accept_remote_state(&self, payload: WatchRoomStateMessage) -> Result<(), String> {
        let mut room = payload.room;
        {
            let mut state = self.lock()?;
            let mut members = state
                .rooms
                .get(&room.room_id)
                .map(|entry| entry.members.clone())
                .unwrap_or_default();
            let messages = state
                .rooms
                .get(&room.room_id)
                .map(|entry| entry.messages.clone())
                .unwrap_or_default();
            if state.sessions.contains_key(&room.room_id) || members.contains(&self.local_user_id) {
                members.insert(self.local_user_id.clone());
            }
            room.has_video_url = room.url.as_ref().is_some_and(|value| !value.is_empty());
            state.rooms.insert(
                room.room_id.clone(),
                RoomEntry {
                    room: room.clone(),
                    members,
                    messages,
                },
            );
        }
        self.emit_room_updated(&room);
        Ok(())
    }

    pub fn accept_join_request(&self, payload: WatchRoomJoinRequest) -> Result<(), String> {
        let room = {
            let state = self.lock()?;
            state
                .rooms
                .get(&payload.room_id)
                .map(|entry| entry.room.clone())
                .ok_or_else(|| "观影房间不存在".to_string())?
        };
        if room.host_user_id != self.local_user_id {
            return Ok(());
        }
        let accepted = if room.is_private {
            room.password_hash == payload.password_hash
        } else {
            true
        };
        let result = WatchJoinResult {
            room_id: payload.room_id.clone(),
            target_user_id: payload.user_id.clone(),
            accepted,
            reason: if accepted {
                None
            } else {
                Some("密码错误".to_string())
            },
        };
        if accepted {
            let updated = {
                let mut state = self.lock()?;
                let entry = state
                    .rooms
                    .get_mut(&payload.room_id)
                    .ok_or_else(|| "观影房间不存在".to_string())?;
                entry.members.insert(payload.user_id.clone());
                entry.room.member_count = entry.members.len();
                entry.room.updated_at = now_secs();
                entry.room.clone()
            };
            self.broadcast_state(&updated);
            self.emit_room_updated(&updated);
            let message = self.append_message(
                &payload.room_id,
                system_message(
                    &payload.room_id,
                    "系统",
                    &format!("{} 加入了房间。", payload.nickname),
                ),
            )?;
            self.broadcast_chat_message(&message);
            self.emit_chat_message(&message);
        }
        if let Some(target) = self
            .discovery()
            .and_then(|discovery| discovery.find_device(&payload.user_id))
        {
            self.post_to_device(
                &target.ip,
                target.api_port,
                "/watch/rooms/join-result",
                &serde_json::to_string(&result).map_err(|err| err.to_string())?,
            );
        }
        Ok(())
    }

    pub fn accept_join_result(&self, result: WatchJoinResult) -> Result<(), String> {
        let sender = {
            let mut state = self.lock()?;
            state
                .pending_joins
                .remove(&join_key(&result.room_id, &result.target_user_id))
        };
        if let Some(sender) = sender {
            let _ = sender.send(result.clone());
        }
        let _ = self.app.emit("watch-room-join-result", result);
        Ok(())
    }

    pub fn accept_leave(&self, payload: WatchRoomLeaveMessage) -> Result<(), String> {
        let updated = {
            let mut state = self.lock()?;
            let entry = state
                .rooms
                .get_mut(&payload.room_id)
                .ok_or_else(|| "观影房间不存在".to_string())?;
            if entry.room.host_user_id != self.local_user_id {
                return Ok(());
            }
            entry.members.remove(&payload.user_id);
            entry.room.member_count = entry.members.len();
            entry.room.updated_at = now_secs();
            entry.room.clone()
        };
        self.broadcast_state(&updated);
        self.emit_room_updated(&updated);
        Ok(())
    }

    pub fn accept_end(&self, payload: WatchRoomEndMessage) -> Result<(), String> {
        let existed = {
            let mut state = self.lock()?;
            state.rooms.remove(&payload.room_id).is_some()
        };
        if !existed {
            return Ok(());
        }
        self.remove_session(&payload.room_id, true);
        self.emit_room_ended(&payload);
        Ok(())
    }

    pub fn accept_url_update(&self, payload: WatchRoomUrlUpdateMessage) -> Result<(), String> {
        let room = {
            let mut state = self.lock()?;
            let entry = state
                .rooms
                .get_mut(&payload.room_id)
                .ok_or_else(|| "观影房间不存在".to_string())?;
            if entry.room.host_user_id != payload.host_user_id {
                return Ok(());
            }
            entry.room.url = Some(payload.url);
            entry.room.has_video_url = true;
            entry.room.updated_at = payload.updated_at;
            entry.room.clone()
        };
        self.emit_room_updated(&room);
        Ok(())
    }

    pub fn accept_chat_message(&self, payload: WatchRoomChatEnvelope) -> Result<(), String> {
        let should_accept = {
            let state = self.lock()?;
            let Some(entry) = state.rooms.get(&payload.message.room_id) else {
                return Ok(());
            };
            self.is_local_member(entry) || state.sessions.contains_key(&payload.message.room_id)
        };
        if !should_accept {
            return Ok(());
        }
        let room_id = payload.message.room_id.clone();
        let message = self.append_message(&room_id, payload.message)?;
        self.emit_chat_message(&message);
        Ok(())
    }

    pub fn accept_sync(&self, payload: WatchSyncEnvelope) -> Result<(), String> {
        let message = WatchSyncMessage {
            room_id: payload.room_id,
            host_user_id: payload.host_user_id,
            action: payload.action,
            time: payload.time,
            playback_rate: payload.playback_rate,
            is_playing: payload.is_playing,
            sent_at: payload.sent_at,
        };
        let session = self
            .inner
            .lock()
            .ok()
            .and_then(|state| state.sessions.get(&message.room_id).cloned());
        let Some(session) = session else {
            return Ok(());
        };
        if session.is_host {
            return Ok(());
        }
        self.apply_sync_to_webview(&session.content_webview_label, &message)
    }

    pub fn handle_window_close_requested(&self, label: &str) {
        let maybe = self.inner.lock().ok().and_then(|state| {
            state
                .sessions
                .values()
                .find(|session| session.window_label == label)
                .map(|session| (session.room_id.clone(), session.is_host))
        });
        let Some((room_id, is_host)) = maybe else {
            return;
        };
        if is_host {
            let _ = self.end_room_internal(&room_id, "host_closed", false);
        } else {
            let _ = self.leave_room_internal(&room_id, false);
        }
    }

    fn open_session(&self, room_id: &str, is_host: bool) -> Result<WatchRoomSession, String> {
        let room = {
            let state = self.lock()?;
            state
                .rooms
                .get(room_id)
                .map(|entry| entry.room.clone())
                .ok_or_else(|| "观影房间不存在".to_string())?
        };
        if matches!(room.status, WatchRoomStatus::Ended) {
            return Err("观影房间已结束".to_string());
        }

        let window_label = "main".to_string();
        let content_webview_label = watch_content_webview_label(&room.room_id);
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut state = self.lock()?;
            if let Some(existing) = state.sessions.insert(
                room.room_id.clone(),
                SessionEntry {
                    room_id: room.room_id.clone(),
                    window_label: window_label.clone(),
                    content_webview_label: content_webview_label.clone(),
                    is_host,
                    stop: stop.clone(),
                },
            ) {
                existing.stop.store(true, Ordering::Relaxed);
            }
        }
        self.spawn_window_loop(
            room.room_id.clone(),
            room.host_user_id.clone(),
            content_webview_label,
            is_host,
            stop,
        );
        Ok(WatchRoomSession { room, is_host })
    }

    fn leave_room_internal(&self, room_id: &str, close_window: bool) -> Result<(), String> {
        let room = {
            let mut state = self.lock()?;
            let entry = state
                .rooms
                .get_mut(room_id)
                .ok_or_else(|| "观影房间不存在".to_string())?;
            if entry.room.host_user_id == self.local_user_id {
                return self.end_room_internal(room_id, "host_closed", close_window);
            }
            entry.members.remove(&self.local_user_id);
            if entry.room.member_count > 0 {
                entry.room.member_count = entry.room.member_count.saturating_sub(1);
            }
            entry.room.updated_at = now_secs();
            entry.room.clone()
        };
        self.remove_session(room_id, close_window);
        let leave = WatchRoomLeaveMessage {
            message_type: "watch_room_leave".to_string(),
            room_id: room_id.to_string(),
            user_id: self.local_user_id.clone(),
        };
        if let Some(host) = self
            .discovery()
            .and_then(|discovery| discovery.find_device(&room.host_user_id))
        {
            self.post_to_device(
                &host.ip,
                host.api_port,
                "/watch/rooms/leave",
                &serde_json::to_string(&leave).map_err(|err| err.to_string())?,
            );
        }
        self.emit_room_updated(&room);
        Ok(())
    }

    fn end_room_internal(
        &self,
        room_id: &str,
        reason: &str,
        close_window: bool,
    ) -> Result<(), String> {
        let room = {
            let mut state = self.lock()?;
            let entry = state
                .rooms
                .get_mut(room_id)
                .ok_or_else(|| "观影房间不存在".to_string())?;
            if entry.room.host_user_id != self.local_user_id {
                return Err("只有房主可以结束房间".to_string());
            }
            entry.room.status = WatchRoomStatus::Ended;
            entry.room.updated_at = now_secs();
            entry.room.clone()
        };
        self.remove_session(room_id, close_window);
        self.broadcast_end(&room, reason);
        let payload = WatchRoomEndMessage {
            message_type: "watch_room_end".to_string(),
            room_id: room.room_id.clone(),
            host_user_id: room.host_user_id.clone(),
            reason: reason.to_string(),
        };
        self.emit_room_ended(&payload);
        if let Ok(mut state) = self.inner.lock() {
            state.rooms.remove(room_id);
        }
        Ok(())
    }

    fn remove_session(&self, room_id: &str, _close_window: bool) {
        let session = self.lock().ok().and_then(|mut state| state.sessions.remove(room_id));
        if let Some(session) = session {
            session.stop.store(true, Ordering::Relaxed);
            if let Some(webview) = self.app.get_webview(&session.content_webview_label) {
                let _ = webview.close();
            }
        }
    }

    fn spawn_window_loop(
        &self,
        room_id: String,
        host_user_id: String,
        content_webview_label: String,
        is_host: bool,
        stop: Arc<AtomicBool>,
    ) {
        let service = self.clone();
        thread::spawn(move || {
            let mut current_url: Option<String> = None;
            let mut detect_deadline: Option<Instant> = None;
            let mut video_ready = false;
            let mut last_snapshot: Option<VideoSnapshot> = None;
            let mut last_poll_at = Instant::now();
            let mut last_heartbeat = Instant::now();
            let mut last_phase = String::new();

            while !stop.load(Ordering::Relaxed) {
                let room = service
                    .inner
                    .lock()
                    .ok()
                    .and_then(|state| state.rooms.get(&room_id).map(|entry| entry.room.clone()));
                let Some(room) = room else {
                    break;
                };

                let next_url = room.url.clone();
                if next_url != current_url {
                    current_url = next_url.clone();
                    detect_deadline = next_url
                        .as_ref()
                        .map(|_| Instant::now() + Duration::from_secs(VIDEO_DETECT_TIMEOUT_SECS));
                    video_ready = false;
                    last_snapshot = None;
                    if next_url.is_none() {
                        service.emit_video_status(
                            &room_id,
                            "empty",
                            "房主尚未设置视频链接，右侧聊天仍可正常使用。",
                            &mut last_phase,
                        );
                    } else {
                        service.emit_video_status(
                            &room_id,
                            "detecting",
                            "正在检测网页播放器…",
                            &mut last_phase,
                        );
                    }
                }

                if current_url.is_none() {
                    thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
                    continue;
                }

                let snapshot = service.eval_video_state(&content_webview_label);
                let found = snapshot.as_ref().is_some_and(|value| value.found);
                if !video_ready {
                    if found {
                        video_ready = true;
                        last_snapshot = snapshot.clone();
                        last_poll_at = Instant::now();
                        last_heartbeat = Instant::now();
                        service.emit_video_status(
                            &room_id,
                            "ready",
                            "已检测到网页播放器，播放同步已就绪。",
                            &mut last_phase,
                        );
                    } else if detect_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                        service.emit_video_status(
                            &room_id,
                            "missing",
                            "当前页面未检测到网页播放器，请手动播放或等待房主更新链接。",
                            &mut last_phase,
                        );
                    }
                    thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
                    continue;
                }

                let Some(snapshot) = snapshot else {
                    video_ready = false;
                    detect_deadline = Some(Instant::now() + Duration::from_secs(VIDEO_DETECT_TIMEOUT_SECS));
                    service.emit_video_status(
                        &room_id,
                        "detecting",
                        "正在重新连接网页播放器…",
                        &mut last_phase,
                    );
                    thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
                    continue;
                };
                if !snapshot.found {
                    video_ready = false;
                    detect_deadline = Some(Instant::now() + Duration::from_secs(VIDEO_DETECT_TIMEOUT_SECS));
                    service.emit_video_status(
                        &room_id,
                        "detecting",
                        "正在重新检测网页播放器…",
                        &mut last_phase,
                    );
                    thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
                    continue;
                }

                if is_host {
                    let now = Instant::now();
                    if let Some(previous) = last_snapshot.clone() {
                        let elapsed = now.duration_since(last_poll_at).as_secs_f64();
                        let expected_time = if previous.is_playing {
                            previous.current_time + elapsed * previous.playback_rate.max(0.1)
                        } else {
                            previous.current_time
                        };
                        let drift = (snapshot.current_time - expected_time).abs();
                        let action = if snapshot.is_playing != previous.is_playing {
                            Some(if snapshot.is_playing { "play" } else { "pause" })
                        } else if (snapshot.playback_rate - previous.playback_rate).abs() > 0.01 {
                            Some("rate")
                        } else if drift > SEEK_THRESHOLD_SECS {
                            Some("seek")
                        } else {
                            None
                        };
                        if let Some(action) = action {
                            let _ = service.send_sync(WatchSyncMessage {
                                room_id: room_id.clone(),
                                host_user_id: host_user_id.clone(),
                                action: action.to_string(),
                                time: snapshot.current_time,
                                playback_rate: snapshot.playback_rate,
                                is_playing: snapshot.is_playing,
                                sent_at: now_secs(),
                            });
                        }
                    }
                    if last_heartbeat.elapsed() >= Duration::from_secs(HEARTBEAT_INTERVAL_SECS) {
                        let _ = service.send_sync(WatchSyncMessage {
                            room_id: room_id.clone(),
                            host_user_id: host_user_id.clone(),
                            action: "heartbeat".to_string(),
                            time: snapshot.current_time,
                            playback_rate: snapshot.playback_rate,
                            is_playing: snapshot.is_playing,
                            sent_at: now_secs(),
                        });
                        last_heartbeat = Instant::now();
                    }
                    last_snapshot = Some(snapshot);
                    last_poll_at = now;
                }

                thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            }
        });
    }

    fn eval_video_state(&self, label: &str) -> Option<VideoSnapshot> {
        let webview = self.app.get_webview(label)?;
        let (tx, rx) = mpsc::channel::<String>();
        let script = r#"
(() => {
  try {
    const video = document.querySelector('video');
    if (!video) {
      return { found: false, current_time: 0, playback_rate: 1, is_playing: false };
    }
    return {
      found: true,
      current_time: Number(video.currentTime || 0),
      playback_rate: Number(video.playbackRate || 1),
      is_playing: !video.paused
    };
  } catch (_err) {
    return { found: false, current_time: 0, playback_rate: 1, is_playing: false };
  }
})()
"#;
        let _ = webview.eval_with_callback(script, move |payload| {
            let _ = tx.send(payload);
        });
        let payload = rx.recv_timeout(Duration::from_millis(400)).ok()?;
        serde_json::from_str::<VideoSnapshot>(&payload).ok()
    }

    fn apply_sync_to_webview(&self, label: &str, message: &WatchSyncMessage) -> Result<(), String> {
        let Some(webview) = self.app.get_webview(label) else {
            return Ok(());
        };
        let script = sync_script(message);
        webview
            .eval(script)
            .map_err(|err| format!("播放同步应用失败: {err}"))
    }

    fn append_message(
        &self,
        room_id: &str,
        message: WatchRoomChatMessage,
    ) -> Result<WatchRoomChatMessage, String> {
        let mut state = self.lock()?;
        let entry = state
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| "观影房间不存在".to_string())?;
        if entry
            .messages
            .iter()
            .any(|item| item.message_id == message.message_id)
        {
            return Ok(message);
        }
        entry.messages.push(message.clone());
        if entry.messages.len() > 200 {
            let drop_count = entry.messages.len().saturating_sub(200);
            entry.messages.drain(0..drop_count);
        }
        Ok(message)
    }

    fn broadcast_create(&self, room: &WatchRoom) {
        let payload = WatchRoomCreateMessage {
            message_type: "watch_room_create".to_string(),
            room: room.clone(),
        };
        self.broadcast_to_devices("/watch/rooms/create", &payload);
    }

    fn broadcast_state(&self, room: &WatchRoom) {
        let payload = WatchRoomStateMessage {
            message_type: "watch_room_state".to_string(),
            room: room.clone(),
        };
        self.broadcast_to_devices("/watch/rooms/state", &payload);
    }

    fn broadcast_end(&self, room: &WatchRoom, reason: &str) {
        let payload = WatchRoomEndMessage {
            message_type: "watch_room_end".to_string(),
            room_id: room.room_id.clone(),
            host_user_id: room.host_user_id.clone(),
            reason: reason.to_string(),
        };
        self.broadcast_to_devices("/watch/rooms/end", &payload);
    }

    fn broadcast_url_update(&self, payload: &WatchRoomUrlUpdateMessage) {
        self.broadcast_to_devices("/watch/rooms/url", payload);
    }

    fn broadcast_chat_message(&self, message: &WatchRoomChatMessage) {
        let payload = WatchRoomChatEnvelope {
            message_type: "watch_room_chat_message".to_string(),
            message: message.clone(),
        };
        self.broadcast_to_devices("/watch/rooms/chat", &payload);
    }

    fn broadcast_to_devices<T: Serialize>(&self, path: &str, payload: &T) {
        let Ok(body) = serde_json::to_string(payload) else {
            return;
        };
        if let Ok(discovery) = self.discovery_required() {
            for device in discovery
                .list_devices()
                .into_iter()
                .filter(|device| device.online && device.id != self.local_user_id)
            {
                self.post_to_device(&device.ip, device.api_port, path, &body);
            }
        }
    }

    fn emit_room_updated(&self, room: &WatchRoom) {
        let _ = self.app.emit("watch-room-updated", room);
    }

    fn emit_room_ended(&self, payload: &WatchRoomEndMessage) {
        let _ = self.app.emit("watch-room-ended", payload);
    }

    fn emit_chat_message(&self, message: &WatchRoomChatMessage) {
        let _ = self.app.emit("watch-room-chat-message", message);
    }

    fn emit_video_status(&self, room_id: &str, phase: &str, message: &str, last_phase: &mut String) {
        let signature = format!("{phase}:{message}");
        if *last_phase == signature {
            return;
        }
        *last_phase = signature;
        let _ = self.app.emit(
            "watch-room-video-status",
            WatchRoomVideoStatus {
                room_id: room_id.to_string(),
                phase: phase.to_string(),
                message: message.to_string(),
            },
        );
    }

    fn is_local_member(&self, entry: &RoomEntry) -> bool {
        entry.room.host_user_id == self.local_user_id || entry.members.contains(&self.local_user_id)
    }

    fn post_to_device(&self, ip: &str, api_port: u16, path: &str, body: &str) {
        if let Ok(mut stream) = StdTcpStream::connect((ip, api_port)) {
            let request = format!(
                "POST {path} HTTP/1.1\r\nHost: {ip}:{api_port}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(request.as_bytes());
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, WatchState>, String> {
        self.inner
            .lock()
            .map_err(|_| "观影房间状态当前不可用".to_string())
    }

    fn discovery(&self) -> Option<DiscoveryService> {
        self.discovery.lock().ok().and_then(|slot| slot.clone())
    }

    fn discovery_required(&self) -> Result<DiscoveryService, String> {
        self.discovery()
            .ok_or_else(|| "设备发现服务尚未初始化".to_string())
    }
}

impl Clone for SessionEntry {
    fn clone(&self) -> Self {
        Self {
            room_id: self.room_id.clone(),
            window_label: self.window_label.clone(),
            content_webview_label: self.content_webview_label.clone(),
            is_host: self.is_host,
            stop: self.stop.clone(),
        }
    }
}

fn join_key(room_id: &str, user_id: &str) -> String {
    format!("{room_id}:{user_id}")
}

fn normalize_title(value: &str, nickname: &str) -> String {
    let title = value.trim().chars().take(64).collect::<String>();
    if title.is_empty() {
        format!("{} 的观影房间", nickname.trim())
    } else {
        title
    }
}

fn validate_watch_url(value: &str) -> Result<String, String> {
    let url = value.trim();
    let parsed = Url::parse(url).map_err(|_| "视频网页链接无效".to_string())?;
    match parsed.scheme() {
        "http" | "https" => Ok(url.to_string()),
        _ => Err("仅支持 http/https 视频网页链接".to_string()),
    }
}

fn hash_required_password(value: Option<&str>) -> Result<String, String> {
    let value = value.unwrap_or("").trim();
    if value.is_empty() {
        Err("密码房间必须设置密码".to_string())
    } else {
        Ok(hash_text(value))
    }
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn watch_window_label(room_id: &str) -> String {
    format!("watch-room-{room_id}")
}

fn watch_content_webview_label(room_id: &str) -> String {
    format!("watch-content-{room_id}")
}

fn quicklan_watch_profile_dir() -> Result<std::path::PathBuf, String> {
    let path = storage::app_data_dir()
        .join("watch_profiles")
        .join("quicklan");
    std::fs::create_dir_all(&path)
        .map_err(|err| format!("创建 QuickLAN 观影登录态目录失败: {err}"))?;
    Ok(path)
}

fn system_message(room_id: &str, sender_name: &str, body: &str) -> WatchRoomChatMessage {
    WatchRoomChatMessage {
        message_id: Uuid::new_v4().to_string(),
        room_id: room_id.to_string(),
        sender_user_id: "system".to_string(),
        sender_name: sender_name.to_string(),
        body: body.to_string(),
        created_at: now_secs(),
        system: true,
    }
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn sync_script(message: &WatchSyncMessage) -> String {
    format!(
        r#"
(() => {{
  try {{
    const video = document.querySelector('video');
    if (!video) return;
    const targetTime = {time};
    const targetRate = {rate};
    const action = {action};
    const shouldPlay = {is_playing};
    if (Math.abs((video.currentTime || 0) - targetTime) > 0.2) {{
      video.currentTime = targetTime;
    }}
    if (Math.abs((video.playbackRate || 1) - targetRate) > 0.01) {{
      video.playbackRate = targetRate;
    }}
    if (action === 'pause') {{
      video.pause();
      return;
    }}
    if (action === 'heartbeat') {{
      if (Math.abs((video.currentTime || 0) - targetTime) > 1) {{
        video.currentTime = targetTime;
      }}
      if (shouldPlay) video.play().catch(() => undefined);
      else video.pause();
      return;
    }}
    if (shouldPlay) {{
      video.play().catch(() => undefined);
    }}
  }} catch (_err) {{}}
}})();
"#,
        time = message.time,
        rate = message.playback_rate,
        action = js_string(&message.action),
        is_playing = if message.is_playing { "true" } else { "false" }
    )
}
