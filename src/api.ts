import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  AppSettings,
  ChatMessage,
  ChatMessagePayload,
  ChatRoom,
  ControlApiInfo,
  DeviceInfo,
  LibrarySettings,
  NetworkStatus,
  ShareItem,
  TransferInfo,
  UpdateInfo,
  WatchActivation,
  WatchBounds,
  WatchChatMessage,
  WatchJoinResponse,
  WatchRoom,
  WatchSyncPayload,
} from "./types";

export function listDevices(): Promise<DeviceInfo[]> {
  return invoke<DeviceInfo[]>("list_devices");
}

export function updateDeviceNote(deviceId: string, note: string): Promise<DeviceInfo[]> {
  return invoke<DeviceInfo[]>("update_device_note", { deviceId, note });
}

export function listChatRooms(): Promise<ChatRoom[]> {
  return invoke<ChatRoom[]>("list_chat_rooms");
}

export function listChatMessages(roomId: string): Promise<ChatMessage[]> {
  return invoke<ChatMessage[]>("list_chat_messages", { roomId });
}

export function createChatRoom(name: string, memberIds: string[]): Promise<ChatRoom> {
  return invoke<ChatRoom>("create_chat_room", { name, memberIds });
}

export function deleteChatRoom(roomId: string): Promise<void> {
  return invoke<void>("delete_chat_room", { roomId });
}

export function sendChatMessage(roomId: string, body: string): Promise<ChatMessagePayload> {
  return invoke<ChatMessagePayload>("send_chat_message", { roomId, body });
}

export function listWatchRooms(): Promise<WatchRoom[]> {
  return invoke<WatchRoom[]>("list_watch_rooms");
}

export function listWatchChatMessages(roomId: string): Promise<WatchChatMessage[]> {
  return invoke<WatchChatMessage[]>("list_watch_chat_messages", { roomId });
}

export function createWatchRoom(
  title: string,
  isPrivate: boolean,
  passwordHash?: string | null,
): Promise<WatchRoom> {
  return invoke<WatchRoom>("create_watch_room", { title, isPrivate, passwordHash: passwordHash ?? null });
}

export function joinWatchRoom(roomId: string, passwordHash?: string | null): Promise<WatchJoinResponse> {
  return invoke<WatchJoinResponse>("join_watch_room", { roomId, passwordHash: passwordHash ?? null });
}

export function leaveWatchRoom(roomId: string): Promise<void> {
  return invoke<void>("leave_watch_room", { roomId });
}

export function endWatchRoom(roomId: string): Promise<void> {
  return invoke<void>("end_watch_room", { roomId });
}

export function submitWatchRoomUrl(roomId: string, url: string): Promise<WatchRoom> {
  return invoke<WatchRoom>("submit_watch_room_url", { roomId, url });
}

export function sendWatchChatMessage(roomId: string, body: string): Promise<WatchChatMessage> {
  return invoke<WatchChatMessage>("send_watch_chat_message", { roomId, body });
}

export function activateWatchRoom(roomId: string): Promise<WatchActivation> {
  return invoke<WatchActivation>("activate_watch_room", { roomId });
}

export function setWatchWebviewBounds(bounds: WatchBounds): Promise<void> {
  return invoke<void>("set_watch_webview_bounds", { bounds });
}

export function hideWatchWebview(): Promise<void> {
  return invoke<void>("hide_watch_webview");
}

export function closeWatchWebview(): Promise<void> {
  return invoke<void>("close_watch_webview");
}

export function applyWatchSync(payload: WatchSyncPayload): Promise<void> {
  return invoke<void>("apply_watch_sync", { payload });
}

export function sendFiles(targetId: string, filePaths: string[]): Promise<string> {
  return invoke<string>("send_files", { targetId, filePaths });
}

export function acceptTransfer(transferId: string): Promise<void> {
  return invoke<void>("accept_transfer", { transferId });
}

export function rejectTransfer(transferId: string): Promise<void> {
  return invoke<void>("reject_transfer", { transferId });
}

export function getTransfers(): Promise<TransferInfo[]> {
  return invoke<TransferInfo[]>("get_transfers");
}

export function getTransfer(transferId: string): Promise<TransferInfo | null> {
  return invoke<TransferInfo | null>("get_transfer", { transferId });
}

export function getControlApiInfo(): Promise<ControlApiInfo> {
  return invoke<ControlApiInfo>("get_control_api_info");
}

export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("get_app_info");
}

export function checkForUpdate(): Promise<UpdateInfo> {
  return invoke<UpdateInfo>("check_for_update");
}

export function installUpdate(update: UpdateInfo): Promise<void> {
  if (!update.download_url || !update.asset_name) {
    return Promise.reject(new Error("没有可下载的更新安装包"));
  }
  return invoke<void>("install_update", {
    downloadUrl: update.download_url,
    assetName: update.asset_name,
    expectedSize: update.asset_size,
  });
}

export function discoverIp(ip: string): Promise<void> {
  return invoke<void>("discover_ip", { ip });
}

export function getNetworkStatus(): Promise<NetworkStatus> {
  return invoke<NetworkStatus>("get_network_status");
}

export function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("get_settings");
}

export function updateNickname(nickname: string): Promise<AppSettings> {
  return invoke<AppSettings>("update_nickname", { nickname });
}

export function chooseDownloadDir(): Promise<AppSettings | null> {
  return invoke<AppSettings | null>("choose_download_dir");
}

export function chooseAvatar(): Promise<AppSettings | null> {
  return invoke<AppSettings | null>("choose_avatar");
}

export function chooseSharePaths(): Promise<string[]> {
  return invoke<string[]>("choose_share_paths");
}

export function chooseFolderPath(): Promise<string | null> {
  return invoke<string | null>("choose_folder_path");
}

export function openPathLocation(path: string): Promise<void> {
  return invoke<void>("open_path_location", { path });
}

export function removeTransferRecord(transferId: string): Promise<void> {
  return invoke<void>("remove_transfer_record", { transferId });
}

export function clearFinishedTransfers(): Promise<void> {
  return invoke<void>("clear_finished_transfers");
}

export function listSharedResources(): Promise<ShareItem[]> {
  return invoke<ShareItem[]>("list_shared_resources");
}

export function listMyShares(): Promise<ShareItem[]> {
  return invoke<ShareItem[]>("list_my_shares");
}

export function addSharePaths(
  paths: string[],
  category: string,
  permission: string,
  password?: string,
): Promise<ShareItem[]> {
  return invoke<ShareItem[]>("add_share_paths", {
    paths,
    category,
    permission,
    password: password || null,
  });
}

export function updateShare(shareId: string, path: string): Promise<ShareItem> {
  return invoke<ShareItem>("update_share", { shareId, path });
}

export function removeShare(shareId: string): Promise<void> {
  return invoke<void>("remove_share", { shareId });
}

export function downloadShare(shareId: string, password?: string): Promise<string> {
  return invoke<string>("download_share", { shareId, password: password || null });
}

export function getLibrarySettings(): Promise<LibrarySettings> {
  return invoke<LibrarySettings>("get_library_settings");
}

export function updateLibrarySettings(settings: LibrarySettings): Promise<LibrarySettings> {
  return invoke<LibrarySettings>("update_library_settings", { settings });
}
