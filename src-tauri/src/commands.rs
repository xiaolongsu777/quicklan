use crate::{
    chat::{ChatMessage, ChatMessagePayload, ChatRoom, MAIN_ROOM_ID},
    game::{
        CreateGameRoomRequest, GameActivation, GameJoinRequest, GameJoinResponse, GameRoomSnapshot,
        GameRoomSummary, GameType,
    },
    protocol::{DeviceInfo, LibrarySettings, NetworkStatus, ShareItem, TransferInfo},
    settings::AppSettings,
    storage,
    watch::{WatchChatMessage, WatchJoinRequest, WatchJoinResponse, WatchRoom},
    watch_player::WatchBounds,
    AppInfo, AppState, ControlApiInfo,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpStream as StdTcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(target_os = "linux")]
fn open_file_manager(path: &std::path::Path) -> Result<(), String> {
    // Try dbus-based file manager first (GNOME, KDE, etc.)
    let file_uri = format!("file://{}", path.display());
    if Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
            &format!("array:string:{}", file_uri),
            "string:",
        ])
        .spawn()
        .map(|mut child| child.wait())
        .is_ok()
    {
        return Ok(());
    }
    // Fallback to xdg-open on the directory
    let dir = if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    Command::new("xdg-open")
        .arg(dir)
        .spawn()
        .map_err(|err| format!("打开文件管理器失败: {err}"))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_file_manager(path: &std::path::Path) -> Result<(), String> {
    if path.is_file() {
        let target = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        Command::new("explorer.exe")
            .arg(format!("/select,{}", target.display()))
            .spawn()
            .map_err(|err| format!("打开资源管理器失败: {err}"))?;
    } else {
        let mut target = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        };
        while !target.exists() {
            let Some(parent) = target.parent().map(PathBuf::from) else {
                target = PathBuf::from(".");
                break;
            };
            target = parent;
        }
        let target = target.canonicalize().unwrap_or(target);
        Command::new("explorer.exe")
            .arg(target)
            .spawn()
            .map_err(|err| format!("打开资源管理器失败: {err}"))?;
    }
    Ok(())
}

#[tauri::command]
pub fn list_devices(state: State<'_, AppState>) -> Vec<DeviceInfo> {
    state.discovery.list_devices()
}

#[tauri::command]
pub fn update_device_note(
    state: State<'_, AppState>,
    device_id: String,
    note: String,
) -> Result<Vec<DeviceInfo>, String> {
    state.discovery.update_device_note(device_id, note)
}

#[tauri::command]
pub fn list_chat_rooms(state: State<'_, AppState>) -> Vec<ChatRoom> {
    state.chat.list_rooms()
}

#[tauri::command]
pub fn list_chat_messages(state: State<'_, AppState>, room_id: String) -> Vec<ChatMessage> {
    state.chat.list_messages(&room_id)
}

#[tauri::command]
pub fn create_chat_room(
    state: State<'_, AppState>,
    name: String,
    member_ids: Vec<String>,
) -> Result<ChatRoom, String> {
    let room = state
        .chat
        .create_room(name, member_ids, state.library.device_id())?;
    post_room_invites(&state, &room);
    Ok(room)
}

#[tauri::command]
pub fn delete_chat_room(state: State<'_, AppState>, room_id: String) -> Result<(), String> {
    let room = state
        .chat
        .delete_room(&room_id, &state.library.device_id())?;
    post_room_delete(&state, &room);
    Ok(())
}

