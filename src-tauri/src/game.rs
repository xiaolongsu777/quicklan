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
const BOARD_SIZE: usize = 15;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GameType {
    Gomoku,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GameRoomVisibility {
    Public,
    Password,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GameRoomStatus {
    Waiting,
    Playing,
    Finished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRoomSummary {
    pub room_id: String,
    pub game_type: GameType,
    pub room_name: String,
    pub host_peer_id: String,
    pub host_name: String,
    pub guest_peer_id: Option<String>,
    pub guest_name: Option<String>,
    pub visibility: GameRoomVisibility,
    pub password_hash: Option<String>,
    pub status: GameRoomStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GomokuMoveRecord {
    pub x: usize,
    pub y: usize,
    pub color: u8,
    pub peer_id: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GomokuState {
    pub room_id: String,
    pub board: Vec<Vec<u8>>,
    pub current_turn: u8,
    pub black_peer_id: String,
    pub white_peer_id: Option<String>,
    pub winner: Option<u8>,
    pub last_move: Option<GomokuPoint>,
    pub move_history: Vec<GomokuMoveRecord>,
    pub restart_requested_by: Option<String>,
    pub ended_reason: Option<String>,
    pub status_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GomokuPoint {
    pub x: usize,
    pub y: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRoomSnapshot {
    pub room: GameRoomSummary,
    pub gomoku_state: GomokuState,
    pub version: i64,
    pub last_event_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGameRoomRequest {
    pub room_name: String,
    pub visibility: GameRoomVisibility,
    pub password_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameJoinRequest {
    pub room_id: String,
    pub user_id: String,
    pub nickname: String,
    pub password_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameJoinResponse {
    pub accepted: bool,
    pub reason: Option<String>,
    pub snapshot: Option<GameRoomSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameActivation {
    pub room: GameRoomSummary,
    pub is_host: bool,
    pub is_member: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GameState {
    rooms: Vec<GameRoomSnapshot>,
}

#[derive(Clone)]
pub struct GameService {
    inner: Arc<Mutex<GameState>>,
    path: PathBuf,
    local_device_id: String,
}

impl GameService {
    pub fn load(local_device_id: String) -> Self {
        let path = storage::app_data_dir().join("games.json");
        let state = fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<GameState>(&content).ok())
            .unwrap_or(GameState { rooms: Vec::new() });
        let service = Self {
            inner: Arc::new(Mutex::new(state)),
            path,
            local_device_id,
        };
        let _ = service.save();
        service
    }

    pub fn list_rooms(&self, game_type: Option<GameType>) -> Vec<GameRoomSummary> {
        let changed = self.prune_stale_remote_rooms().unwrap_or(false);
        let rooms = self
            .inner
            .lock()
            .map(|state| {
                let mut rooms = state
                    .rooms
                    .iter()
                    .filter(|snapshot| {
                        game_type
                            .as_ref()
                            .map(|value| snapshot.room.game_type == *value)
                            .unwrap_or(true)
                    })
                    .map(|snapshot| snapshot.room.clone())
                    .collect::<Vec<_>>();
                rooms.sort_by(|a, b| {
                    b.created_at
                        .cmp(&a.created_at)
                        .then_with(|| a.room_name.cmp(&b.room_name))
                });
                rooms
            })
            .unwrap_or_default();
        if changed {
            let _ = self.save();
        }
        rooms
    }

    pub fn find_room(&self, room_id: &str) -> Option<GameRoomSummary> {
        self.find_snapshot(room_id).map(|snapshot| snapshot.room)
    }

    pub fn find_snapshot(&self, room_id: &str) -> Option<GameRoomSnapshot> {
        let changed = self.prune_stale_remote_rooms().unwrap_or(false);
        let room = self.inner.lock().ok().and_then(|state| {
            state
                .rooms
                .iter()
                .find(|snapshot| snapshot.room.room_id == room_id)
                .cloned()
        });
        if changed {
            let _ = self.save();
        }
        room
    }

    pub fn create_room(
        &self,
        request: CreateGameRoomRequest,
        host_name: String,
    ) -> Result<GameRoomSnapshot, String> {
        let now = now_secs();
        let room_id = Uuid::new_v4().to_string();
        let room_name = clean_room_name(&request.room_name)?;
        let visibility = request.visibility;
        let password_hash = normalized_password_hash(&visibility, request.password_hash)?;
        let room = GameRoomSummary {
            room_id: room_id.clone(),
            game_type: GameType::Gomoku,
            room_name,
            host_peer_id: self.local_device_id.clone(),
            host_name,
            guest_peer_id: None,
            guest_name: None,
            visibility,
            password_hash,
            status: GameRoomStatus::Waiting,
            created_at: now,
            updated_at: now,
        };
        let snapshot = GameRoomSnapshot {
            room: room.clone(),
            gomoku_state: new_gomoku_state(&room),
            version: 1,
            last_event_id: Uuid::new_v4().to_string(),
        };
        {
            let mut state = self.lock()?;
            upsert_snapshot(&mut state.rooms, snapshot.clone());
        }
        self.save()?;
        Ok(snapshot)
    }

    pub fn accept_room(&self, snapshot: GameRoomSnapshot) -> Result<(), String> {
        {
            let mut state = self.lock()?;
            upsert_snapshot(&mut state.rooms, snapshot);
        }
        self.save()
    }

    pub fn remove_room(&self, room_id: &str) -> Result<(), String> {
        {
            let mut state = self.lock()?;
            state
                .rooms
                .retain(|snapshot| snapshot.room.room_id != room_id);
        }
        self.save()
    }

    pub fn join_room_request(&self, request: GameJoinRequest) -> Result<GameJoinResponse, String> {
        let mut snapshot = self
            .find_snapshot(&request.room_id)
            .ok_or_else(|| "小游戏房间不存在".to_string())?;
        if snapshot.room.host_peer_id != self.local_device_id {
            return Err("当前设备不是房主".to_string());
        }
        if snapshot.room.visibility == GameRoomVisibility::Password
            && snapshot.room.password_hash != request.password_hash
        {
            return Ok(GameJoinResponse {
                accepted: false,
                reason: Some("密码错误".to_string()),
                snapshot: None,
            });
        }
        if snapshot.room.guest_peer_id.as_deref() == Some(request.user_id.as_str()) {
            return Ok(GameJoinResponse {
                accepted: true,
                reason: None,
                snapshot: Some(snapshot),
            });
        }
        if snapshot.room.guest_peer_id.is_some() {
            return Ok(GameJoinResponse {
                accepted: false,
                reason: Some("房间已满".to_string()),
                snapshot: None,
            });
        }
        snapshot.room.guest_peer_id = Some(request.user_id.clone());
        snapshot.room.guest_name = Some(clean_nickname(&request.nickname));
        snapshot.room.status = GameRoomStatus::Playing;
        snapshot.room.updated_at = now_secs();
        snapshot.gomoku_state.white_peer_id = Some(request.user_id);
        snapshot.gomoku_state.current_turn = 1;
        snapshot.gomoku_state.status_text = "黑方回合".to_string();
        touch_snapshot(&mut snapshot);
        {
            let mut state = self.lock()?;
            upsert_snapshot(&mut state.rooms, snapshot.clone());
        }
        self.save()?;
        Ok(GameJoinResponse {
            accepted: true,
            reason: None,
            snapshot: Some(snapshot),
        })
    }

    pub fn leave_room(
        &self,
        room_id: &str,
        user_id: &str,
    ) -> Result<Option<GameRoomSnapshot>, String> {
        let mut next = None;
        {
            let mut state = self.lock()?;
            if let Some(index) = state
                .rooms
                .iter()
                .position(|snapshot| snapshot.room.room_id == room_id)
            {
                let snapshot = &mut state.rooms[index];
                if snapshot.room.host_peer_id == user_id {
                    state.rooms.remove(index);
                } else if snapshot.room.guest_peer_id.as_deref() == Some(user_id) {
                    reset_guest(snapshot);
                    next = Some(snapshot.clone());
                }
            }
        }
        self.save()?;
        Ok(next)
    }

    pub fn close_room(&self, room_id: &str, requester_id: &str) -> Result<(), String> {
        let room = self
            .find_room(room_id)
            .ok_or_else(|| "小游戏房间不存在".to_string())?;
        if room.host_peer_id != requester_id {
            return Err("只有房主可以解散房间".to_string());
        }
        self.remove_room(room_id)
    }

    pub fn request_move(
        &self,
        room_id: &str,
        actor_peer_id: &str,
        x: usize,
        y: usize,
    ) -> Result<GameRoomSnapshot, String> {
        let mut snapshot = self
            .find_snapshot(room_id)
            .ok_or_else(|| "小游戏房间不存在".to_string())?;
        ensure_host(&snapshot, &self.local_device_id)?;
        ensure_member(&snapshot, actor_peer_id)?;
        if snapshot.room.status != GameRoomStatus::Playing {
            return Err("当前房间不在对局中".to_string());
        }
        if x >= BOARD_SIZE || y >= BOARD_SIZE {
            return Err("落子位置超出棋盘范围".to_string());
        }
        let color = color_for_peer(&snapshot, actor_peer_id)?;
        if snapshot.gomoku_state.current_turn != color {
            return Err("当前还不是你的回合".to_string());
        }
        if snapshot.gomoku_state.board[y][x] != 0 {
            return Err("该位置已有棋子".to_string());
        }
        snapshot.gomoku_state.board[y][x] = color;
        snapshot.gomoku_state.last_move = Some(GomokuPoint { x, y });
        snapshot.gomoku_state.move_history.push(GomokuMoveRecord {
            x,
            y,
            color,
            peer_id: actor_peer_id.to_string(),
            timestamp: now_secs(),
        });
        snapshot.gomoku_state.restart_requested_by = None;
        if is_winning_move(&snapshot.gomoku_state.board, x, y, color) {
            snapshot.gomoku_state.winner = Some(color);
            snapshot.room.status = GameRoomStatus::Finished;
            snapshot.gomoku_state.ended_reason = Some("win".to_string());
            snapshot.gomoku_state.status_text = if color == 1 {
                "黑方获胜".to_string()
            } else {
                "白方获胜".to_string()
            };
        } else {
            snapshot.gomoku_state.current_turn = if color == 1 { 2 } else { 1 };
            snapshot.gomoku_state.status_text = if snapshot.gomoku_state.current_turn == 1 {
                "黑方回合".to_string()
            } else {
                "白方回合".to_string()
            };
        }
        snapshot.room.updated_at = now_secs();
        touch_snapshot(&mut snapshot);
        {
            let mut state = self.lock()?;
            upsert_snapshot(&mut state.rooms, snapshot.clone());
        }
        self.save()?;
        Ok(snapshot)
    }

    pub fn request_restart(
        &self,
        room_id: &str,
        actor_peer_id: &str,
    ) -> Result<GameRoomSnapshot, String> {
        let mut snapshot = self
            .find_snapshot(room_id)
            .ok_or_else(|| "小游戏房间不存在".to_string())?;
        ensure_host(&snapshot, &self.local_device_id)?;
        ensure_member(&snapshot, actor_peer_id)?;
        snapshot.gomoku_state.restart_requested_by = Some(actor_peer_id.to_string());
        snapshot.gomoku_state.status_text = "等待对方确认重新开始".to_string();
        snapshot.room.updated_at = now_secs();
        touch_snapshot(&mut snapshot);
        {
            let mut state = self.lock()?;
            upsert_snapshot(&mut state.rooms, snapshot.clone());
        }
        self.save()?;
        Ok(snapshot)
    }

    pub fn accept_restart(
        &self,
        room_id: &str,
        actor_peer_id: &str,
    ) -> Result<GameRoomSnapshot, String> {
        let mut snapshot = self
            .find_snapshot(room_id)
            .ok_or_else(|| "小游戏房间不存在".to_string())?;
        ensure_host(&snapshot, &self.local_device_id)?;
        ensure_member(&snapshot, actor_peer_id)?;
        let requester = snapshot
            .gomoku_state
            .restart_requested_by
            .clone()
            .ok_or_else(|| "当前没有待确认的重新开始请求".to_string())?;
        if requester == actor_peer_id {
            return Err("不能由发起者自己确认重新开始".to_string());
        }
        let black_peer_id = snapshot.gomoku_state.black_peer_id.clone();
        let white_peer_id = snapshot.gomoku_state.white_peer_id.clone();
        snapshot.gomoku_state = new_gomoku_state(&snapshot.room);
        snapshot.gomoku_state.black_peer_id = black_peer_id;
        snapshot.gomoku_state.white_peer_id = white_peer_id;
        snapshot.room.status = if snapshot.room.guest_peer_id.is_some() {
            GameRoomStatus::Playing
        } else {
            GameRoomStatus::Waiting
        };
        snapshot.gomoku_state.status_text = if snapshot.room.status == GameRoomStatus::Playing {
            "黑方回合".to_string()
        } else {
            "等待玩家加入".to_string()
        };
        snapshot.room.updated_at = now_secs();
        touch_snapshot(&mut snapshot);
        {
            let mut state = self.lock()?;
            upsert_snapshot(&mut state.rooms, snapshot.clone());
        }
        self.save()?;
        Ok(snapshot)
    }

    pub fn surrender(
        &self,
        room_id: &str,
        actor_peer_id: &str,
    ) -> Result<GameRoomSnapshot, String> {
        let mut snapshot = self
            .find_snapshot(room_id)
            .ok_or_else(|| "小游戏房间不存在".to_string())?;
        ensure_host(&snapshot, &self.local_device_id)?;
        ensure_member(&snapshot, actor_peer_id)?;
        let color = color_for_peer(&snapshot, actor_peer_id)?;
        let winner = if color == 1 { 2 } else { 1 };
        snapshot.room.status = GameRoomStatus::Finished;
        snapshot.gomoku_state.winner = Some(winner);
        snapshot.gomoku_state.ended_reason = Some("surrender".to_string());
        snapshot.gomoku_state.restart_requested_by = None;
        snapshot.gomoku_state.status_text = if winner == 1 {
            "黑方获胜（对手认输）".to_string()
        } else {
            "白方获胜（对手认输）".to_string()
        };
        snapshot.room.updated_at = now_secs();
        touch_snapshot(&mut snapshot);
        {
            let mut state = self.lock()?;
            upsert_snapshot(&mut state.rooms, snapshot.clone());
        }
        self.save()?;
        Ok(snapshot)
    }

    pub fn activation(&self, room_id: &str, local_id: &str) -> Result<GameActivation, String> {
        let room = self
            .find_room(room_id)
            .ok_or_else(|| "小游戏房间不存在".to_string())?;
        let is_host = room.host_peer_id == local_id;
        let is_member = is_host || room.guest_peer_id.as_deref() == Some(local_id);
        Ok(GameActivation {
            room,
            is_host,
            is_member,
        })
    }

    pub fn hosted_rooms(&self) -> Vec<GameRoomSnapshot> {
        self.list_snapshots()
            .into_iter()
            .filter(|snapshot| snapshot.room.host_peer_id == self.local_device_id)
            .collect()
    }

    pub fn reconcile_hosted_rooms(
        &self,
        online_ids: &HashSet<String>,
    ) -> Result<Vec<GameRoomSnapshot>, String> {
        let mut changed = Vec::new();
        {
            let mut state = self.lock()?;
            for snapshot in &mut state.rooms {
                if snapshot.room.host_peer_id != self.local_device_id {
                    continue;
                }
                let Some(guest_id) = snapshot.room.guest_peer_id.clone() else {
                    continue;
                };
                if !online_ids.contains(&guest_id) {
                    reset_guest(snapshot);
                    changed.push(snapshot.clone());
                }
            }
        }
        if !changed.is_empty() {
            self.save()?;
        }
        Ok(changed)
    }

    fn list_snapshots(&self) -> Vec<GameRoomSnapshot> {
        self.inner
            .lock()
            .map(|state| state.rooms.clone())
            .unwrap_or_default()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, GameState>, String> {
        self.inner
            .lock()
            .map_err(|_| "小游戏房间数据正在被占用".to_string())
    }

    fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("创建小游戏目录失败: {err}"))?;
        }
        let state = self.lock()?;
        let content = serde_json::to_string_pretty(&*state)
            .map_err(|err| format!("序列化小游戏数据失败: {err}"))?;
        fs::write(&self.path, content).map_err(|err| format!("保存小游戏数据失败: {err}"))
    }

    fn prune_stale_remote_rooms(&self) -> Result<bool, String> {
        let mut state = self.lock()?;
        let now = now_secs();
        let before = state.rooms.len();
        state.rooms.retain(|snapshot| {
            snapshot.room.host_peer_id == self.local_device_id
                || now.saturating_sub(snapshot.room.updated_at) <= REMOTE_ROOM_STALE_SECS
        });
        Ok(state.rooms.len() != before)
    }
}

fn upsert_snapshot(rooms: &mut Vec<GameRoomSnapshot>, snapshot: GameRoomSnapshot) {
    if let Some(current) = rooms
        .iter_mut()
        .find(|item| item.room.room_id == snapshot.room.room_id)
    {
        *current = snapshot;
    } else {
        rooms.push(snapshot);
    }
}

fn touch_snapshot(snapshot: &mut GameRoomSnapshot) {
    snapshot.version += 1;
    snapshot.last_event_id = Uuid::new_v4().to_string();
}

fn clean_room_name(value: &str) -> Result<String, String> {
    let name = value.trim().chars().take(48).collect::<String>();
    if name.is_empty() {
        Err("小游戏房间名称不能为空".to_string())
    } else {
        Ok(name)
    }
}

fn clean_nickname(value: &str) -> String {
    let nickname = value.trim().chars().take(32).collect::<String>();
    if nickname.is_empty() {
        "玩家".to_string()
    } else {
        nickname
    }
}

fn normalized_password_hash(
    visibility: &GameRoomVisibility,
    password_hash: Option<String>,
) -> Result<Option<String>, String> {
    if *visibility == GameRoomVisibility::Public {
        return Ok(None);
    }
    let value = password_hash
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .ok_or_else(|| "密码房间必须提供密码".to_string())?;
    Ok(Some(value))
}

fn new_gomoku_state(room: &GameRoomSummary) -> GomokuState {
    GomokuState {
        room_id: room.room_id.clone(),
        board: vec![vec![0; BOARD_SIZE]; BOARD_SIZE],
        current_turn: 1,
        black_peer_id: room.host_peer_id.clone(),
        white_peer_id: room.guest_peer_id.clone(),
        winner: None,
        last_move: None,
        move_history: Vec::new(),
        restart_requested_by: None,
        ended_reason: None,
        status_text: if room.guest_peer_id.is_some() {
            "黑方回合".to_string()
        } else {
            "等待玩家加入".to_string()
        },
    }
}

fn reset_guest(snapshot: &mut GameRoomSnapshot) {
    snapshot.room.guest_peer_id = None;
    snapshot.room.guest_name = None;
    snapshot.room.status = GameRoomStatus::Waiting;
    snapshot.room.updated_at = now_secs();
    let black_peer_id = snapshot.gomoku_state.black_peer_id.clone();
    snapshot.gomoku_state = new_gomoku_state(&snapshot.room);
    snapshot.gomoku_state.black_peer_id = black_peer_id;
    snapshot.gomoku_state.white_peer_id = None;
    touch_snapshot(snapshot);
}

fn ensure_host(snapshot: &GameRoomSnapshot, local_device_id: &str) -> Result<(), String> {
    if snapshot.room.host_peer_id == local_device_id {
        Ok(())
    } else {
        Err("当前设备不是房主".to_string())
    }
}

fn ensure_member(snapshot: &GameRoomSnapshot, peer_id: &str) -> Result<(), String> {
    if snapshot.room.host_peer_id == peer_id
        || snapshot.room.guest_peer_id.as_deref() == Some(peer_id)
    {
        Ok(())
    } else {
        Err("你不在当前房间中".to_string())
    }
}

fn color_for_peer(snapshot: &GameRoomSnapshot, peer_id: &str) -> Result<u8, String> {
    if snapshot.gomoku_state.black_peer_id == peer_id {
        Ok(1)
    } else if snapshot.gomoku_state.white_peer_id.as_deref() == Some(peer_id) {
        Ok(2)
    } else {
        Err("当前玩家没有执棋资格".to_string())
    }
}

fn is_winning_move(board: &[Vec<u8>], x: usize, y: usize, color: u8) -> bool {
    const DIRECTIONS: &[(isize, isize)] = &[(1, 0), (0, 1), (1, 1), (1, -1)];
    DIRECTIONS.iter().any(|(dx, dy)| {
        let total = 1
            + count_direction(board, x, y, color, *dx, *dy)
            + count_direction(board, x, y, color, -*dx, -*dy);
        total >= 5
    })
}

fn count_direction(
    board: &[Vec<u8>],
    x: usize,
    y: usize,
    color: u8,
    dx: isize,
    dy: isize,
) -> usize {
    let mut count = 0;
    let mut next_x = x as isize + dx;
    let mut next_y = y as isize + dy;
    while next_x >= 0
        && next_y >= 0
        && (next_x as usize) < BOARD_SIZE
        && (next_y as usize) < BOARD_SIZE
        && board[next_y as usize][next_x as usize] == color
    {
        count += 1;
        next_x += dx;
        next_y += dy;
    }
    count
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
