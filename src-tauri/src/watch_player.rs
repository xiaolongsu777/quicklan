use crate::{
    protocol::DeviceInfo,
    storage,
    watch::{validate_watch_url, WatchSyncPayload},
    AppState,
};
use serde::{Deserialize, Serialize};
use std::{
    net::{TcpStream as StdTcpStream, ToSocketAddrs},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{
    webview::{Color, Url},
    Manager, PhysicalPosition, PhysicalSize, Rect, Runtime, Size, Webview, WebviewUrl,
};
use tokio::time::sleep;

const WATCH_WEBVIEW_LABEL: &str = "watch-player";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayerSnapshot {
    pub found: bool,
    pub current_time: f64,
    pub playback_rate: f64,
    pub is_playing: bool,
}

#[derive(Debug, Clone, Default)]
struct PlayerSession {
    room_id: Option<String>,
    host_device_id: Option<String>,
    is_host: bool,
    last_state: Option<PlayerSnapshot>,
    last_heartbeat: Option<Instant>,
    poll_started: bool,
}

#[derive(Clone, Default)]
pub struct WatchPlayerController {
    session: Arc<Mutex<PlayerSession>>,
}

impl WatchPlayerController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn activate<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        room_id: String,
        host_device_id: String,
        is_host: bool,
        current_url: Option<String>,
    ) -> Result<(), String> {
        {
            let mut session = self.lock()?;
            session.room_id = Some(room_id);
            session.host_device_id = Some(host_device_id);
            session.is_host = is_host;
            session.last_state = None;
            session.last_heartbeat = None;
        }
        let _ = ensure_watch_webview(app)?;
        if let Some(url) = current_url {
            self.load_url_async(app.clone(), url);
        }
        self.ensure_poll_loop(app.clone())?;
        Ok(())
    }

    pub fn clear_session<R: Runtime>(&self, app: &tauri::AppHandle<R>) -> Result<(), String> {
        {
            let mut session = self.lock()?;
            let poll_started = session.poll_started;
            *session = PlayerSession::default();
            session.poll_started = poll_started;
        }
        if let Some(webview) = app.get_webview(WATCH_WEBVIEW_LABEL) {
            let _ = webview.hide();
            let _ = webview.close();
        }
        Ok(())
    }

    pub fn load_url<R: Runtime>(&self, app: &tauri::AppHandle<R>, url: &str) -> Result<(), String> {
        validate_watch_url(url)?;
        let webview = ensure_watch_webview(app)?;
        let parsed = Url::parse(url).map_err(|err| format!("视频链接无效: {err}"))?;
        webview.navigate(parsed).map_err(|err| format!("加载视频页面失败: {err}"))?;
        Ok(())
    }

    pub fn load_url_async<R: Runtime>(&self, app: tauri::AppHandle<R>, url: String) {
        tauri::async_runtime::spawn(async move {
            let _ = validate_watch_url(&url).and_then(|_| {
                let webview = ensure_watch_webview(&app)?;
                let parsed = Url::parse(&url).map_err(|err| format!("??????: {err}"))?;
                webview
                    .navigate(parsed)
                    .map_err(|err| format!("????????: {err}"))
            });
        });
    }

    pub fn set_bounds<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        bounds: WatchBounds,
    ) -> Result<(), String> {
        let Some(webview) = app.get_webview(WATCH_WEBVIEW_LABEL) else {
            return Ok(());
        };
        let width = bounds.width.max(1.0).round() as u32;
        let height = bounds.height.max(1.0).round() as u32;
        let rect = Rect {
            position: tauri::Position::Physical(PhysicalPosition::new(
                bounds.x.round() as i32,
                bounds.y.round() as i32,
            )),
            size: Size::Physical(PhysicalSize::new(width, height)),
        };
        webview
            .set_bounds(rect)
            .map_err(|err| format!("更新播放器位置失败: {err}"))?;
        if bounds.visible {
            webview.show().map_err(|err| format!("显示播放器失败: {err}"))?;
        } else {
            webview.hide().map_err(|err| format!("隐藏播放器失败: {err}"))?;
        }
        Ok(())
    }

    pub fn hide<R: Runtime>(&self, app: &tauri::AppHandle<R>) -> Result<(), String> {
        if let Some(webview) = app.get_webview(WATCH_WEBVIEW_LABEL) {
            webview.hide().map_err(|err| format!("隐藏播放器失败: {err}"))?;
        }
        Ok(())
    }

    pub fn apply_sync<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        payload: &WatchSyncPayload,
    ) -> Result<(), String> {
        let session = self.lock()?.clone();
        if session.is_host {
            return Ok(());
        }
        if session.room_id.as_deref() != Some(payload.room_id.as_str()) {
            return Ok(());
        }
        if session.host_device_id.as_deref() != Some(payload.host_device_id.as_str()) {
            return Ok(());
        }
        let Some(webview) = app.get_webview(WATCH_WEBVIEW_LABEL) else {
            return Ok(());
        };
        webview
            .eval(sync_eval_script(payload))
            .map_err(|err| format!("执行远端同步失败: {err}"))
    }

    pub fn current_sync<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
    ) -> Option<WatchSyncPayload> {
        let session = self.lock().ok()?.clone();
        if !session.is_host {
            return None;
        }
        let room_id = session.room_id?;
        let host_device_id = session.host_device_id?;
        let snapshot = self.snapshot(app).ok()??;
        Some(WatchSyncPayload {
            room_id,
            host_device_id,
            action: "heartbeat".to_string(),
            time: snapshot.current_time,
            playback_rate: snapshot.playback_rate,
            is_playing: snapshot.is_playing,
            sent_at: now_secs(),
        })
    }

    fn ensure_poll_loop<R: Runtime>(&self, app: tauri::AppHandle<R>) -> Result<(), String> {
        let mut session = self.lock()?;
        if session.poll_started {
            return Ok(());
        }
        session.poll_started = true;
        drop(session);

        let controller = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                sleep(Duration::from_millis(800)).await;
                let (room_id, host_device_id, previous_state, last_heartbeat) = {
                    let session = match controller.lock() {
                        Ok(session) => session,
                        Err(_) => continue,
                    };
                    if !session.is_host {
                        continue;
                    }
                    let Some(room_id) = session.room_id.clone() else {
                        continue;
                    };
                    let Some(host_device_id) = session.host_device_id.clone() else {
                        continue;
                    };
                    (
                        room_id,
                        host_device_id,
                        session.last_state.clone(),
                        session.last_heartbeat,
                    )
                };
                let snapshot = match controller.snapshot(&app) {
                    Ok(Some(snapshot)) => snapshot,
                    _ => continue,
                };
                let action = if let Some(previous) = &previous_state {
                    if previous.is_playing != snapshot.is_playing {
                        if snapshot.is_playing {
                            Some("play")
                        } else {
                            Some("pause")
                        }
                    } else if (previous.playback_rate - snapshot.playback_rate).abs() > 0.01 {
                        Some("rate")
                    } else if (previous.current_time - snapshot.current_time).abs() > 2.0 {
                        Some("seek")
                    } else if last_heartbeat
                        .map(|instant| instant.elapsed() >= Duration::from_secs(10))
                        .unwrap_or(true)
                    {
                        Some("heartbeat")
                    } else {
                        None
                    }
                } else {
                    Some("heartbeat")
                };
                let mut to_send = None;
                {
                    let mut session = match controller.lock() {
                        Ok(session) => session,
                        Err(_) => continue,
                    };
                    if !session.is_host
                        || session.room_id.as_deref() != Some(room_id.as_str())
                        || session.host_device_id.as_deref() != Some(host_device_id.as_str())
                    {
                        continue;
                    }
                    session.last_state = Some(snapshot.clone());
                    if let Some(action) = action {
                        session.last_heartbeat = Some(Instant::now());
                        to_send = Some(WatchSyncPayload {
                            room_id,
                            host_device_id,
                            action: action.to_string(),
                            time: snapshot.current_time,
                            playback_rate: snapshot.playback_rate,
                            is_playing: snapshot.is_playing,
                            sent_at: now_secs(),
                        });
                    }
                }
                if let Some(payload) = to_send {
                    let _ = broadcast_sync(&app, &payload);
                }
            }
        });
        Ok(())
    }

    fn snapshot<R: Runtime>(&self, app: &tauri::AppHandle<R>) -> Result<Option<PlayerSnapshot>, String> {
        let Some(webview) = app.get_webview(WATCH_WEBVIEW_LABEL) else {
            return Ok(None);
        };
        let (tx, rx) = mpsc::channel();
        webview
            .eval_with_callback(snapshot_eval_script(), move |value| {
                let parsed = serde_json::from_str::<PlayerSnapshot>(&value).ok();
                let _ = tx.send(parsed);
            })
            .map_err(|err| format!("读取播放器状态失败: {err}"))?;
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|_| "读取播放器状态超时".to_string())
            .map(|value| value.filter(|snapshot| snapshot.found))
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, PlayerSession>, String> {
        self.session
            .lock()
            .map_err(|_| "播放器状态正在被占用".to_string())
    }
}

