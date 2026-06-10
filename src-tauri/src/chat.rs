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

pub const MAIN_ROOM_ID: &str = "main";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRoom {
    pub room_id: String,
    pub name: String,
    pub owner_device_id: String,
    pub member_ids: Vec<String>,
    pub is_main: bool,
    pub created_at: i64,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub message_id: String,
    pub room_id: String,
    pub sender_device_id: String,
    pub sender_name: String,
    pub avatar_hash: Option<String>,
    pub body: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessagePayload {
    pub room: ChatRoom,
    pub message: ChatMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatState {
    rooms: Vec<ChatRoom>,
    messages: Vec<ChatMessage>,
}

#[derive(Clone)]
pub struct ChatService {
    inner: Arc<Mutex<ChatState>>,
    path: PathBuf,
    local_device_id: String,
}

impl ChatService {
    pub fn load(local_device_id: String) -> Self {
        let path = storage::app_data_dir().join("chat.json");
        let mut state = fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<ChatState>(&content).ok())
            .unwrap_or_else(default_state);
        ensure_main_room(&mut state);
        let service = Self {
            inner: Arc::new(Mutex::new(state)),
            path,
            local_device_id,
        };
        let _ = service.save();
        service
    }

    pub fn list_rooms(&self) -> Vec<ChatRoom> {
        self.inner
            .lock()
            .map(|state| {
                let mut rooms = state
                    .rooms
                    .iter()
                    .filter(|room| !room.deleted)
                    .cloned()
                    .collect::<Vec<_>>();
                rooms.sort_by(|a, b| {
                    b.is_main
                        .cmp(&a.is_main)
                        .then_with(|| a.created_at.cmp(&b.created_at))
                        .then_with(|| a.name.cmp(&b.name))
                });
                rooms
            })
            .unwrap_or_default()
    }

    pub fn list_messages(&self, room_id: &str) -> Vec<ChatMessage> {
        self.inner
            .lock()
            .map(|state| {
                state
                    .messages
                    .iter()
                    .filter(|message| message.room_id == room_id)
                    .rev()
                    .take(100)
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn create_room(
        &self,
        name: String,
        member_ids: Vec<String>,
        owner_device_id: String,
    ) -> Result<ChatRoom, String> {
        let clean_name = clean_room_name(&name)?;
        let mut members = unique_ids(member_ids);
        if !members.iter().any(|id| id == &owner_device_id) {
            members.push(owner_device_id.clone());
        }
        let room;
        {
            let mut state = self.lock()?;
            if state
                .rooms
                .iter()
                .any(|room| !room.deleted && room.name == clean_name)
            {
                return Err("已存在同名聊天室".to_string());
            }
            room = ChatRoom {
                room_id: Uuid::new_v4().to_string(),
                name: clean_name,
                owner_device_id,
                member_ids: members,
                is_main: false,
                created_at: now_secs(),
                deleted: false,
            };
            state.rooms.push(room.clone());
        }
        self.save()?;
        Ok(room)
    }

    pub fn delete_room(
        &self,
        room_id: &str,
        requester_device_id: &str,
    ) -> Result<ChatRoom, String> {
        let deleted;
        {
            let mut state = self.lock()?;
            let room = state
                .rooms
                .iter_mut()
                .find(|room| room.room_id == room_id && !room.deleted)
                .ok_or_else(|| "聊天室不存在".to_string())?;
            if room.is_main {
                return Err("主聊天室不能删除".to_string());
            }
            if room.owner_device_id != requester_device_id {
                return Err("只有创建者可以删除聊天室".to_string());
            }
            room.deleted = true;
            deleted = room.clone();
        }
        self.save()?;
        Ok(deleted)
    }

    pub fn remove_remote_room(&self, room_id: &str) -> Result<(), String> {
        {
            let mut state = self.lock()?;
            if let Some(room) = state.rooms.iter_mut().find(|room| room.room_id == room_id) {
                room.deleted = true;
            }
        }
        self.save()
    }

    pub fn add_message(
        &self,
        room_id: String,
        sender_device_id: String,
        sender_name: String,
        avatar_hash: Option<String>,
        body: String,
    ) -> Result<ChatMessagePayload, String> {
        let body = clean_message(&body)?;
        let room = self
            .find_room(&room_id)
            .ok_or_else(|| "聊天室不存在".to_string())?;
        let message = ChatMessage {
            message_id: Uuid::new_v4().to_string(),
            room_id,
            sender_device_id,
            sender_name,
            avatar_hash,
            body,
            created_at: now_secs(),
        };
        self.insert_message(room.clone(), message.clone())?;
        Ok(ChatMessagePayload { room, message })
    }

    pub fn accept_room(&self, room: ChatRoom) -> Result<(), String> {
        {
            let mut state = self.lock()?;
            upsert_room(&mut state.rooms, room);
        }
        self.save()
    }

    pub fn accept_message(&self, payload: ChatMessagePayload) -> Result<(), String> {
        self.insert_message(payload.room, payload.message)
    }

    pub fn find_room(&self, room_id: &str) -> Option<ChatRoom> {
        self.inner.lock().ok().and_then(|state| {
            state
                .rooms
                .iter()
                .find(|room| room.room_id == room_id && !room.deleted)
                .cloned()
        })
    }

    pub fn is_local_member(&self, room: &ChatRoom) -> bool {
        room.is_main || room.member_ids.iter().any(|id| id == &self.local_device_id)
    }

    fn insert_message(&self, room: ChatRoom, message: ChatMessage) -> Result<(), String> {
        {
            let mut state = self.lock()?;
            upsert_room(&mut state.rooms, room);
            if !state
                .messages
                .iter()
                .any(|item| item.message_id == message.message_id)
            {
                state.messages.push(message);
                if state.messages.len() > 5000 {
                    let drop_count = state.messages.len() - 5000;
                    state.messages.drain(0..drop_count);
                }
            }
        }
        self.save()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, ChatState>, String> {
        self.inner
            .lock()
            .map_err(|_| "聊天室数据正在被占用".to_string())
    }

    fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("创建聊天目录失败: {err}"))?;
        }
        let state = self.lock()?;
        let content = serde_json::to_string_pretty(&*state)
            .map_err(|err| format!("序列化聊天数据失败: {err}"))?;
        fs::write(&self.path, content).map_err(|err| format!("保存聊天数据失败: {err}"))
    }
}

fn default_state() -> ChatState {
    let mut state = ChatState {
        rooms: Vec::new(),
        messages: Vec::new(),
    };
    ensure_main_room(&mut state);
    state
}

fn ensure_main_room(state: &mut ChatState) {
    if !state.rooms.iter().any(|room| room.room_id == MAIN_ROOM_ID) {
        state.rooms.push(ChatRoom {
            room_id: MAIN_ROOM_ID.to_string(),
            name: "主聊天室".to_string(),
            owner_device_id: "system".to_string(),
            member_ids: Vec::new(),
            is_main: true,
            created_at: 0,
            deleted: false,
        });
    }
}

fn upsert_room(rooms: &mut Vec<ChatRoom>, room: ChatRoom) {
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

fn clean_room_name(value: &str) -> Result<String, String> {
    let name = value.trim().chars().take(32).collect::<String>();
    if name.is_empty() {
        Err("聊天室名称不能为空".to_string())
    } else {
        Ok(name)
    }
}

fn clean_message(value: &str) -> Result<String, String> {
    let body = value.trim().chars().take(1000).collect::<String>();
    if body.is_empty() {
        Err("消息不能为空".to_string())
    } else {
        Ok(body)
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