#[tauri::command]
pub fn send_chat_message(
    state: State<'_, AppState>,
    room_id: String,
    body: String,
) -> Result<ChatMessagePayload, String> {
    let payload = state.chat.add_message(
        room_id,
        state.library.device_id(),
        state.settings.nickname(),
        state.settings.avatar_hash(),
        body,
    )?;
    post_chat_message(&state, &payload);
    Ok(payload)
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchActivation {
    pub room: WatchRoom,
    pub is_host: bool,
    pub is_member: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalEzmovieStreamInfo {
    pub active: bool,
    pub url: String,
    pub lan_preview_url: String,
    pub quick_lan_import_url: String,
    pub room_id: String,
    pub stream_code: Option<String>,
}

#[tauri::command]
pub fn list_game_rooms(
    state: State<'_, AppState>,
    game_type: Option<String>,
) -> Result<Vec<GameRoomSummary>, String> {
    let game_type = match game_type.as_deref() {
        Some("gomoku") => Some(GameType::Gomoku),
        Some(other) => return Err(format!("不支持的游戏类型: {other}")),
        None => None,
    };
    Ok(state.game.list_rooms(game_type))
}

#[tauri::command]
pub fn get_game_room_state(
    state: State<'_, AppState>,
    room_id: String,
) -> Result<GameRoomSnapshot, String> {
    state
        .game
        .find_snapshot(&room_id)
        .ok_or_else(|| "小游戏房间不存在".to_string())
}

#[tauri::command]
pub fn create_game_room(
    state: State<'_, AppState>,
    room_name: String,
    visibility: String,
    password_hash: Option<String>,
) -> Result<GameRoomSnapshot, String> {
    let visibility = match visibility.as_str() {
        "public" => crate::game::GameRoomVisibility::Public,
        "password" => crate::game::GameRoomVisibility::Password,
        _ => return Err("无效的房间可见性".to_string()),
    };
    let normalized_room_name = if room_name.trim().is_empty() {
        format!("{} 的五子棋房间", state.settings.nickname())
    } else {
        room_name
    };
    let snapshot = state.game.create_room(
        CreateGameRoomRequest {
            room_name: normalized_room_name,
            visibility,
            password_hash,
        },
        state.settings.nickname(),
    )?;
    broadcast_game_room(&state, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub fn join_game_room(
    state: State<'_, AppState>,
    room_id: String,
    password_hash: Option<String>,
    host_peer_id: Option<String>,
) -> Result<GameJoinResponse, String> {
    let room = state.game.find_room(&room_id);
    let resolved_host_peer_id = room
        .as_ref()
        .map(|summary| summary.host_peer_id.clone())
        .or(host_peer_id)
        .ok_or_else(|| "????????".to_string())?;
    let response = if resolved_host_peer_id == state.library.device_id() {
        state.game.join_room_request(GameJoinRequest {
            room_id,
            user_id: state.library.device_id(),
            nickname: state.settings.nickname(),
            password_hash,
        })?
    } else {
        let host = state
            .discovery
            .find_device(&resolved_host_peer_id)
            .ok_or_else(|| "???????".to_string())?;
        post_lan_json_response(
            &host,
            "/games/rooms/join",
            &GameJoinRequest {
                room_id,
                user_id: state.library.device_id(),
                nickname: state.settings.nickname(),
                password_hash,
            },
        )?
    };
    if let Some(snapshot) = response.snapshot.clone() {
        state.game.accept_room(snapshot)?;
    }
    Ok(response)
}

#[tauri::command]
pub fn leave_game_room(state: State<'_, AppState>, room_id: String) -> Result<(), String> {
    let local_id = state.library.device_id();
    let Some(room) = state.game.find_room(&room_id) else {
        return Ok(());
    };
    if room.host_peer_id == local_id {
        state.game.close_room(&room_id, &local_id)?;
        broadcast_game_room_end(&state, &room_id);
    } else {
        let host = state
            .discovery
            .find_device(&room.host_peer_id)
            .ok_or_else(|| "房主当前不在线".to_string())?;
        let response = post_lan_json_response::<serde_json::Value, _>(
            &host,
            "/games/rooms/leave",
            &serde_json::json!({
                "room_id": room_id,
                "user_id": local_id,
            }),
        )?;
        if let Ok(snapshot) = serde_json::from_value::<GameRoomSnapshot>(response.clone()) {
            state.game.accept_room(snapshot)?;
        } else {
            let _ = state
                .game
                .leave_room(&room.room_id, &state.library.device_id())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn close_game_room(state: State<'_, AppState>, room_id: String) -> Result<(), String> {
    state
        .game
        .close_room(&room_id, &state.library.device_id())?;
    broadcast_game_room_end(&state, &room_id);
    Ok(())
}

#[tauri::command]
pub fn activate_game_room(
    state: State<'_, AppState>,
    room_id: String,
) -> Result<GameActivation, String> {
    state.game.activation(&room_id, &state.library.device_id())
}

#[tauri::command]
pub fn request_gomoku_move(
    state: State<'_, AppState>,
    room_id: String,
    x: usize,
    y: usize,
) -> Result<GameRoomSnapshot, String> {
    let room = state
        .game
        .find_room(&room_id)
        .ok_or_else(|| "小游戏房间不存在".to_string())?;
    let snapshot = if room.host_peer_id == state.library.device_id() {
        state
            .game
            .request_move(&room_id, &state.library.device_id(), x, y)?
    } else {
        let host = state
            .discovery
            .find_device(&room.host_peer_id)
            .ok_or_else(|| "房主当前不在线".to_string())?;
        post_lan_json_response(
            &host,
            "/games/gomoku/move",
            &serde_json::json!({
                "room_id": room_id,
                "actor_peer_id": state.library.device_id(),
                "x": x,
                "y": y,
            }),
        )?
    };
    state.game.accept_room(snapshot.clone())?;
    broadcast_if_local_host(&state, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub fn request_gomoku_restart(
    state: State<'_, AppState>,
    room_id: String,
) -> Result<GameRoomSnapshot, String> {
    let room = state
        .game
        .find_room(&room_id)
        .ok_or_else(|| "小游戏房间不存在".to_string())?;
    let snapshot = if room.host_peer_id == state.library.device_id() {
        state
            .game
            .request_restart(&room_id, &state.library.device_id())?
    } else {
        let host = state
            .discovery
            .find_device(&room.host_peer_id)
            .ok_or_else(|| "房主当前不在线".to_string())?;
        post_lan_json_response(
            &host,
            "/games/gomoku/restart/request",
            &serde_json::json!({
                "room_id": room_id,
                "actor_peer_id": state.library.device_id(),
            }),
        )?
    };
    state.game.accept_room(snapshot.clone())?;
    broadcast_if_local_host(&state, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub fn accept_gomoku_restart(
    state: State<'_, AppState>,
    room_id: String,
) -> Result<GameRoomSnapshot, String> {
    let room = state
        .game
        .find_room(&room_id)
        .ok_or_else(|| "小游戏房间不存在".to_string())?;
    let snapshot = if room.host_peer_id == state.library.device_id() {
        state
            .game
            .accept_restart(&room_id, &state.library.device_id())?
    } else {
        let host = state
            .discovery
            .find_device(&room.host_peer_id)
            .ok_or_else(|| "房主当前不在线".to_string())?;
        post_lan_json_response(
            &host,
            "/games/gomoku/restart/accept",
            &serde_json::json!({
                "room_id": room_id,
                "actor_peer_id": state.library.device_id(),
            }),
        )?
    };
    state.game.accept_room(snapshot.clone())?;
    broadcast_if_local_host(&state, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub fn surrender_gomoku(
    state: State<'_, AppState>,
    room_id: String,
) -> Result<GameRoomSnapshot, String> {
    let room = state
        .game
        .find_room(&room_id)
        .ok_or_else(|| "小游戏房间不存在".to_string())?;
    let snapshot = if room.host_peer_id == state.library.device_id() {
        state.game.surrender(&room_id, &state.library.device_id())?
    } else {
        let host = state
            .discovery
            .find_device(&room.host_peer_id)
            .ok_or_else(|| "房主当前不在线".to_string())?;
        post_lan_json_response(
            &host,
            "/games/gomoku/surrender",
            &serde_json::json!({
                "room_id": room_id,
                "actor_peer_id": state.library.device_id(),
            }),
        )?
    };
    state.game.accept_room(snapshot.clone())?;
    broadcast_if_local_host(&state, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub fn list_watch_rooms(state: State<'_, AppState>) -> Vec<WatchRoom> {
    state.watch.list_rooms()
}

#[tauri::command]
pub fn list_watch_chat_messages(
    state: State<'_, AppState>,
    room_id: String,
) -> Vec<WatchChatMessage> {
    state.watch.list_messages(&room_id)
}

#[tauri::command]
pub fn create_watch_room(
    state: State<'_, AppState>,
    title: String,
    is_private: bool,
    password_hash: Option<String>,
) -> Result<WatchRoom, String> {
    let normalized_title = if title.trim().is_empty() {
        format!("{} 的观影房间", state.settings.nickname())
    } else {
        title
    };
    let room = state.watch.create_room(
        normalized_title,
        state.settings.nickname(),
        is_private,
        password_hash,
    )?;
    broadcast_watch_room(&state, &room);
    Ok(room)
}

#[tauri::command]
pub fn join_watch_room(
    app: AppHandle,
    state: State<'_, AppState>,
    room_id: String,
    password_hash: Option<String>,
) -> Result<WatchJoinResponse, String> {
    let room = state
        .watch
        .find_room(&room_id)
        .ok_or_else(|| "观影房间不存在".to_string())?;
    let response = if room.host_device_id == state.library.device_id() {
        let mut response = state.watch.join_room_request(WatchJoinRequest {
            room_id,
            user_id: state.library.device_id(),
            nickname: state.settings.nickname(),
            password_hash,
        })?;
        response.sync = state.watch_player.current_sync(&app);
        response
    } else {
        let host = state
            .discovery
            .find_device(&room.host_device_id)
            .ok_or_else(|| "房主当前不在线".to_string())?;
        post_lan_json_response(
            &host,
            "/watch/rooms/join",
            &WatchJoinRequest {
                room_id: room.room_id.clone(),
                user_id: state.library.device_id(),
                nickname: state.settings.nickname(),
                password_hash,
            },
        )?
    };
    if response.accepted {
        if let Some(next_room) = response.room.clone() {
            state.watch.accept_room(next_room)?;
        }
    }
    Ok(response)
}

#[tauri::command]
pub fn leave_watch_room(state: State<'_, AppState>, room_id: String) -> Result<(), String> {
    let local_id = state.library.device_id();
    let Some(room) = state.watch.find_room(&room_id) else {
        return Ok(());
    };
    if room.host_device_id == local_id {
        state.watch.end_room(&room_id, &local_id)?;
        broadcast_watch_room_end(&state, &room_id);
    } else {
        let host = state
            .discovery
            .find_device(&room.host_device_id)
            .ok_or_else(|| "房主当前不在线".to_string())?;
        let _ = post_lan_json_response::<serde_json::Value, _>(
            &host,
            "/watch/rooms/leave",
            &serde_json::json!({
                "room_id": room_id,
                "user_id": local_id,
            }),
        )?;
        let _ = state
            .watch
            .leave_room(&room.room_id, &state.library.device_id())?;
    }
    Ok(())
}

#[tauri::command]
pub fn end_watch_room(state: State<'_, AppState>, room_id: String) -> Result<(), String> {
    state.watch.end_room(&room_id, &state.library.device_id())?;
    broadcast_watch_room_end(&state, &room_id);
    Ok(())
}

#[tauri::command]
pub fn submit_watch_room_url(
    app: AppHandle,
    state: State<'_, AppState>,
    room_id: String,
    url: String,
) -> Result<WatchRoom, String> {
    let room = state
        .watch
        .update_room_url(&room_id, &state.library.device_id(), url.clone())?;
    state.watch_player.load_url_async(app.clone(), url);
    broadcast_watch_room(&state, &room);
    Ok(room)
}

#[tauri::command]
pub fn get_local_ezmovie_stream() -> Result<Option<LocalEzmovieStreamInfo>, String> {
    fetch_local_ezmovie_stream()
}

#[tauri::command]
pub fn import_local_ezmovie_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    room_id: String,
) -> Result<WatchRoom, String> {
    let stream = fetch_local_ezmovie_stream()?
        .ok_or_else(|| "当前未检测到本机 ezmovie 推流".to_string())?;
    let room = state.watch.import_local_stream(
        &room_id,
        &state.library.device_id(),
        stream.url.clone(),
        Some(stream.lan_preview_url.clone()),
        stream.room_id,
        stream.stream_code,
    )?;
    if let Some(url) = effective_watch_room_url(&state, &room, &state.library.device_id()) {
        state.watch_player.activate(
            &app,
            room.room_id.clone(),
            room.host_device_id.clone(),
            true,
            Some(url),
        )?;
    }
    broadcast_watch_room(&state, &room);
    Ok(room)
}

#[tauri::command]
pub fn send_watch_chat_message(
    state: State<'_, AppState>,
    room_id: String,
    body: String,
) -> Result<WatchChatMessage, String> {
    let message = state.watch.add_chat_message(
        room_id.clone(),
        state.library.device_id(),
        state.settings.nickname(),
        state.settings.avatar_hash(),
        body,
    )?;
    broadcast_watch_chat_message(&state, &message);
    Ok(message)
}

#[tauri::command]
pub async fn activate_watch_room(
    app: AppHandle,
    state: State<'_, AppState>,
    room_id: String,
) -> Result<WatchActivation, String> {
    let room = state
        .watch
        .find_room(&room_id)
        .ok_or_else(|| "观影房间不存在".to_string())?;
    let local_id = state.library.device_id();
    let is_host = room.host_device_id == local_id;
    let is_member = is_host || room.member_ids.iter().any(|id| id == &local_id);
    if is_member {
        state.watch_player.activate(
            &app,
            room.room_id.clone(),
            room.host_device_id.clone(),
            is_host,
            effective_watch_room_url(&state, &room, &local_id),
        )?;
    } else {
        state.watch_player.hide(&app)?;
    }
    Ok(WatchActivation {
        room,
        is_host,
        is_member,
    })
}

#[tauri::command]
pub fn set_watch_webview_bounds(
    app: AppHandle,
    state: State<'_, AppState>,
    bounds: WatchBounds,
) -> Result<(), String> {
    let _ = &state;
    state.watch_player.set_bounds(&app, bounds)
}

#[tauri::command]
pub fn hide_watch_webview(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let _ = &state;
    state.watch_player.hide(&app)
}

#[tauri::command]
pub fn close_watch_webview(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let _ = &state;
    state.watch_player.clear_session(&app)
}

#[tauri::command]
pub fn apply_watch_sync(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: crate::watch::WatchSyncPayload,
) -> Result<(), String> {
    let _ = &state;
    state.watch_player.apply_sync(&app, &payload)
}

#[tauri::command]
pub fn send_files(
    state: State<'_, AppState>,
    target_id: String,
    file_paths: Vec<String>,
) -> Result<String, String> {
    let file_paths = storage::collect_files(file_paths)?
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    let target = state
        .discovery
        .find_device(&target_id)
        .ok_or_else(|| "目标设备不在线".to_string())?;
    state.transfer.send_files(
        target.ip,
        target.tcp_port,
        target.name,
        state.discovery.local_sender(),
        file_paths,
    )
}

#[tauri::command]
pub fn discover_ip(state: State<'_, AppState>, ip: String) -> Result<(), String> {
    state.discovery.probe_ip(ip)
}

#[tauri::command]
pub fn accept_transfer(state: State<'_, AppState>, transfer_id: String) -> Result<(), String> {
    state.transfer.accept(&transfer_id)
}

#[tauri::command]
pub fn reject_transfer(state: State<'_, AppState>, transfer_id: String) -> Result<(), String> {
    state.transfer.reject(&transfer_id)
}

#[tauri::command]
pub fn get_transfers(state: State<'_, AppState>) -> Vec<TransferInfo> {
    state.transfer.list_transfers()
}

#[tauri::command]
pub fn get_transfer(state: State<'_, AppState>, transfer_id: String) -> Option<TransferInfo> {
    state.transfer.get_transfer(&transfer_id)
}

#[tauri::command]
pub fn remove_transfer_record(
    state: State<'_, AppState>,
    transfer_id: String,
) -> Result<(), String> {
    state.transfer.remove_transfer(&transfer_id)
}

#[tauri::command]
pub fn clear_finished_transfers(state: State<'_, AppState>) -> Result<(), String> {
    state.transfer.clear_finished()
}

#[tauri::command]
pub fn get_app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        device_id: state.library.device_id(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub asset_name: Option<String>,
    pub download_url: Option<String>,
    pub asset_size: Option<u64>,
    pub release_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    html_url: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[tauri::command]
pub fn check_for_update() -> Result<UpdateInfo, String> {
    let release = fetch_latest_github_release()?;
    if release.draft || release.prerelease {
        return Err("GitHub 最新发布不是正式版本，请稍后再试".to_string());
    }

    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let latest_version = clean_version(&release.tag_name)
        .or_else(|| release.name.as_deref().and_then(clean_version))
        .ok_or_else(|| "GitHub Release 中未找到可识别的版本号".to_string())?;
    let update_available = compare_versions(&latest_version, &current_version).is_gt();
    let asset = if update_available {
        select_installer_asset(&release.assets)
    } else {
        None
    };
    if update_available && asset.is_none() {
        return Err("发现新版本，但 Release 中没有找到 Windows 安装包".to_string());
    }

    Ok(UpdateInfo {
        current_version,
        latest_version,
        update_available,
        asset_name: asset.as_ref().map(|item| item.name.clone()),
        download_url: asset.as_ref().map(|item| item.browser_download_url.clone()),
        asset_size: asset.as_ref().map(|item| item.size),
        release_url: release.html_url,
    })
}

#[tauri::command]
pub fn install_update(
    app: AppHandle,
    download_url: String,
    asset_name: String,
    expected_size: Option<u64>,
) -> Result<(), String> {
    if !download_url.starts_with("https://github.com/")
        && !download_url.starts_with("https://objects.githubusercontent.com/")
    {
        return Err("更新下载地址不是 GitHub Release 资源".to_string());
    }

    let updates_dir = std::env::temp_dir().join("QuickLAN-updates");
    fs::create_dir_all(&updates_dir).map_err(|err| format!("创建更新临时目录失败: {err}"))?;
    let installer_path = updates_dir.join(storage::safe_file_name(&asset_name));

    download_file_with_powershell(&download_url, &installer_path)?;
    let metadata =
        fs::metadata(&installer_path).map_err(|err| format!("读取安装包信息失败: {err}"))?;
    if metadata.len() == 0 {
        return Err("下载的安装包为空".to_string());
    }
    if let Some(expected_size) = expected_size {
        if expected_size > 0 && metadata.len() != expected_size {
            return Err(format!(
                "安装包大小校验失败: expected {expected_size}, got {}",
                metadata.len()
            ));
        }
    }

    relaunch_installer_after_exit(&installer_path)?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn get_control_api_info(state: State<'_, AppState>) -> ControlApiInfo {
    state.control_api.clone()
}

#[tauri::command]
pub fn get_network_status(state: State<'_, AppState>) -> NetworkStatus {
    state.discovery.network_status()
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.settings.get()
}

#[tauri::command]
pub fn update_nickname(
    state: State<'_, AppState>,
    nickname: String,
) -> Result<AppSettings, String> {
    let next = state.settings.update_nickname(nickname)?;
    state.library.set_device_name(next.nickname.clone());
    state.discovery.broadcast_now();
    state.discovery.emit_devices();
    Ok(next)
}

#[tauri::command]
pub async fn choose_download_dir(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<AppSettings>, String> {
    let Some(folder) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let path = folder
        .into_path()
        .map_err(|_| "请选择本地文件夹路径".to_string())?;
    state.settings.update_download_dir(path).map(Some)
}

#[tauri::command]
pub async fn choose_avatar(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<AppSettings>, String> {
    let Some(file) = app.dialog().file().blocking_pick_file() else {
        return Ok(None);
    };
    let path = file
        .into_path()
        .map_err(|_| "请选择本地图片路径".to_string())?;
    if !path.is_file() {
        return Err("头像必须是本地图片文件".to_string());
    }
    let extension = avatar_extension(&path)?;
    let hash = sha256_file(&path)?;
    fs::create_dir_all(storage::avatar_dir()).map_err(|err| format!("创建头像目录失败: {err}"))?;
    let target = storage::current_avatar_path(extension);
    if !same_file_path(&path, &target) {
        clear_current_avatar_files();
        fs::copy(&path, &target).map_err(|err| format!("保存头像失败: {err}"))?;
    }
    let settings = state.settings.update_avatar(target, hash)?;
    state.discovery.broadcast_now();
    state.discovery.emit_devices();
    Ok(Some(settings))
}

#[tauri::command]
pub async fn choose_share_paths(app: AppHandle) -> Result<Vec<String>, String> {
    let Some(files) = app.dialog().file().blocking_pick_files() else {
        return Ok(Vec::new());
    };
    files
        .into_iter()
        .map(|file| {
            file.into_path()
                .map(|path| path.display().to_string())
                .map_err(|_| "请选择本地文件路径".to_string())
        })
        .collect()
}

#[tauri::command]
pub async fn choose_folder_path(app: AppHandle) -> Result<Option<String>, String> {
    let Some(folder) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    folder
        .into_path()
        .map(|path| Some(path.display().to_string()))
        .map_err(|_| "请选择本地文件夹路径".to_string())
}

#[tauri::command]
pub fn open_path_location(path: String) -> Result<(), String> {
    let path = PathBuf::from(path);
    let target = if path.exists() {
        path.canonicalize().unwrap_or(path)
    } else {
        let mut t = path.parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        while !t.exists() {
            let Some(parent) = t.parent().map(PathBuf::from) else {
                t = PathBuf::from(".");
                break;
            };
            t = parent;
        }
        t.canonicalize().unwrap_or(t)
    };
    open_file_manager(&target)
}

#[tauri::command]
pub fn list_shared_resources(state: State<'_, AppState>) -> Result<Vec<ShareItem>, String> {
    state.library.list_shared_resources()
}

#[tauri::command]
pub fn list_my_shares(state: State<'_, AppState>) -> Result<Vec<ShareItem>, String> {
    state.library.list_my_shares()
}

#[tauri::command]
pub fn add_share_paths(
    state: State<'_, AppState>,
    paths: Vec<String>,
    category: String,
    permission: String,
    password: Option<String>,
) -> Result<Vec<ShareItem>, String> {
    let shares = state
        .library
        .add_share_paths(paths, category, permission, password)?;
    state.discovery.broadcast_now();
    Ok(shares)
}

#[tauri::command]
pub fn update_share(
    state: State<'_, AppState>,
    share_id: String,
    path: String,
) -> Result<ShareItem, String> {
    let share = state.library.update_share(share_id, path)?;
    state.discovery.broadcast_now();
    Ok(share)
}

#[tauri::command]
pub fn remove_share(state: State<'_, AppState>, share_id: String) -> Result<(), String> {
    state.library.remove_share(share_id)?;
    state.discovery.broadcast_now();
    Ok(())
}

#[tauri::command]
pub fn download_share(
    state: State<'_, AppState>,
    share_id: String,
    password: Option<String>,
) -> Result<String, String> {
    state
        .library
        .verify_share_password(&share_id, password.as_deref())?;
    let source = state.library.select_download_source(&share_id)?;
    state
        .transfer
        .download_shared(source, state.discovery.local_sender(), password)
}

#[tauri::command]
pub fn get_library_settings(state: State<'_, AppState>) -> LibrarySettings {
    state.library.settings()
}

#[tauri::command]
pub fn update_library_settings(
    state: State<'_, AppState>,
    settings: LibrarySettings,
) -> Result<LibrarySettings, String> {
    state.library.update_settings(settings)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EzmovieSessionResponse {
    active: bool,
    url: Option<String>,
    preview_url: Option<String>,
    lan_preview_url: Option<String>,
    room_id: Option<String>,
    stream_code: Option<String>,
}

fn fetch_local_ezmovie_stream() -> Result<Option<LocalEzmovieStreamInfo>, String> {
    let Some(address) = ("127.0.0.1", 18333)
        .to_socket_addrs()
        .map_err(|err| format!("解析 ezmovie 本地地址失败: {err}"))?
        .next()
    else {
        return Ok(None);
    };
    let mut stream = match StdTcpStream::connect_timeout(&address, Duration::from_secs(2)) {
        Ok(stream) => stream,
        Err(_) => return Ok(None),
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    stream
        .write_all(
            b"GET /session HTTP/1.1\r\nHost: 127.0.0.1:18333\r\nConnection: close\r\n\r\n",
        )
        .map_err(|err| format!("请求 ezmovie 会话失败: {err}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|err| format!("读取 ezmovie 会话失败: {err}"))?;
    let mut sections = response.splitn(2, "\r\n\r\n");
    let head = sections.next().unwrap_or_default();
    let body = sections
        .next()
        .or_else(|| response.splitn(2, "\n\n").nth(1))
        .unwrap_or_default();
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return Ok(None);
    }
    let session = serde_json::from_str::<EzmovieSessionResponse>(body)
        .map_err(|err| format!("解析 ezmovie 会话失败: {err}"))?;
    if !session.active {
        return Ok(None);
    }
    let lan_host = resolve_ezmovie_lan_host(&session)?;
    let session = EzmovieSessionResponse {
        active: session.active,
        url: Some(format!("http://{lan_host}:18333/")),
        preview_url: Some(format!("http://{lan_host}:18333/")),
        lan_preview_url: Some(format!("http://{lan_host}:18333/")),
        room_id: session.room_id,
        stream_code: session.stream_code,
    };

    let stream_url = session
        .url
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "ezmovie 会话缺少可共享的直播地址".to_string())?;
    let lan_host = extract_http_host(&stream_url)?.to_owned();

    let quick_lan_import_url = format!("http://{lan_host}:18333/");
    validate_local_ezmovie_share_url(&quick_lan_import_url)?;

    let lan_preview_url = session
        .lan_preview_url
        .or(session.preview_url)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "ezmovie 会话缺少局域网预览地址".to_string())?;
    validate_local_ezmovie_share_url(&lan_preview_url)?;

    let room_id = session
        .room_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "ezmovie 会话缺少房间 ID".to_string())?;

    Ok(Some(LocalEzmovieStreamInfo {
        active: true,
        url: quick_lan_import_url.clone(),
        lan_preview_url,
        quick_lan_import_url,
        room_id,
        stream_code: session.stream_code,
    }))
}

fn resolve_ezmovie_lan_host(session: &EzmovieSessionResponse) -> Result<String, String> {
    [
        session.lan_preview_url.as_deref(),
        session.preview_url.as_deref(),
        session.url.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| extract_private_ipv4_host(value).ok())
    .ok_or_else(|| "ezmovie 共享地址必须使用局域网 IPv4 地址".to_string())
}

fn validate_local_ezmovie_share_url(value: &str) -> Result<(), String> {
    let host = extract_http_host(value)?;
    if host == "127.0.0.1" || host == "localhost" {
        return Err("该直播地址仅限本机访问，不能共享给局域网观众".to_string());
    }
    let ip = host
        .parse::<std::net::IpAddr>()
        .map_err(|_| "ezmovie 共享地址必须使用局域网 IPv4 地址".to_string())?;
    if !is_supported_local_share_ipv4(ip) {
        return Err("ezmovie 共享地址必须使用局域网 IPv4 地址".to_string());
    }
    Ok(())
}

fn extract_private_ipv4_host(value: &str) -> Result<String, String> {
    validate_local_ezmovie_share_url(value)?;
    Ok(extract_http_host(value)?.to_string())
}

fn extract_http_host(value: &str) -> Result<&str, String> {
    let rest = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
        .ok_or_else(|| "ezmovie 共享地址必须是 http 或 https".to_string())?;
    let authority = rest
        .split('/')
        .next()
        .filter(|part| !part.trim().is_empty())
        .ok_or_else(|| "ezmovie 直播地址缺少主机名".to_string())?;
    Ok(authority.split(':').next().unwrap_or(authority))
}

fn effective_watch_room_url(
    state: &State<'_, AppState>,
    room: &WatchRoom,
    local_device_id: &str,
) -> Option<String> {
    match room.source_kind {
        crate::watch::WatchSourceKind::Url => room.current_url.clone(),
        crate::watch::WatchSourceKind::LocalEzmovie => {
            let base_url = room
                .stream_preview_url
                .clone()
                .or_else(|| room.stream_url.clone())?;
            if room.host_device_id == local_device_id {
                return Some(base_url);
            }
            let host = state.discovery.find_device(&room.host_device_id)?;
            Some(rewrite_http_url_host(&base_url, &host.ip))
        }
    }
}

fn rewrite_http_url_host(value: &str, host: &str) -> String {
    let (scheme, rest) = if let Some(rest) = value.strip_prefix("http://") {
        ("http://", rest)
    } else if let Some(rest) = value.strip_prefix("https://") {
        ("https://", rest)
    } else {
        return value.to_string();
    };
    let (authority, suffix) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, String::new()),
    };
    let port = authority
        .split_once(':')
        .map(|(_, port)| format!(":{port}"))
        .unwrap_or_default();
    format!("{scheme}{host}{port}{suffix}")
}

fn is_supported_local_share_ipv4(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ipv4) => {
            let [a, b, _, _] = ipv4.octets();
            let is_private =
                a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168);
            let is_cgnat = a == 100 && (64..=127).contains(&b);
            let is_allowed_public = ipv4.octets() == [115, 156, 214, 21];
            is_private || is_cgnat || is_allowed_public
        }
        std::net::IpAddr::V6(_) => false,
    }
}

fn fetch_latest_github_release() -> Result<GithubRelease, String> {
    let script = r#"
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
[Console]::OutputEncoding = [Text.UTF8Encoding]::UTF8
$ProgressPreference = 'SilentlyContinue'
$headers = @{
  'User-Agent' = 'QuickLAN-Updater'
  'Accept' = 'application/vnd.github+json'
  'X-GitHub-Api-Version' = '2022-11-28'
}
Invoke-RestMethod -Headers $headers -Uri 'https://api.github.com/repos/xiaolongsu777/quicklan/releases/latest' |
  ConvertTo-Json -Depth 8 -Compress
"#;
    let output = hidden_powershell()
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .map_err(|err| format!("启动 GitHub 更新检查失败: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "GitHub 更新检查失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let body = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str::<GithubRelease>(body.trim())
        .map_err(|err| format!("解析 GitHub Release 失败: {err}"))
}

fn select_installer_asset(assets: &[GithubAsset]) -> Option<GithubAsset> {
    assets
        .iter()
        .find(|asset| {
            let name = asset.name.to_ascii_lowercase();
            name.starts_with("quicklan_") && name.ends_with("_x64-setup.exe")
        })
        .cloned()
        .or_else(|| {
            assets
                .iter()
                .find(|asset| {
                    let name = asset.name.to_ascii_lowercase();
                    name.ends_with(".exe") && name.contains("setup")
                })
                .cloned()
        })
}

fn download_file_with_powershell(url: &str, path: &Path) -> Result<(), String> {
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'
$headers = @{{ 'User-Agent' = 'QuickLAN-Updater' }}
Invoke-WebRequest -Headers $headers -Uri '{}' -OutFile '{}'
"#,
        ps_quote(url),
        ps_quote(&path.display().to_string())
    );
    let output = hidden_powershell()
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .output()
        .map_err(|err| format!("启动安装包下载失败: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "下载安装包失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn relaunch_installer_after_exit(installer_path: &Path) -> Result<(), String> {
    let script = format!(
        r#"
Start-Sleep -Seconds 1
Start-Process -FilePath '{}' -ArgumentList '/S','/UPDATE','/R'
"#,
        ps_quote(&installer_path.display().to_string())
    );
    hidden_powershell()
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .spawn()
        .map_err(|err| format!("启动更新安装包失败: {err}"))?;
    Ok(())
}

fn hidden_powershell() -> Command {
    let mut command = Command::new("powershell.exe");
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn ps_quote(value: &str) -> String {
    value.replace('\'', "''")
}

fn clean_version(value: &str) -> Option<String> {
    let value = value.trim().trim_start_matches(['v', 'V']);
    let mut version = String::new();
    for c in value.chars() {
        if c.is_ascii_digit() || c == '.' {
            version.push(c);
        } else if !version.is_empty() {
            break;
        }
    }
    if version.split('.').filter(|part| !part.is_empty()).count() >= 2 {
        Some(version)
    } else {
        None
    }
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts = version_parts(left);
    let right_parts = version_parts(right);
    for idx in 0..left_parts.len().max(right_parts.len()) {
        let left = *left_parts.get(idx).unwrap_or(&0);
        let right = *right_parts.get(idx).unwrap_or(&0);
        match left.cmp(&right) {
            std::cmp::Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    std::cmp::Ordering::Equal
}

fn version_parts(value: &str) -> Vec<u64> {
    clean_version(value)
        .unwrap_or_else(|| value.to_string())
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

fn post_chat_message(state: &State<'_, AppState>, payload: &ChatMessagePayload) {
    let targets = chat_targets(state, &payload.room);
    if let Ok(body) = serde_json::to_string(payload) {
        for device in targets {
            let send_body = encrypt_chat_body(&device, &body);
            post_lan_json(&device, "/chat/messages", &send_body);
        }
    }
}

/// 对支持加密的设备用其公钥加密聊天消息，否则回退明文（兼容旧版本设备）。
fn encrypt_chat_body(device: &DeviceInfo, plaintext: &str) -> String {
    match device.public_key.as_deref() {
        Some(peer_key) if !peer_key.trim().is_empty() => {
            match crate::crypto::encrypt_for_peer(peer_key, plaintext) {
                Ok(envelope) => serde_json::to_string(&envelope).unwrap_or_else(|_| plaintext.to_string()),
                Err(_) => plaintext.to_string(),
            }
        }
        _ => plaintext.to_string(),
    }
}

fn post_room_invites(state: &State<'_, AppState>, room: &ChatRoom) {
    let targets = chat_targets(state, room);
    if let Ok(body) = serde_json::to_string(room) {
        for device in targets {
            post_lan_json(&device, "/chat/rooms/invite", &body);
        }
    }
}

fn post_room_delete(state: &State<'_, AppState>, room: &ChatRoom) {
    let targets = chat_targets(state, room);
    let body = serde_json::json!({ "room_id": room.room_id }).to_string();
    for device in targets {
        post_lan_json(&device, "/chat/rooms/delete", &body);
    }
}

fn chat_targets(state: &State<'_, AppState>, room: &ChatRoom) -> Vec<DeviceInfo> {
    let local_id = state.library.device_id();
    state
        .discovery
        .list_devices()
        .into_iter()
        .filter(|device| device.online && device.id != local_id)
        .filter(|device| {
            room.room_id == MAIN_ROOM_ID || room.member_ids.iter().any(|id| id == &device.id)
        })
        .collect()
}

fn broadcast_watch_room(state: &State<'_, AppState>, room: &WatchRoom) {
    let local_id = state.library.device_id();
    if let Ok(body) = serde_json::to_string(room) {
        for device in state
            .discovery
            .list_devices()
            .into_iter()
            .filter(|device| device.online && device.id != local_id)
        {
            post_lan_json(&device, "/watch/rooms/update", &body);
        }
    }
}

pub fn broadcast_local_watch_rooms(state: &AppState) {
    let local_id = state.library.device_id();
    let rooms = state
        .watch
        .list_rooms()
        .into_iter()
        .filter(|room| room.host_device_id == local_id)
        .collect::<Vec<_>>();
    if rooms.is_empty() {
        return;
    }
    let devices = state
        .discovery
        .list_devices()
        .into_iter()
        .filter(|device| device.online && device.id != local_id)
        .collect::<Vec<_>>();
    for room in rooms {
        if let Ok(body) = serde_json::to_string(&room) {
            for device in &devices {
                post_lan_json(device, "/watch/rooms/update", &body);
            }
        }
    }
}

pub fn broadcast_game_room_for_state(state: &AppState, snapshot: &GameRoomSnapshot) {
    let local_id = state.library.device_id();
    if let Ok(body) = serde_json::to_string(snapshot) {
        for device in state
            .discovery
            .list_devices()
            .into_iter()
            .filter(|device| device.online && device.id != local_id)
        {
            post_lan_json(&device, "/games/rooms/update", &body);
        }
    }
}

pub fn broadcast_local_game_rooms(state: &AppState) {
    let rooms = state.game.hosted_rooms();
    if rooms.is_empty() {
        return;
    }
    for snapshot in rooms {
        broadcast_game_room_for_state(state, &snapshot);
    }
}

pub fn broadcast_game_room_end_for_state(state: &AppState, room_id: &str) {
    let local_id = state.library.device_id();
    let body = serde_json::json!({ "room_id": room_id }).to_string();
    for device in state
        .discovery
        .list_devices()
        .into_iter()
        .filter(|device| device.online && device.id != local_id)
    {
        post_lan_json(&device, "/games/rooms/end", &body);
    }
}

pub fn broadcast_watch_room_end_for_state(state: &AppState, room_id: &str) {
    let local_id = state.library.device_id();
    let body = serde_json::json!({ "room_id": room_id }).to_string();
    for device in state
        .discovery
        .list_devices()
        .into_iter()
        .filter(|device| device.online && device.id != local_id)
    {
        post_lan_json(&device, "/watch/rooms/end", &body);
    }
}

fn broadcast_watch_room_end(state: &State<'_, AppState>, room_id: &str) {
    broadcast_watch_room_end_for_state(state.inner(), room_id);
}

fn broadcast_game_room(state: &State<'_, AppState>, snapshot: &GameRoomSnapshot) {
    broadcast_game_room_for_state(state.inner(), snapshot);
}

fn broadcast_game_room_end(state: &State<'_, AppState>, room_id: &str) {
    broadcast_game_room_end_for_state(state.inner(), room_id);
}

fn broadcast_if_local_host(state: &State<'_, AppState>, snapshot: &GameRoomSnapshot) {
    if snapshot.room.host_peer_id == state.library.device_id() {
        broadcast_game_room(state, snapshot);
    }
}

fn broadcast_watch_chat_message(state: &State<'_, AppState>, message: &WatchChatMessage) {
    let local_id = state.library.device_id();
    let Some(room) = state.watch.find_room(&message.room_id) else {
        return;
    };
    if let Ok(body) = serde_json::to_string(message) {
        for device in state
            .discovery
            .list_devices()
            .into_iter()
            .filter(|device| device.online && device.id != local_id)
            .filter(|device| room.member_ids.iter().any(|id| id == &device.id))
        {
            post_lan_json(&device, "/watch/chat/messages", &body);
        }
    }
}

fn post_lan_json(device: &DeviceInfo, path: &str, body: &str) {
    let Some(address) = socket_addr(device) else {
        return;
    };
    if let Ok(mut stream) = StdTcpStream::connect_timeout(&address, Duration::from_millis(900)) {
        let _ = stream.set_write_timeout(Some(Duration::from_millis(900)));
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            device.ip,
            device.api_port,
            body.len()
        );
        let _ = stream.write_all(request.as_bytes());
    }
}

fn post_lan_json_response<T: DeserializeOwned, B: Serialize>(
    device: &DeviceInfo,
    path: &str,
    payload: &B,
) -> Result<T, String> {
    let body = serde_json::to_string(payload).map_err(|err| format!("???????: {err}"))?;
    let address = socket_addr(device).ok_or_else(|| "????????".to_string())?;
    let mut stream = StdTcpStream::connect_timeout(&address, Duration::from_millis(1200))
        .map_err(|err| format!("?????????: {err}"))?;
    let _ = stream.set_write_timeout(Some(Duration::from_millis(1200)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1200)));
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        device.ip,
        device.api_port,
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| format!("??????: {err}"))?;
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .map_err(|err| format!("??????: {err}"))?;
    let raw = String::from_utf8_lossy(&buf);
    let body = raw
        .split("\r\n\r\n")
        .nth(1)
        .or_else(|| raw.split("\n\n").nth(1))
        .unwrap_or_default();
    serde_json::from_str(body).map_err(|err| format!("??????: {err}"))
}

fn socket_addr(device: &DeviceInfo) -> Option<std::net::SocketAddr> {
    (device.ip.as_str(), device.api_port)
        .to_socket_addrs()
        .ok()?
        .next()
}

fn avatar_extension(path: &Path) -> Result<&'static str, String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| "头像文件需要 jpg、png 或 webp 格式".to_string())?;
    match extension.as_str() {
        "jpg" | "jpeg" => Ok("jpg"),
        "png" => Ok("png"),
        "webp" => Ok("webp"),
        _ => Err("头像文件需要 jpg、png 或 webp 格式".to_string()),
    }
}

fn clear_current_avatar_files() {
    for extension in ["jpg", "png", "webp"] {
        let path = storage::current_avatar_path(extension);
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }
}

fn same_file_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|err| format!("打开头像文件失败 {}: {err}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0_u8; crate::protocol::CHUNK_SIZE];
    loop {
        let read = file
            .read(&mut buf)
            .map_err(|err| format!("读取头像文件失败: {err}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