fn ensure_watch_webview<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<Webview<R>, String> {
    if let Some(webview) = app.get_webview(WATCH_WEBVIEW_LABEL) {
        return Ok(webview);
    }
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let url = WebviewUrl::External("about:blank".parse().expect("valid about:blank url"));
    let webview_builder = tauri::WebviewBuilder::new(WATCH_WEBVIEW_LABEL, url)
        .data_directory(storage::watch_profile_dir())
        .focused(false)
        .zoom_hotkeys_enabled(true)
        .background_color(Color(18, 24, 32, 255))
        .initialization_script(WATCH_INIT_SCRIPT);
    let webview = window
        .add_child(
            webview_builder,
            tauri::LogicalPosition::new(0.0, 0.0),
            tauri::LogicalSize::new(1.0, 1.0),
        )
        .map_err(|err| format!("创建观影播放器失败: {err}"))?;
    webview
        .hide()
        .map_err(|err| format!("初始化隐藏播放器失败: {err}"))?;
    Ok(webview)
}

fn broadcast_sync<R: Runtime>(app: &tauri::AppHandle<R>, payload: &WatchSyncPayload) -> Result<(), String> {
    let state = app.state::<AppState>();
    let Some(room) = state.watch.find_room(&payload.room_id) else {
        return Ok(());
    };
    let body = serde_json::to_string(payload).map_err(|err| format!("序列化同步消息失败: {err}"))?;
    let local_id = state.library.device_id();
    for device in watch_member_devices(&state, &room.member_ids, &local_id) {
        post_lan_json(&device, "/watch/sync", &body);
    }
    Ok(())
}

