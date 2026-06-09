use crate::storage;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const REMOTE_ROOM_STALE_SECS: i64 = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRoom {
    pub room_id: String,
    pub host_device_id: String,
    pub host_name: String,
    pub title: String,
    pub is_private: bool,
    pub password_hash: Option<String>,
    pub current_url: Option<String>,
    pub member_ids: Vec<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchChatMessage {
    pub message_id: String,
    pub room_id: String,
    pub sender_device_id: String,
    pub sender_name: String,
    pub avatar_hash: Option<String>,
    pub body: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchSyncPayload {
    pub room_id: String,
    pub host_device_id: String,
    pub action: String,
    pub time: f64,
    pub playback_rate: f64,
    pub is_playing: bool,
    pub sent_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchJoinRequest {
    pub room_id: String,
    pub user_id: String,
    pub nickname: String,
    pub password_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchJoinResponse {
    pub accepted: bool,
    pub reason: Option<String>,
    pub room: Option<WatchRoom>,
    pub sync: Option<WatchSyncPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WatchState {
    rooms: Vec<WatchRoom>,
    messages: Vec<WatchChatMessage>,
}

#[derive(Clone)]
pub struct WatchService {
    inner: Arc<Mutex<WatchState>>,
    path: PathBuf,
    local_device_id: String,
}

impl WatchService {
    pub fn load(local_device_id: String) -> Self {
        let path = storage::app_data_dir().join("watch.json");
        let state = fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<WatchState>(&content).ok())
            .unwrap_or(WatchState {
                rooms: Vec::new(),
                messages: Vec::new(),
            });
        let service = Self {
            inner: Arc::new(Mutex::new(state)),
            path,
            local_device_id,
        };
        let _ = service.save();
        service
    }

    pub fn list_rooms(&self) -> Vec<WatchRoom> {
        let changed = self.prune_stale_remote_rooms().unwrap_or(false);
        let rooms = self
            .inner
            .lock()
            .map(|state| {
                let mut rooms = state
                    .rooms
                    .iter()
                    .filter(|room| room.status == "active")
                    .cloned()
                    .collect::<Vec<_>>();
                rooms.sort_by(|a, b| {
                    b.created_at
                        .cmp(&a.created_at)
                        .then_with(|| a.title.cmp(&b.title))
                });
                rooms
            })
            .unwrap_or_default();
        if changed {
            let _ = self.save();
        }
        rooms
    }

    pub fn list_messages(&self, room_id: &str) -> Vec<WatchChatMessage> {
        let changed = self.prune_stale_remote_rooms().unwrap_or(false);
        let messages = self
            .inner
            .lock()
            .map(|state| {
                state
                    .messages
                    .iter()
                    .filter(|message| message.room_id == room_id)
                    .rev()
                    .take(200)
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect()
            })
            .unwrap_or_default();
        if changed {
            let _ = self.save();
        }
        messages
    }

    pub fn find_room(&self, room_id: &str) -> Option<WatchRoom> {
        let changed = self.prune_stale_remote_rooms().unwrap_or(false);
        let room = self.inner.lock().ok().and_then(|state| {
            state
                .rooms
                .iter()
                .find(|room| room.room_id == room_id && room.status == "active")
                .cloned()
        });
        if changed {
            let _ = self.save();
        }
        room
    }

    pub fn create_room(
        &self,
        title: String,
        host_name: String,
        is_private: bool,
        password_hash: Option<String>,
    ) -> Result<WatchRoom, String> {
        let now = now_secs();
        let room = WatchRoom {
            room_id: Uuid::new_v4().to_string(),
            host_device_id: self.local_device_id.clone(),
            host_name,
            title: clean_room_title(&title)?,
            is_private,
            password_hash: normalized_password_hash(is_private, password_hash)?,
            current_url: None,
            member_ids: vec![self.local_device_id.clone()],
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        };
        {
            let mut state = self.lock()?;
            upsert_room(&mut state.rooms, room.clone());
        }
        self.save()?;
        Ok(room)
    }

    pub fn accept_room(&self, room: WatchRoom) -> Result<(), String> {
        {
            let mut state = self.lock()?;
            upsert_room(&mut state.rooms, room);
        }
        self.save()
    }

    pub fn remove_room(&self, room_id: &str) -> Result<(), String> {
        {
            let mut state = self.lock()?;
            state.rooms.retain(|room| room.room_id != room_id);
            state.messages.retain(|message| message.room_id != room_id);
        }
        self.save()
    }

    pub fn join_room_request(&self, request: WatchJoinRequest) -> Result<WatchJoinResponse, String> {
        let mut room = self
            .find_room(&request.room_id)
            .ok_or_else(|| "观影房间不存在".to_string())?;
        if room.host_device_id != self.local_device_id {
            return Err("当前设备不是房主".to_string());
        }
        if room.is_private && room.password_hash != request.password_hash {
            return Ok(WatchJoinResponse {
                accepted: false,
                reason: Some("密码错误".to_string()),
                room: None,
                sync: None,
            });
        }
        if !room.member_ids.iter().any(|id| id == &request.user_id) {
            room.member_ids.push(request.user_id);
        }
        room.member_ids = unique_ids(room.member_ids);
        room.updated_at = now_secs();
        {
            let mut state = self.lock()?;
            upsert_room(&mut state.rooms, room.clone());
        }
        self.save()?;
        Ok(WatchJoinResponse {
            accepted: true,
            reason: None,
            room: Some(room),
            sync: None,
        })
    }

    pub fn leave_room(&self, room_id: &str, user_id: &str) -> Result<Option<WatchRoom>, String> {
        let mut next = None;
        {
            let mut state = self.lock()?;
            if let Some(room) = state.rooms.iter_mut().find(|room| room.room_id == room_id) {
                if room.host_device_id == user_id {
                    state.rooms.retain(|item| item.room_id != room_id);
                    state.messages.retain(|message| message.room_id != room_id);
                } else {
                    room.member_ids.retain(|id| id != user_id);
                    room.updated_at = now_secs();
                    next = Some(room.clone());
                }
            }
        }
        self.save()?;
        Ok(next)
    }

    pub fn update_room_url(
        &self,
        room_id: &str,
        requester_device_id: &str,
        url: String,
    ) -> Result<WatchRoom, String> {
        validate_watch_url(&url)?;
        let updated;
        {
            let mut state = self.lock()?;
            let room = state
                .rooms
                .iter_mut()
                .find(|room| room.room_id == room_id && room.status == "active")
                .ok_or_else(|| "观影房间不存在".to_string())?;
            if room.host_device_id != requester_device_id {
                return Err("只有房主可以提交视频链接".to_string());
            }
            room.current_url = Some(url);
            room.updated_at = now_secs();
            updated = room.clone();
        }
        self.save()?;
        Ok(updated)
    }

    pub fn end_room(&self, room_id: &str, requester_device_id: &str) -> Result<(), String> {
        let room = self
            .find_room(room_id)
            .ok_or_else(|| "观影房间不存在".to_string())?;
        if room.host_device_id != requester_device_id {
            return Err("只有房主可以结束房间".to_string());
        }
        self.remove_room(room_id)
    }

    pub fn add_chat_message(
        &self,
        room_id: String,
        sender_device_id: String,
        sender_name: String,
        avatar_hash: Option<String>,
        body: String,
    ) -> Result<WatchChatMessage, String> {
        if self.find_room(&room_id).is_none() {
            return Err("观影房间不存在".to_string());
        }
        let message = WatchChatMessage {
            message_id: Uuid::new_v4().to_string(),
            room_id,
            sender_device_id,
            sender_name,
            avatar_hash,
            body: clean_message(&body)?,
            created_at: now_secs(),
        };
        {
            let mut state = self.lock()?;
            if !state
                .messages
                .iter()
                .any(|item| item.message_id == message.message_id)
            {
                state.messages.push(message.clone());
                if state.messages.len() > 8_000 {
                    let drop_count = state.messages.len() - 8_000;
                    state.messages.drain(0..drop_count);
                }
            }
        }
        self.save()?;
        Ok(message)
    }

    pub fn accept_chat_message(&self, message: WatchChatMessage) -> Result<(), String> {
        {
            let mut state = self.lock()?;
            if !state
                .messages
                .iter()
                .any(|item| item.message_id == message.message_id)
            {
                state.messages.push(message);
            }
        }
        self.save()
    }

    pub fn is_member(&self, room_id: &str, user_id: &str) -> bool {
        self.find_room(room_id)
            .map(|room| room.member_ids.iter().any(|id| id == user_id))
            .unwrap_or(false)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, WatchState>, String> {
        self.inner
            .lock()
            .map_err(|_| "观影房间数据正在被占用".to_string())
    }

    fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("创建观影目录失败: {err}"))?;
        }
        let state = self.lock()?;
        let content = serde_json::to_string_pretty(&*state)
            .map_err(|err| format!("序列化观影数据失败: {err}"))?;
        fs::write(&self.path, content).map_err(|err| format!("保存观影数据失败: {err}"))
    }

    fn prune_stale_remote_rooms(&self) -> Result<bool, String> {
        let mut state = self.lock()?;
        let now = now_secs();
        let before_rooms = state.rooms.len();
        state.rooms.retain(|room| {
            room.host_device_id == self.local_device_id
                || now.saturating_sub(room.updated_at) <= REMOTE_ROOM_STALE_SECS
        });
        if state.rooms.len() == before_rooms {
            return Ok(false);
        }
        let active_room_ids = state
            .rooms
            .iter()
            .map(|room| room.room_id.clone())
            .collect::<HashSet<_>>();
        state
            .messages
            .retain(|message| active_room_ids.contains(&message.room_id));
        Ok(true)
    }

}

fn upsert_room(rooms: &mut Vec<WatchRoom>, room: WatchRoom) {
    if let Some(current) = rooms.iter_mut().find(|item| item.room_id == room.room_id) {
        *current = room;
    } else {
        rooms.push(room);
    }
}

fn unique_ids(ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter()
        .filter_map(|id| {
            let id = id.trim().to_string();
            if id.is_empty() || !seen.insert(id.clone()) {
                None
            } else {
                Some(id)
            }
        })
        .collect()
}

fn clean_room_title(value: &str) -> Result<String, String> {
    let title = value.trim().chars().take(48).collect::<String>();
    if title.is_empty() {
        Err("观影房间名称不能为空".to_string())
    } else {
        Ok(title)
    }
}

fn clean_message(value: &str) -> Result<String, String> {
    let body = value.trim().chars().take(2000).collect::<String>();
    if body.is_empty() {
        Err("消息不能为空".to_string())
    } else {
        Ok(body)
    }
}

fn normalized_password_hash(
    is_private: bool,
    password_hash: Option<String>,
) -> Result<Option<String>, String> {
    if !is_private {
        return Ok(None);
    }
    let value = password_hash
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "密码房间必须提供密码".to_string())?;
    Ok(Some(value))
}

pub fn validate_watch_url(value: &str) -> Result<(), String> {
    let url = value.trim();
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        Err("视频链接只支持 http 或 https".to_string())
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
