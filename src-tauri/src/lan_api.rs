use crate::{
    chat::{ChatMessagePayload, ChatRoom, ChatService},
    library::LibraryService,
    protocol::Manifest,
    settings::SettingsService,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    net::{TcpStream as StdTcpStream, ToSocketAddrs},
    path::Path,
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[derive(Debug, Deserialize)]
struct CompletedRequest {
    share_id: String,
}

#[derive(Debug, Deserialize)]
struct DeleteRoomRequest {
    room_id: String,
}

pub fn start(
    app: AppHandle,
    library: LibraryService,
    settings: SettingsService,
    chat: ChatService,
    requested_port: u16,
) -> u16 {
    for port in requested_port..requested_port + 20 {
        let bind = format!("0.0.0.0:{port}");
        let app = app.clone();
        let library = library.clone();
        let settings = settings.clone();
        let chat = chat.clone();
        let listener = std::net::TcpListener::bind(&bind);
        let Ok(listener) = listener else {
            continue;
        };
        listener
            .set_nonblocking(true)
            .expect("failed to configure LAN API listener");
        tauri::async_runtime::spawn(async move {
            let listener = match TcpListener::from_std(listener) {
                Ok(listener) => listener,
                Err(err) => {
                    eprintln!("LAN API listener failed on {bind}: {err}");
                    return;
                }
            };
            loop {
                let Ok((stream, _addr)) = listener.accept().await else {
                    continue;
                };
                let app = app.clone();
                let library = library.clone();
                let settings = settings.clone();
                let chat = chat.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = handle_connection(app, library, settings, chat, stream).await;
                });
            }
        });
        return port;
    }
    requested_port
}

async fn handle_connection(
    app: AppHandle,
    library: LibraryService,
    settings: SettingsService,
    chat: ChatService,
    mut stream: TcpStream,
) -> Result<(), String> {
    let mut buf = vec![0_u8; 128 * 1024];
    let len = stream
        .read(&mut buf)
        .await
        .map_err(|err| format!("读取 LAN API 请求失败: {err}"))?;
    if len == 0 {
        return Ok(());
    }
    let request = String::from_utf8_lossy(&buf[..len]);
    let mut lines = request.lines();
    let first = lines.next().unwrap_or_default();
    let parts: Vec<&str> = first.split_whitespace().collect();
    if parts.len() < 2 {
        return write_json(&mut stream, 400, json!({"error":"bad_request"})).await;
    }
    let method = parts[0];
    let path = parts[1];
    let body = request
        .split("\r\n\r\n")
        .nth(1)
        .or_else(|| request.split("\n\n").nth(1))
        .unwrap_or_default();

    match (method, path) {
        ("GET", "/manifest") => {
            let manifest = library.local_manifest()?;
            write_json(&mut stream, 200, json!(manifest)).await
        }
        ("GET", path) if path.starts_with("/avatars/") => {
            let hash = path
                .trim_start_matches("/avatars/")
                .split('?')
                .next()
                .unwrap_or_default();
            write_avatar(&mut stream, &settings, hash).await
        }
        ("GET", path) if path.starts_with("/shares/") => {
            let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
            if parts.len() != 4 || parts[2] != "versions" {
                return write_json(&mut stream, 404, json!({"error":"not_found"})).await;
            }
            let share_id = parts[1];
            let version = parts[3].parse::<i64>().unwrap_or_default();
            let manifest = library.local_manifest()?;
            let item = manifest
                .shares
                .iter()
                .find(|share| share.share_id == share_id)
                .and_then(|share| {
                    share
                        .versions
                        .iter()
                        .find(|item| item.version == version)
                        .map(|version| json!({"share": share, "version": version}))
                });
            match item {
                Some(item) => write_json(&mut stream, 200, item).await,
                None => write_json(&mut stream, 404, json!({"error":"not_found"})).await,
            }
        }
        ("POST", "/downloads/completed") => {
            if let Ok(req) = serde_json::from_str::<CompletedRequest>(body) {
                library.increment_download_count(&req.share_id);
            }
            write_json(&mut stream, 202, json!({"ok":true})).await
        }
        ("POST", "/chat/messages") => {
            if let Ok(payload) = serde_json::from_str::<ChatMessagePayload>(body) {
                if chat.is_local_member(&payload.room) {
                    chat.accept_message(payload.clone())?;
                    let _ = app.emit("chat-message-received", payload);
                }
            }
            write_json(&mut stream, 202, json!({"ok":true})).await
        }
        ("POST", "/chat/rooms/invite") => {
            if let Ok(room) = serde_json::from_str::<ChatRoom>(body) {
                if chat.is_local_member(&room) {
                    chat.accept_room(room.clone())?;
                    let _ = app.emit("chat-room-updated", room);
                }
            }
            write_json(&mut stream, 202, json!({"ok":true})).await
        }
        ("POST", "/chat/rooms/delete") => {
            if let Ok(req) = serde_json::from_str::<DeleteRoomRequest>(body) {
                chat.remove_remote_room(&req.room_id)?;
                let _ = app.emit("chat-room-deleted", req.room_id);
            }
            write_json(&mut stream, 202, json!({"ok":true})).await
        }
        _ => write_json(&mut stream, 404, json!({"error":"not_found"})).await,
    }
}