fn watch_member_devices(
    state: &AppState,
    member_ids: &[String],
    local_id: &str,
) -> Vec<DeviceInfo> {
    state
        .discovery
        .list_devices()
        .into_iter()
        .filter(|device| device.online && device.id != local_id)
        .filter(|device| member_ids.iter().any(|id| id == &device.id))
        .collect()
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
        let _ = std::io::Write::write_all(&mut stream, request.as_bytes());
    }
}

fn socket_addr(device: &DeviceInfo) -> Option<std::net::SocketAddr> {
    (device.ip.as_str(), device.api_port)
        .to_socket_addrs()
        .ok()?
        .next()
}

fn snapshot_eval_script() -> String {
    "(() => window.__quicklanWatch && window.__quicklanWatch.snapshot ? window.__quicklanWatch.snapshot() : { found: false })()".to_string()
}

fn sync_eval_script(payload: &WatchSyncPayload) -> String {
    format!(
        "(() => {{
          const api = window.__quicklanWatch;
          if (!api) return;
          api.seek({time});
          api.setRate({rate});
          if ({playing}) {{
            api.play();
          }} else {{
            api.pause();
          }}
        }})()",
        time = payload.time,
        rate = payload.playback_rate,
        playing = if payload.is_playing { "true" } else { "false" }
    )
}

fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

const WATCH_INIT_SCRIPT: &str = r#"
(() => {
  if (window.__quicklanWatchInstalled) return;
  window.__quicklanWatchInstalled = true;

  function getVideo() {
    return document.querySelector("video");
  }

  function snapshot() {
    const video = getVideo();
    if (!video) {
      return {
        found: false,
        current_time: 0,
        playback_rate: 1,
        is_playing: false
      };
    }
    return {
      found: true,
      current_time: Number(video.currentTime || 0),
      playback_rate: Number(video.playbackRate || 1),
      is_playing: !video.paused
    };
  }

  window.__quicklanWatch = {
    snapshot,
    play() {
      const video = getVideo();
      if (!video) return false;
      const result = video.play();
      if (result && typeof result.catch === "function") {
        result.catch(() => {});
      }
      return true;
    },
    pause() {
      const video = getVideo();
      if (!video) return false;
      video.pause();
      return true;
    },
    seek(seconds) {
      const video = getVideo();
      if (!video) return false;
      video.currentTime = Number(seconds || 0);
      return true;
    },
    setRate(rate) {
      const video = getVideo();
      if (!video) return false;
      video.playbackRate = Number(rate || 1);
      return true;
    }
  };
})();
"#;
