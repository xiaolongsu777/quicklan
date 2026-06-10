mod chat;
mod commands;
mod control_api;
mod discovery;
mod game;
mod lan_api;
mod library;
mod protocol;
mod settings;
mod storage;
mod transfer;
mod watch;
mod watch_player;

use chat::ChatService;
use commands::{
    accept_gomoku_restart, accept_transfer, activate_game_room, activate_watch_room,
    add_share_paths, apply_watch_sync, broadcast_local_watch_rooms,
    broadcast_watch_room_end_for_state, check_for_update, choose_avatar, choose_download_dir,
    choose_folder_path, choose_share_paths, clear_finished_transfers, close_game_room,
    close_watch_webview, create_chat_room, create_game_room, create_watch_room, delete_chat_room,
    discover_ip, download_share, end_watch_room, get_app_info, get_control_api_info,
    get_game_room_state, get_library_settings, get_network_status, get_settings, get_transfer,
    get_transfers, hide_watch_webview, install_update, join_game_room, join_watch_room,
    leave_game_room, leave_watch_room, list_chat_messages, list_chat_rooms, list_devices,
    list_game_rooms, list_my_shares, list_shared_resources, list_watch_chat_messages,
    list_watch_rooms, open_path_location, reject_transfer, remove_share, remove_transfer_record,
    request_gomoku_move, request_gomoku_restart, send_chat_message, send_files,
    send_watch_chat_message, set_watch_webview_bounds, submit_watch_room_url, surrender_gomoku,
    update_device_note, update_library_settings, update_nickname, update_share,
};
use discovery::DiscoveryService;
use game::GameService;
use library::LibraryService;
use protocol::LAN_API_PORT;
use serde::Serialize;
use settings::SettingsService;
use std::{
    collections::HashSet,
    io::{Read, Write},
    net::TcpStream as StdTcpStream,
    time::Duration,
};
use tauri::{
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};
use transfer::TransferService;
use watch::WatchService;
use watch_player::WatchPlayerController;

const CONTROL_API_BIND: &str = "127.0.0.1:45456";

#[derive(Clone, Serialize)]
pub struct ControlApiInfo {
    pub enabled: bool,
    pub bind: String,
}

#[derive(Clone, Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub device_id: String,
}

pub struct AppState {
    pub discovery: DiscoveryService,
    pub transfer: TransferService,
    pub settings: SettingsService,
    pub library: LibraryService,
    pub chat: ChatService,
    pub watch: WatchService,
    pub game: GameService,
    pub watch_player: WatchPlayerController,
    pub control_api: ControlApiInfo,
}

pub(crate) fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text("show", "打开 QuickLAN")
        .separator()
        .text("quit", "退出")
        .build()?;
    let mut builder = TrayIconBuilder::with_id("quicklan-tray")
        .tooltip("QuickLAN")
        .menu(&menu)
        .show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

#[cfg(windows)]
struct SingleInstanceGuard(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn acquire_single_instance() -> Option<SingleInstanceGuard> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
    };

    let name = "Global\\QuickLAN.SingleInstance"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
    if handle.is_null() {
        eprintln!("failed to create QuickLAN single-instance mutex");
        return None;
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = windows_sys::Win32::Foundation::CloseHandle(handle);
        }
        notify_existing_instance();
        return None;
    }
    Some(SingleInstanceGuard(handle))
}

#[cfg(not(windows))]
fn acquire_single_instance() -> Option<()> {
    Some(())
}