async fn write_avatar(
    stream: &mut TcpStream,
    settings: &SettingsService,
    hash: &str,
) -> Result<(), String> {
    let app_settings = settings.get();
    if app_settings.avatar_hash.as_deref() != Some(hash) {
        return write_json(stream, 404, json!({"error":"not_found"})).await;
    }
    let Some(path) = app_settings.avatar_path else {
        return write_json(stream, 404, json!({"error":"not_found"})).await;
    };
    let path = Path::new(&path);
    let bytes = fs::read(path).map_err(|err| format!("读取头像失败: {err}"))?;
    let content_type = avatar_content_type(path);
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: max-age=86400\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    stream
        .write_all(header.as_bytes())
        .await
        .map_err(|err| format!("写入头像响应失败: {err}"))?;
    stream
        .write_all(&bytes)
        .await
        .map_err(|err| format!("写入头像内容失败: {err}"))
}

fn avatar_content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
}

async fn write_json(
    stream: &mut TcpStream,
    status: u16,
    payload: serde_json::Value,
) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "OK",
    };
    let body = payload.to_string();
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|err| format!("写入 LAN API 响应失败: {err}"))
}

pub fn fetch_manifest_blocking(ip: &str, port: u16) -> Result<Manifest, String> {
    let mut addrs = (ip, port)
        .to_socket_addrs()
        .map_err(|err| format!("解析 manifest 地址失败: {err}"))?;
    let addr = addrs
        .next()
        .ok_or_else(|| "manifest 地址为空".to_string())?;
    let mut stream = StdTcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .map_err(|err| format!("连接 manifest 失败: {err}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .map_err(|err| format!("设置 manifest 超时失败: {err}"))?;
    let request =
        format!("GET /manifest HTTP/1.1\r\nHost: {ip}:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|err| format!("发送 manifest 请求失败: {err}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|err| format!("读取 manifest 失败: {err}"))?;
    let body = response
        .split("\r\n\r\n")
        .nth(1)
        .or_else(|| response.split("\n\n").nth(1))
        .ok_or_else(|| "manifest 响应无正文".to_string())?;
    serde_json::from_str(body).map_err(|err| format!("解析 manifest 失败: {err}"))
}

pub fn post_download_completed(ip: &str, port: u16, share_id: &str) {
    if let Ok(mut stream) = StdTcpStream::connect((ip, port)) {
        let body = json!({ "share_id": share_id }).to_string();
        let request = format!(
            "POST /downloads/completed HTTP/1.1\r\nHost: {ip}:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(request.as_bytes());
    }
}
