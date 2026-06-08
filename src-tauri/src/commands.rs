use crate::{
    chat::{ChatMessage, ChatMessagePayload, ChatRoom, MAIN_ROOM_ID},
    protocol::{DeviceInfo, LibrarySettings, NetworkStatus, ShareItem, TransferInfo},
    settings::AppSettings,
    storage, AppInfo, AppState, ControlApiInfo,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream as StdTcpStream,
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

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
pub fn get_app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
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
    if path.is_file() {
        let target = path.canonicalize().unwrap_or(path);
        Command::new("explorer.exe")
            .arg(format!("/select,{}", target.display()))
            .spawn()
            .map_err(|err| format!("打开资源管理器失败: {err}"))?;
    } else {
        let mut target = if path.is_dir() {
            path
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
            post_lan_json(&device, "/chat/messages", &body);
        }
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

fn post_lan_json(device: &DeviceInfo, path: &str, body: &str) {
    if let Ok(mut stream) = StdTcpStream::connect((&*device.ip, device.api_port)) {
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            device.ip,
            device.api_port,
            body.len()
        );
        let _ = stream.write_all(request.as_bytes());
    }
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