fn notify_existing_instance() {
    let Ok(mut stream) = StdTcpStream::connect(CONTROL_API_BIND) else {
        return;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.write_all(
        b"POST /show HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    let mut buf = [0_u8; 128];
    let _ = stream.read(&mut buf);
}

pub fn run() {
    let Some(_single_instance_guard) = acquire_single_instance() else {
        return;
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            setup_tray(app)?;
            let app_handle = app.handle().clone();
            storage::migrate_legacy_data()
                .map_err(|err| format!("failed to migrate local data: {err}"))?;
            let settings = SettingsService::load();
            let device_id = discovery::load_or_create_device_id();
            let library = LibraryService::load(device_id, settings.nickname())
                .map_err(|err| format!("failed to load library: {err}"))?;
            let chat = ChatService::load(library.device_id());
            let watch = WatchService::load(library.device_id());
            let game = GameService::load(library.device_id());
            let api_port = lan_api::start(
                app_handle.clone(),
                library.clone(),
                settings.clone(),
                chat.clone(),
                watch.clone(),
                game.clone(),
                LAN_API_PORT,
            );
            let transfer = TransferService::new(
                app_handle.clone(),
                settings.clone(),
                library.clone(),
                api_port,
            );
            let tcp_port = transfer
                .start_listener()
                .map_err(|err| format!("failed to start TCP listener: {err}"))?;
            let discovery = DiscoveryService::new(
                app_handle.clone(),
                tcp_port,
                api_port,
                settings.clone(),
                library.clone(),
            );

            discovery.start();

            let control_api = ControlApiInfo {
                enabled: true,
                bind: CONTROL_API_BIND.to_string(),
            };

            let state = AppState {
                discovery,
                transfer,
                settings,
                library,
                chat,
                watch,
                game,
                watch_player: WatchPlayerController::new(),
                control_api: control_api.clone(),
            };
            app.manage(state);
            let app_for_watch_broadcast = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut previous_hosted_rooms = HashSet::<String>::new();
                loop {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    if let Some(state) = app_for_watch_broadcast.try_state::<AppState>() {
                        let local_id = state.library.device_id();
                        let current_hosted_rooms = state
                            .watch
                            .list_rooms()
                            .into_iter()
                            .filter(|room| room.host_device_id == local_id)
                            .map(|room| room.room_id)
                            .collect::<HashSet<_>>();
                        for room_id in previous_hosted_rooms.difference(&current_hosted_rooms) {
                            broadcast_watch_room_end_for_state(&state, room_id);
                        }
                        broadcast_local_watch_rooms(&state);
                        previous_hosted_rooms = current_hosted_rooms;
                        let online_ids = state
                            .discovery
                            .list_devices()
                            .into_iter()
                            .filter(|device| device.online)
                            .map(|device| device.id)
                            .collect::<HashSet<_>>();
                        if let Ok(changed_rooms) = state.game.reconcile_hosted_rooms(&online_ids) {
                            for snapshot in changed_rooms {
                                let _ = app_for_watch_broadcast
                                    .emit("game-room-updated", snapshot.clone());
                                crate::commands::broadcast_game_room_for_state(&state, &snapshot);
                            }
                        }
                        crate::commands::broadcast_local_game_rooms(&state);
                    }
                }
            });
            control_api::start(app_handle, control_api.bind.clone());
            Ok(())
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } | TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_devices,
            update_device_note,
            send_files,
            accept_transfer,
            reject_transfer,
            get_transfers,
            get_transfer,
            remove_transfer_record,
            clear_finished_transfers,
            get_app_info,
            get_control_api_info,
            discover_ip,
            get_network_status,
            get_settings,
            choose_avatar,
            update_nickname,
            choose_download_dir,
            choose_share_paths,
            choose_folder_path,
            open_path_location,
            list_shared_resources,
            list_my_shares,
            add_share_paths,
            update_share,
            remove_share,
            download_share,
            get_library_settings,
            update_library_settings,
            check_for_update,
            install_update,
            list_chat_rooms,
            list_chat_messages,
            create_chat_room,
            delete_chat_room,
            send_chat_message,
            list_game_rooms,
            get_game_room_state,
            create_game_room,
            join_game_room,
            leave_game_room,
            close_game_room,
            activate_game_room,
            request_gomoku_move,
            request_gomoku_restart,
            accept_gomoku_restart,
            surrender_gomoku,
            list_watch_rooms,
            list_watch_chat_messages,
            create_watch_room,
            join_watch_room,
            leave_watch_room,
            end_watch_room,
            submit_watch_room_url,
            send_watch_chat_message,
            activate_watch_room,
            set_watch_webview_bounds,
            hide_watch_webview,
            close_watch_webview,
            apply_watch_sync
        ])
        .run(tauri::generate_context!())
        .expect("failed to run QuickLAN");
}
