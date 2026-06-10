import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Check,
  Clock,
  Download,
  FilePlus,
  FolderOpen,
  HardDrive,
  Info,
  Laptop,
  Library,
  MessageCircle,
  Network,
  Plus,
  RefreshCw,
  Save,
  Search,
  Send,
  Settings,
  Share2,
  ShieldCheck,
  StickyNote,
  Trash2,
  UploadCloud,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  acceptTransfer,
  acceptGomokuRestart,
  activateGameRoom,
  applyWatchSync,
  addSharePaths,
  checkForUpdate,
  chooseAvatar,
  chooseDownloadDir,
  chooseFolderPath,
  chooseSharePaths,
  createChatRoom,
  createGameRoom,
  createWatchRoom,
  deleteChatRoom,
  discoverIp,
  downloadShare,
  endWatchRoom,
  getAppInfo,
  getControlApiInfo,
  getGameRoomState,
  getLibrarySettings,
  getNetworkStatus,
  getSettings,
  getTransfer,
  getTransfers,
  installUpdate,
  listChatMessages,
  listChatRooms,
  listDevices,
  listGameRooms,
  listMyShares,
  listWatchChatMessages,
  listWatchRooms,
  listSharedResources,
  openPathLocation,
  clearFinishedTransfers,
  closeWatchWebview,
  rejectTransfer,
  removeTransferRecord,
  removeShare,
  activateWatchRoom,
  hideWatchWebview,
  joinWatchRoom,
  leaveWatchRoom,
  leaveGameRoom,
  joinGameRoom,
  closeGameRoom,
  requestGomokuMove,
  requestGomokuRestart,
  sendChatMessage,
  sendFiles,
  sendWatchChatMessage,
  setWatchWebviewBounds,
  submitWatchRoomUrl,
  surrenderGomoku,
  updateDeviceNote,
  updateLibrarySettings,
  updateNickname,
  updateShare,
} from "./api";
import type {
  AppSettings,
  AppInfo,
  ChatMessage,
  ChatMessagePayload,
  ChatRoom,
  ControlApiInfo,
  DeviceInfo,
  GameActivation,
  GameJoinResponse,
  GameRoomSnapshot,
  GameRoomSummary,
  IncomingTransferPayload,
  LibrarySettings,
  NetworkStatus,
  ShareItem,
  TransferInfo,
  TransferPayload,
  UpdateInfo,
  WatchActivation,
  WatchBounds,
  WatchChatMessage,
  WatchJoinResponse,
  WatchRoom,
  WatchSyncPayload,
} from "./types";
import defaultAvatarUrl from "./assets/normal.jpg";
import "./styles.css";

type Tab = "devices" | "store" | "mine" | "chat" | "settings";
type ChatSection = "chat" | "watch" | "game";

function unwrapTransfer(payload: TransferPayload): TransferInfo {
  if ("transfer" in payload) return payload.transfer;
  return payload;
}

export default function App() {
  const params = new URLSearchParams(window.location.search);
  if (params.get("mode") === "incoming") {
    return <IncomingWindow transferId={params.get("transfer_id") ?? ""} />;
  }
  if (params.get("mode") === "watch-empty") {
    return <main className="watch-empty-shell" />;
  }
  return <MainWindow />;
}

function MainWindow() {
  const [tab, setTab] = useState<Tab>("devices");
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [shares, setShares] = useState<ShareItem[]>([]);
  const [myShares, setMyShares] = useState<ShareItem[]>([]);
  const [transfers, setTransfers] = useState<TransferInfo[]>([]);
  const [chatRooms, setChatRooms] = useState<ChatRoom[]>([]);
  const [chatMessages, setChatMessages] = useState<ChatMessage[]>([]);
  const [selectedRoomId, setSelectedRoomId] = useState("main");
  const selectedRoomIdRef = useRef("main");
  const [chatDraft, setChatDraft] = useState("");
  const [chatSection, setChatSection] = useState<ChatSection>("chat");
  const [watchRooms, setWatchRooms] = useState<WatchRoom[]>([]);
  const [gameRooms, setGameRooms] = useState<GameRoomSummary[]>([]);
  const [selectedGameRoomId, setSelectedGameRoomId] = useState<string | null>(null);
  const selectedGameRoomIdRef = useRef<string | null>(null);
  const [gameActivation, setGameActivation] = useState<GameActivation | null>(null);
  const [gameSnapshot, setGameSnapshot] = useState<GameRoomSnapshot | null>(null);
  const [exitedGameRoomIds, setExitedGameRoomIds] = useState<string[]>([]);
  const [gameCreateOpen, setGameCreateOpen] = useState(false);
  const [gameJoinPasswordOpen, setGameJoinPasswordOpen] = useState(false);
  const [gameRoomNameDraft, setGameRoomNameDraft] = useState("");
  const [gamePrivateDraft, setGamePrivateDraft] = useState(false);
  const [gamePasswordDraft, setGamePasswordDraft] = useState("");
  const [gameJoinPassword, setGameJoinPassword] = useState("");
  const [pendingJoinGameRoom, setPendingJoinGameRoom] = useState<GameRoomSummary | null>(null);
  const [watchMessages, setWatchMessages] = useState<WatchChatMessage[]>([]);
  const [selectedWatchRoomId, setSelectedWatchRoomId] = useState<string | null>(null);
  const selectedWatchRoomIdRef = useRef<string | null>(null);
  const [watchDraft, setWatchDraft] = useState("");
  const [watchCreateOpen, setWatchCreateOpen] = useState(false);
  const [watchJoinPasswordOpen, setWatchJoinPasswordOpen] = useState(false);
  const [watchTitleDraft, setWatchTitleDraft] = useState("");
  const [watchPrivateDraft, setWatchPrivateDraft] = useState(false);
  const [watchPasswordDraft, setWatchPasswordDraft] = useState("");
  const [watchJoinPassword, setWatchJoinPassword] = useState("");
  const [pendingJoinRoom, setPendingJoinRoom] = useState<WatchRoom | null>(null);
  const [watchUrlDraft, setWatchUrlDraft] = useState("");
  const [watchActivation, setWatchActivation] = useState<WatchActivation | null>(null);
  const [pendingWatchSync, setPendingWatchSync] = useState<WatchSyncPayload | null>(null);
  const watchViewportRef = useRef<HTMLDivElement | null>(null);
  const [roomNameDraft, setRoomNameDraft] = useState("");
  const [roomMemberDraft, setRoomMemberDraft] = useState<string[]>([]);
  const [roomDialogOpen, setRoomDialogOpen] = useState(false);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [librarySettings, setLibrarySettings] = useState<LibrarySettings | null>(null);
  const [controlApi, setControlApi] = useState<ControlApiInfo | null>(null);
  const [networkStatus, setNetworkStatus] = useState<NetworkStatus | null>(null);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null);
  const [filePaths, setFilePaths] = useState<string[]>([]);
  const [manualIp, setManualIp] = useState("");
  const [nicknameDraft, setNicknameDraft] = useState("");
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState("全部");
  const [sort, setSort] = useState("updated");
  const [shareCategory, setShareCategory] = useState("文档");
  const [sharePermission, setSharePermission] = useState("public");
  const [sharePassword, setSharePassword] = useState("");
  const [downloadPassword, setDownloadPassword] = useState("");
  const [pendingPasswordShare, setPendingPasswordShare] = useState<ShareItem | null>(null);
  const [noteDevice, setNoteDevice] = useState<DeviceInfo | null>(null);
  const [noteDraft, setNoteDraft] = useState("");
  const [transfersOpen, setTransfersOpen] = useState(true);
  const [shareDraftPaths, setShareDraftPaths] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const selectedDevice = devices.find((device) => device.id === selectedDeviceId);
  const selectedRoom = chatRooms.find((room) => room.room_id === selectedRoomId) ?? chatRooms[0];
  const selectedGameRoom = selectedGameRoomId
    ? gameRooms.find((room) => room.room_id === selectedGameRoomId) ?? null
    : null;
  const selectedWatchRoom = selectedWatchRoomId
    ? watchRooms.find((room) => room.room_id === selectedWatchRoomId) ?? null
    : null;
  const onlineCount = devices.filter((device) => device.online).length;

  function findMemberGameRoomId(rooms: GameRoomSummary[], preferredRoomId?: string | null) {
    if (!appInfo?.device_id) return null;
    if (
      preferredRoomId &&
      rooms.some(
        (room) =>
          room.room_id === preferredRoomId &&
          (room.host_peer_id === appInfo.device_id || room.guest_peer_id === appInfo.device_id),
      )
    ) {
      return preferredRoomId;
    }
    return (
      rooms.find(
        (room) => room.host_peer_id === appInfo.device_id || room.guest_peer_id === appInfo.device_id,
      )?.room_id ?? null
    );
  }

  useEffect(() => {
    selectedRoomIdRef.current = selectedRoomId;
  }, [selectedRoomId]);

  useEffect(() => {
    selectedWatchRoomIdRef.current = selectedWatchRoomId;
  }, [selectedWatchRoomId]);

  useEffect(() => {
    selectedGameRoomIdRef.current = selectedGameRoomId;
  }, [selectedGameRoomId]);

  const filteredShares = useMemo(() => {
    const q = search.trim().toLowerCase();
    const result = shares.filter((share) => {
      const matchesCategory = category === "全部" || share.category === category;
      const matchesSearch =
        !q ||
        share.name.toLowerCase().includes(q) ||
        share.owner_name.toLowerCase().includes(q) ||
        share.file_hash.toLowerCase().includes(q);
      return matchesCategory && matchesSearch;
    });
    return [...result].sort((a, b) => {
      if (sort === "size") return b.size - a.size;
      if (sort === "downloads") return b.download_count - a.download_count;
      if (sort === "name") return a.name.localeCompare(b.name);
      return b.updated_at - a.updated_at;
    });
  }, [shares, search, category, sort]);

  const categories = useMemo(
    () => ["全部", ...Array.from(new Set(shares.map((share) => share.category))).sort()],
    [shares],
  );

  useEffect(() => {
    void refreshAll();
    const unsubscribers = [
      listen<DeviceInfo[]>("devices-updated", (event) => setDevices(event.payload)),
      listen<ShareItem[]>("library-updated", (event) => {
        setShares(event.payload);
        void refreshShares();
      }),
      listen<IncomingTransferPayload>("incoming-transfer", (event) => {
        upsertTransfer(event.payload.transfer);
      }),
      listen<TransferPayload>("transfer-progress", (event) => {
        upsertTransfer(unwrapTransfer(event.payload));
      }),
      listen<TransferPayload>("transfer-completed", (event) => {
        upsertTransfer(unwrapTransfer(event.payload));
        void refreshShares();
      }),
      listen<TransferPayload>("transfer-failed", (event) => {
        upsertTransfer(unwrapTransfer(event.payload));
      }),
      listen<ChatMessagePayload>("chat-message-received", (event) => {
        upsertChatMessage(event.payload.message);
        void refreshChatRooms();
      }),
      listen<ChatRoom>("chat-room-updated", (event) => {
        setChatRooms((current) => upsertRoom(current, event.payload));
        setSelectedRoomId((current) => current || event.payload.room_id);
      }),
      listen<string>("chat-room-deleted", (event) => {
        setChatRooms((current) => current.filter((room) => room.room_id !== event.payload));
        setChatMessages((current) => current.filter((message) => message.room_id !== event.payload));
        setSelectedRoomId("main");
      }),
      listen<WatchRoom>("watch-room-updated", (event) => {
        setWatchRooms((current) => upsertWatchRoom(current, event.payload));
        if (selectedWatchRoomIdRef.current === event.payload.room_id) {
          setWatchUrlDraft(event.payload.current_url ?? "");
        }
      }),
      listen<GameRoomSnapshot>("game-room-updated", (event) => {
        setGameRooms((current) => upsertGameRoom(current, event.payload.room));
        if (selectedGameRoomIdRef.current === event.payload.room.room_id) {
          setGameSnapshot(event.payload);
        }
      }),
      listen<GameRoomSnapshot>("game-state-updated", (event) => {
        if (selectedGameRoomIdRef.current === event.payload.room.room_id) {
          setGameSnapshot(event.payload);
        }
        setGameRooms((current) => upsertGameRoom(current, event.payload.room));
      }),
      listen<string>("game-room-deleted", (event) => {
        setGameRooms((current) => current.filter((room) => room.room_id !== event.payload));
        setExitedGameRoomIds((current) => current.filter((roomId) => roomId !== event.payload));
        if (selectedGameRoomIdRef.current === event.payload) {
          setSelectedGameRoomId(null);
          setGameActivation(null);
          setGameSnapshot(null);
        }
      }),
      listen<string>("watch-room-deleted", (event) => {
        setWatchRooms((current) => current.filter((room) => room.room_id !== event.payload));
        setWatchMessages((current) => current.filter((message) => message.room_id !== event.payload));
        if (selectedWatchRoomIdRef.current === event.payload) {
          setSelectedWatchRoomId(null);
          setWatchActivation(null);
          setWatchMessages([]);
          setWatchUrlDraft("");
          void closeWatchWebview();
        }
      }),
      listen<WatchChatMessage>("watch-chat-message-received", (event) => {
        upsertWatchMessage(event.payload);
      }),
      getCurrentWebview().onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type !== "drop" || payload.paths.length === 0) return;
        if (tab === "mine") addShareDraftPaths(payload.paths);
        else addQuickSendFiles(payload.paths);
      }),
    ];

    return () => {
      void Promise.all(unsubscribers).then((items) => {
        items.forEach((unsubscribe) => unsubscribe());
      });
    };
  }, [tab]);

  async function refreshAll() {
    const [deviceList, transferList, status, appSettings, nextLibrarySettings, api, info] =
      await Promise.all([
        listDevices(),
        getTransfers(),
        getNetworkStatus(),
        getSettings(),
        getLibrarySettings(),
        getControlApiInfo(),
        getAppInfo(),
      ]);
    setDevices(deviceList);
    setTransfers(transferList);
    setNetworkStatus(status);
    setSettings(appSettings);
    setNicknameDraft(appSettings.nickname);
    setLibrarySettings(nextLibrarySettings);
    setControlApi(api);
    setAppInfo(info);
    await refreshShares();
    await refreshChat("main");
    await refreshWatch();
    await refreshGame();
  }

  async function refreshShares() {
    const [nextShares, nextMyShares] = await Promise.all([listSharedResources(), listMyShares()]);
    setShares(nextShares);
    setMyShares(nextMyShares);
  }

  async function refreshChat(roomId = selectedRoomId) {
    const rooms = await listChatRooms();
    setChatRooms(rooms);
    const nextRoomId = rooms.some((room) => room.room_id === roomId) ? roomId : rooms[0]?.room_id ?? "main";
    setSelectedRoomId(nextRoomId);
    setChatMessages(await listChatMessages(nextRoomId));
  }

  async function refreshChatRooms() {
    setChatRooms(await listChatRooms());
  }

  async function refreshWatch(roomId = selectedWatchRoomIdRef.current) {
    const rooms = await listWatchRooms();
    setWatchRooms(rooms);
    const nextRoomId = roomId && rooms.some((room) => room.room_id === roomId) ? roomId : null;
    setSelectedWatchRoomId(nextRoomId);
    if (nextRoomId) {
      setWatchMessages(await listWatchChatMessages(nextRoomId));
    } else {
      setWatchMessages([]);
    }
  }

  async function refreshGame(roomId = selectedGameRoomIdRef.current) {
    const rooms = await listGameRooms("gomoku");
    setGameRooms(rooms);
    const preferredRoomId =
      roomId && rooms.some((room) => room.room_id === roomId)
        ? roomId
        : findMemberGameRoomId(rooms, selectedGameRoomIdRef.current);
    const nextRoomId = preferredRoomId && rooms.some((room) => room.room_id === preferredRoomId) ? preferredRoomId : null;
    setSelectedGameRoomId(nextRoomId);
    if (nextRoomId) {
      try {
        setGameSnapshot(await getGameRoomState(nextRoomId));
      } catch {
        setGameSnapshot(null);
      }
    } else {
      setGameSnapshot(null);
    }
  }

  function upsertTransfer(next: TransferInfo) {
    setTransfers((current) => {
      const rest = current.filter((transfer) => transfer.id !== next.id);
      return [next, ...rest].slice(0, 100);
    });
  }

  function upsertChatMessage(next: ChatMessage) {
    setChatMessages((current) => {
      if (next.room_id !== selectedRoomIdRef.current) return current;
      if (current.some((message) => message.message_id === next.message_id)) return current;
      return [...current, next].slice(-100);
    });
  }

  function upsertWatchMessage(next: WatchChatMessage) {
    setWatchMessages((current) => {
      if (next.room_id !== selectedWatchRoomIdRef.current) return current;
      if (current.some((message) => message.message_id === next.message_id)) return current;
      return [...current, next].slice(-200);
    });
  }

  function showToast(message: string) {
    setToast(message);
    window.setTimeout(() => setToast(null), 2400);
  }

  function addQuickSendFiles(paths: string[]) {
    const onlyFiles = paths.filter(Boolean);
    if (onlyFiles.length === 0) return;
    setFilePaths((current) => Array.from(new Set([...current, ...onlyFiles])));
    setError(null);
  }

  function addShareDraftPaths(paths: string[]) {
    const next = paths.filter(Boolean);
    if (next.length === 0) return;
    setShareDraftPaths((current) => Array.from(new Set([...current, ...next])));
    setError(null);
  }

  async function runAction(action: () => Promise<void>, success?: string) {
    try {
      setBusy(true);
      setError(null);
      await action();
      if (success) showToast(success);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function startDownload(share: ShareItem, password?: string) {
    await downloadShare(share.share_id, password);
    setTransfersOpen(true);
  }

  useEffect(() => {
    if (!selectedWatchRoom) {
      setWatchActivation(null);
      setPendingWatchSync(null);
      setWatchMessages([]);
      setWatchUrlDraft("");
      void hideWatchWebview();
      return;
    }
    setWatchUrlDraft(selectedWatchRoom.current_url ?? "");
    void activateWatchRoom(selectedWatchRoom.room_id)
      .then((activation) => {
        setWatchActivation(activation);
        if (!activation.is_member) {
          setWatchMessages([]);
        }
      })
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, [selectedWatchRoom?.room_id, selectedWatchRoom?.current_url]);

  useEffect(() => {
    if (!selectedGameRoom) {
      setGameActivation(null);
      setGameSnapshot(null);
      return;
    }
    if (exitedGameRoomIds.includes(selectedGameRoom.room_id)) {
      setGameActivation(null);
      setGameSnapshot(null);
      return;
    }
    void activateGameRoom(selectedGameRoom.room_id)
      .then(setGameActivation)
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
    void getGameRoomState(selectedGameRoom.room_id)
      .then(setGameSnapshot)
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, [selectedGameRoom?.room_id, exitedGameRoomIds]);

  useEffect(() => {
    if (tab !== "chat" || chatSection !== "game" || selectedGameRoomId) return;
    const memberRoomId = findMemberGameRoomId(gameRooms);
    if (!memberRoomId) return;
    setSelectedGameRoomId(memberRoomId);
  }, [tab, chatSection, selectedGameRoomId, gameRooms, appInfo?.device_id]);

  useEffect(() => {
    if (!selectedWatchRoom?.room_id || !watchActivation?.is_member) {
      setWatchMessages([]);
      return;
    }
    void listWatchChatMessages(selectedWatchRoom.room_id)
      .then(setWatchMessages)
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, [selectedWatchRoom?.room_id, watchActivation?.is_member]);

  useEffect(() => {
    if (!pendingWatchSync || !watchActivation?.is_member || selectedWatchRoom?.room_id !== pendingWatchSync.room_id) {
      return;
    }
    const timer = window.setTimeout(() => {
      void applyWatchSync(pendingWatchSync).catch((err) =>
        setError(err instanceof Error ? err.message : String(err)),
      );
      setPendingWatchSync(null);
    }, 900);
    return () => window.clearTimeout(timer);
  }, [pendingWatchSync, watchActivation?.is_member, selectedWatchRoom?.room_id]);

  useEffect(() => {
    if (tab !== "chat" || chatSection !== "watch") {
      void hideWatchWebview();
      return;
    }
    const element = watchViewportRef.current;
    if (!element || !selectedWatchRoom?.current_url) return;

    let frame = 0;
    const syncBounds = () => {
      frame = 0;
      const rect = element.getBoundingClientRect();
      const scale = window.devicePixelRatio || 1;
      const bounds: WatchBounds = {
        x: rect.left * scale,
        y: rect.top * scale,
        width: rect.width * scale,
        height: rect.height * scale,
        visible: rect.width > 1 && rect.height > 1,
      };
      void setWatchWebviewBounds(bounds).catch((err) =>
        setError(err instanceof Error ? err.message : String(err)),
      );
    };
    const requestSync = () => {
      if (frame) return;
      frame = window.requestAnimationFrame(syncBounds);
    };
    requestSync();
    window.addEventListener("scroll", requestSync, true);
    window.addEventListener("resize", requestSync);
    return () => {
      if (frame) window.cancelAnimationFrame(frame);
      window.removeEventListener("scroll", requestSync, true);
      window.removeEventListener("resize", requestSync);
    };
  }, [tab, chatSection, selectedWatchRoom?.room_id, selectedWatchRoom?.current_url]);

  return (
    <main className="shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">QuickLAN</p>
          <h1>局域网共享与快传</h1>
        </div>
        <div className="status-strip">
          <span>
            <Network size={16} /> {onlineCount} 台在线
          </span>
          <span>
            <Library size={16} /> {shares.length} 个资源
          </span>
          <span>
            <ShieldCheck size={16} /> 去中心化索引
          </span>
        </div>
      </header>

      <nav className="tabs">
        <TabButton active={tab === "devices"} icon={<Laptop size={17} />} onClick={() => setTab("devices")}>
          设备
        </TabButton>
        <TabButton active={tab === "store"} icon={<Library size={17} />} onClick={() => setTab("store")}>
          共享广场
        </TabButton>
        <TabButton active={tab === "mine"} icon={<Share2 size={17} />} onClick={() => setTab("mine")}>
          我的共享
        </TabButton>
        <TabButton active={tab === "chat"} icon={<MessageCircle size={17} />} onClick={() => setTab("chat")}>
          聊天室
        </TabButton>
        <TabButton active={tab === "settings"} icon={<Settings size={17} />} onClick={() => setTab("settings")}>
          设置
        </TabButton>
        <button
          className="icon-button"
          disabled={busy}
          onClick={() => void runAction(refreshAll, "刷新成功")}
          title="刷新"
        >
          <RefreshCw size={17} />
        </button>
      </nav>

      {error && (
        <div className="error">
          <span>{error}</span>
          <button className="icon-button" onClick={() => setError(null)} title="关闭">
            <X size={15} />
          </button>
        </div>
      )}

      {tab === "devices" && (
        <DevicesTab
          devices={devices}
          selectedDeviceId={selectedDeviceId}
          selectedDevice={selectedDevice}
          filePaths={filePaths}
          manualIp={manualIp}
          busy={busy}
          onSelectDevice={setSelectedDeviceId}
          onManualIp={setManualIp}
          onAddFiles={addQuickSendFiles}
          onChooseFiles={() =>
            void runAction(async () => {
              addQuickSendFiles(await chooseSharePaths());
            })
          }
          onChooseFolder={() =>
            void runAction(async () => {
              const folder = await chooseFolderPath();
              if (folder) addQuickSendFiles([folder]);
            })
          }
          onProbe={() =>
            void runAction(async () => {
              if (!manualIp.trim()) throw new Error("请输入对方局域网 IP 地址");
              await discoverIp(manualIp.trim());
              setTimeout(() => void refreshAll(), 500);
            }, "已发送探测广播")
          }
          onEditNote={(device) => {
            setNoteDevice(device);
            setNoteDraft(device.note ?? "");
          }}
          onSend={() =>
            void runAction(async () => {
              if (!selectedDeviceId) throw new Error("请先选择目标设备");
              if (selectedDevice?.is_local) throw new Error("不能向本机快传");
              if (filePaths.length === 0) throw new Error("请先添加要快传的文件");
              await sendFiles(selectedDeviceId, filePaths);
              setFilePaths([]);
            }, "快传任务已创建")
          }
          onRemoveFile={(path) => setFilePaths((items) => items.filter((item) => item !== path))}
        />
      )}

      {tab === "store" && (
        <StoreTab
          shares={filteredShares}
          categories={categories}
          search={search}
          category={category}
          sort={sort}
          busy={busy}
          onSearch={setSearch}
          onCategory={setCategory}
          onSort={setSort}
          onDownload={(share) =>
            void runAction(async () => {
              if (share.permission === "password") {
                setPendingPasswordShare(share);
                setDownloadPassword("");
                return;
              }
              await startDownload(share);
            })
          }
        />
      )}

      {tab === "mine" && (
        <MineTab
          shares={myShares}
          draftPaths={shareDraftPaths}
          category={shareCategory}
          permission={sharePermission}
          password={sharePassword}
          busy={busy}
          onCategory={setShareCategory}
          onPermission={setSharePermission}
          onPassword={setSharePassword}
          onAddDraftPaths={addShareDraftPaths}
          onChoosePaths={() =>
            void runAction(async () => {
              addShareDraftPaths(await chooseSharePaths());
            })
          }
          onChooseFolder={() =>
            void runAction(async () => {
              const folder = await chooseFolderPath();
              if (folder) addShareDraftPaths([folder]);
            })
          }
          onRemoveDraftPath={(path) =>
            setShareDraftPaths((items) => items.filter((item) => item !== path))
          }
          onPublish={() =>
            void runAction(async () => {
              if (shareDraftPaths.length === 0) throw new Error("请先选择要共享的文件");
              await addSharePaths(shareDraftPaths, shareCategory, sharePermission, sharePassword);
              setShareDraftPaths([]);
              await refreshShares();
            }, "共享索引已发布")
          }
          onUpdate={(shareId) =>
            void runAction(async () => {
              const paths = await chooseSharePaths();
              if (paths.length === 0) return;
              await updateShare(shareId, paths[0]);
              await refreshShares();
            }, "共享版本已更新")
          }
          onRemove={(shareId) =>
            void runAction(async () => {
              await removeShare(shareId);
              await refreshShares();
            }, "共享已取消")
          }
        />
      )}

      {tab === "chat" && (
        <ChatPage
          section={chatSection}
          onSectionChange={setChatSection}
          chatContent={
            <ChatTab
              rooms={chatRooms}
              messages={chatMessages}
              selectedRoom={selectedRoom}
              devices={devices}
              draft={chatDraft}
              busy={busy}
              onDraft={setChatDraft}
              onSelectRoom={(roomId) =>
                void runAction(async () => {
                  setSelectedRoomId(roomId);
                  setChatMessages(await listChatMessages(roomId));
                })
              }
              onOpenCreate={() => {
                setRoomNameDraft("");
                setRoomMemberDraft([]);
                setRoomDialogOpen(true);
              }}
              onDeleteRoom={(roomId) =>
                void runAction(async () => {
                  await deleteChatRoom(roomId);
                  await refreshChat("main");
                }, "Chat room deleted")
              }
              onSend={() =>
                void runAction(async () => {
                  if (!selectedRoom) throw new Error("Select a chat room first");
                  const payload = await sendChatMessage(selectedRoom.room_id, chatDraft);
                  setChatDraft("");
                  upsertChatMessage(payload.message);
                })
              }
            />
          }
          watchContent={
            <WatchTab
              localDeviceId={appInfo?.device_id ?? null}
              rooms={watchRooms}
              messages={watchMessages}
              selectedRoom={selectedWatchRoom}
              activation={watchActivation}
              devices={devices}
              draft={watchDraft}
              urlDraft={watchUrlDraft}
              busy={busy}
              viewportRef={watchViewportRef}
              onDraft={setWatchDraft}
              onUrlDraft={setWatchUrlDraft}
              onSelectRoom={(roomId) =>
                void runAction(async () => {
                  setSelectedWatchRoomId(roomId);
                  setWatchMessages([]);
                })
              }
              onOpenCreate={() => {
                setWatchTitleDraft("");
                setWatchPrivateDraft(false);
                setWatchPasswordDraft("");
                setWatchCreateOpen(true);
              }}
              onRoomAction={(room) =>
                void runAction(async () => {
                  if (appInfo?.device_id && room.host_device_id === appInfo.device_id) {
                    return;
                  }
                  if (appInfo?.device_id && room.member_ids.includes(appInfo.device_id)) {
                    await leaveWatchRoom(room.room_id);
                    if (selectedWatchRoomIdRef.current === room.room_id) {
                      setSelectedWatchRoomId(null);
                      setWatchActivation(null);
                      setPendingWatchSync(null);
                      setWatchMessages([]);
                      setWatchUrlDraft("");
                      await closeWatchWebview();
                    }
                    await refreshWatch(selectedWatchRoomIdRef.current === room.room_id ? null : selectedWatchRoomIdRef.current);
                    return;
                  }
                  if (room.is_private) {
                    setPendingJoinRoom(room);
                    setWatchJoinPassword("");
                    setWatchJoinPasswordOpen(true);
                    return;
                  }
                  const joined = await joinWatchRoom(room.room_id, null);
                  if (!joined.accepted || !joined.room) {
                    throw new Error(joined.reason ?? "Join watch room failed");
                  }
                  setSelectedWatchRoomId(joined.room.room_id);
                  await refreshWatch(joined.room.room_id);
                })
              }
              onLeaveRoom={() =>
                void runAction(async () => {
                  if (!selectedWatchRoom) return;
                  await leaveWatchRoom(selectedWatchRoom.room_id);
                  setSelectedWatchRoomId(null);
                  setWatchActivation(null);
                  setPendingWatchSync(null);
                  setWatchMessages([]);
                  setWatchUrlDraft("");
                  await closeWatchWebview();
                  await refreshWatch(null);
                }, "Left watch room")
              }
              onEndRoom={() =>
                void runAction(async () => {
                  if (!selectedWatchRoom) return;
                  await endWatchRoom(selectedWatchRoom.room_id);
                  setSelectedWatchRoomId(null);
                  setWatchActivation(null);
                  setPendingWatchSync(null);
                  setWatchMessages([]);
                  setWatchUrlDraft("");
                  await closeWatchWebview();
                  await refreshWatch(null);
                }, "Watch room ended")
              }
              onSend={() =>
                void runAction(async () => {
                  if (!selectedWatchRoom) throw new Error("Please select a watch room first");
                  const message = await sendWatchChatMessage(selectedWatchRoom.room_id, watchDraft);
                  setWatchDraft("");
                  upsertWatchMessage(message);
                })
              }
              onSubmitUrl={() =>
                void runAction(async () => {
                  if (!selectedWatchRoom) throw new Error("Please select a watch room first");
                  const room = await submitWatchRoomUrl(selectedWatchRoom.room_id, watchUrlDraft);
                  setWatchRooms((current) => upsertWatchRoom(current, room));
                  setWatchUrlDraft(room.current_url ?? "");
                }, "Video link updated")
              }
            />
          }
          gameContent={
            <GameTabFixed
              localDeviceId={appInfo?.device_id ?? null}
              rooms={gameRooms}
              selectedRoom={selectedGameRoom}
              activation={gameActivation}
              snapshot={gameSnapshot}
              exitedRoomIds={exitedGameRoomIds}
              busy={busy}
              onSelectRoom={(roomId) =>
                void runAction(async () => {
                  setSelectedGameRoomId(roomId);
                })
              }
              onOpenCreate={() => {
                setGameRoomNameDraft("");
                setGamePrivateDraft(false);
                setGamePasswordDraft("");
                setGameCreateOpen(true);
              }}
              onRoomAction={(room) =>
                void runAction(async () => {
                  if (appInfo?.device_id && room.host_peer_id === appInfo.device_id) return;
                  if (
                    appInfo?.device_id &&
                    (room.host_peer_id === appInfo.device_id || room.guest_peer_id === appInfo.device_id)
                  ) {
                    await leaveGameRoom(room.room_id);
                    if (selectedGameRoomIdRef.current === room.room_id) {
                      setSelectedGameRoomId(null);
                      setGameActivation(null);
                      setGameSnapshot(null);
                    }
                    setExitedGameRoomIds((current) => current.filter((roomId) => roomId !== room.room_id));
                    await refreshGame(selectedGameRoomIdRef.current === room.room_id ? null : selectedGameRoomIdRef.current);
                    return;
                  }
                  if (room.visibility === "password") {
                    setPendingJoinGameRoom(room);
                    setGameJoinPassword("");
                    setGameJoinPasswordOpen(true);
                    return;
                  }
                  const joined = await joinGameRoom(room.room_id, null, room.host_peer_id);
                  if (!joined.accepted || !joined.snapshot) {
                    throw new Error(joined.reason ?? "Join game room failed");
                  }
                  setExitedGameRoomIds((current) => current.filter((roomId) => roomId !== room.room_id));
                  setSelectedGameRoomId(joined.snapshot.room.room_id);
                  setGameSnapshot(joined.snapshot);
                  await refreshGame(joined.snapshot.room.room_id);
                })
              }
              onLeaveRoom={() =>
                void runAction(async () => {
                  if (!selectedGameRoom) return;
                  await leaveGameRoom(selectedGameRoom.room_id);
                  setExitedGameRoomIds((current) => current.filter((roomId) => roomId !== selectedGameRoom.room_id));
                  setSelectedGameRoomId(null);
                  setGameActivation(null);
                  setGameSnapshot(null);
                  await refreshGame(null);
                }, "已退出当前房间")
              }
              onCloseRoom={() =>
                void runAction(async () => {
                  if (!selectedGameRoom) return;
                  await closeGameRoom(selectedGameRoom.room_id);
                  setSelectedGameRoomId(null);
                  setGameActivation(null);
                  setGameSnapshot(null);
                  await refreshGame(null);
                }, "Game room closed")
              }
              onMove={(x, y) =>
                void runAction(async () => {
                  if (!selectedGameRoom) throw new Error("Please select a game room first");
                  const snapshot = await requestGomokuMove(selectedGameRoom.room_id, x, y);
                  setGameSnapshot(snapshot);
                })
              }
              onRestart={() =>
                void runAction(async () => {
                  if (!selectedGameRoom) throw new Error("Please select a game room first");
                  const snapshot = await requestGomokuRestart(selectedGameRoom.room_id);
                  setGameSnapshot(snapshot);
                })
              }
              onAcceptRestart={() =>
                void runAction(async () => {
                  if (!selectedGameRoom) throw new Error("Please select a game room first");
                  const snapshot = await acceptGomokuRestart(selectedGameRoom.room_id);
                  setGameSnapshot(snapshot);
                })
              }
              onSurrender={() =>
                void runAction(async () => {
                  if (!selectedGameRoom) throw new Error("Please select a game room first");
                  const snapshot = await surrenderGomoku(selectedGameRoom.room_id);
                  setGameSnapshot(snapshot);
                })
              }
            />
          }
        />
      )}

      {tab === "settings" && settings && librarySettings && (
        <SettingsTab
          settings={settings}
          appInfo={appInfo}
          updateInfo={updateInfo}
          librarySettings={librarySettings}
          nicknameDraft={nicknameDraft}
          networkStatus={networkStatus}
          controlApi={controlApi}
          busy={busy}
          onNicknameDraft={setNicknameDraft}
          onSaveNickname={() =>
            void runAction(async () => {
              const next = await updateNickname(nicknameDraft);
              setSettings(next);
              setNicknameDraft(next.nickname);
            }, "昵称已保存")
          }
          onChooseDownloadDir={() =>
            void runAction(async () => {
              const next = await chooseDownloadDir();
              if (next) setSettings(next);
            })
          }
          onChooseAvatar={() =>
            void runAction(async () => {
              const next = await chooseAvatar();
              if (next) {
                setSettings(next);
              }
            }, "头像已保存")
          }
          onOpenDownloadDir={() =>
            void runAction(async () => {
              await openPathLocation(settings.download_dir);
            })
          }
          onUpdateApp={() =>
            void runAction(async () => {
              const info = await checkForUpdate();
              setUpdateInfo(info);
              if (!info.update_available) {
                showToast(`当前已是最新版 ${info.current_version}`);
                return;
              }
              showToast(`发现新版本 ${info.latest_version}，正在下载更新`);
              await installUpdate(info);
            })
          }
          onLibrarySettings={(next) =>
            void runAction(async () => {
              setLibrarySettings(await updateLibrarySettings(next));
            }, "共享设置已保存")
          }
        />
      )}

      <TransfersPanel
        transfers={transfers}
        open={transfersOpen}
        onToggle={() => setTransfersOpen((value) => !value)}
        onRemove={(transferId) =>
          void runAction(async () => {
            await removeTransferRecord(transferId);
            setTransfers(await getTransfers());
          })
        }
        onClearFinished={() =>
          void runAction(async () => {
            await clearFinishedTransfers();
            setTransfers(await getTransfers());
          }, "已清理已结束传输")
        }
      />
      {pendingPasswordShare && (
        <div className="modal-backdrop">
          <div className="modal">
            <h2>输入访问密码</h2>
            <p className="muted">{pendingPasswordShare.name}</p>
            <input
              type="password"
              value={downloadPassword}
              onChange={(event) => setDownloadPassword(event.target.value)}
              placeholder="共享密码"
              autoFocus
            />
            <div className="modal-actions">
              <button
                onClick={() => {
                  setPendingPasswordShare(null);
                  setDownloadPassword("");
                }}
              >
                取消
              </button>
              <button
                className="primary"
                onClick={() =>
                  void runAction(async () => {
                    await startDownload(pendingPasswordShare, downloadPassword);
                    setPendingPasswordShare(null);
                    setDownloadPassword("");
                  }, "下载任务已创建")
                }
              >
                <Download size={16} /> 下载
              </button>
            </div>
          </div>
        </div>
      )}
      {noteDevice && (
        <div className="modal-backdrop">
          <div className="modal">
            <h2>编辑备注</h2>
            <p className="muted">{noteDevice.name}</p>
            <input
              value={noteDraft}
              onChange={(event) => setNoteDraft(event.target.value)}
              placeholder="给这台设备添加本地备注"
              autoFocus
              maxLength={48}
            />
            <div className="modal-actions">
              <button
                onClick={() => {
                  setNoteDevice(null);
                  setNoteDraft("");
                }}
              >
                取消
              </button>
              <button
                className="primary"
                onClick={() =>
                  void runAction(async () => {
                    setDevices(await updateDeviceNote(noteDevice.id, noteDraft));
                    setNoteDevice(null);
                    setNoteDraft("");
                  }, "备注已保存")
                }
              >
                <Save size={16} /> 保存
              </button>
            </div>
          </div>
        </div>
      )}
      {watchJoinPasswordOpen && pendingJoinRoom && (
        <div className="modal-backdrop">
          <div className="modal">
            <h2>加入私密观影房间</h2>
            <p className="muted">{pendingJoinRoom.title}</p>
            <input
              type="password"
              value={watchJoinPassword}
              onChange={(event) => setWatchJoinPassword(event.target.value)}
              placeholder="输入房间密码"
              autoFocus
            />
            <div className="modal-actions">
              <button
                onClick={() => {
                  setWatchJoinPasswordOpen(false);
                  setPendingJoinRoom(null);
                  setWatchJoinPassword("");
                }}
              >
                取消
              </button>
              <button
                className="primary"
                onClick={() =>
                  void runAction(async () => {
                    const joined = await joinWatchRoom(
                      pendingJoinRoom.room_id,
                      await sha256Text(watchJoinPassword),
                    );
                    if (!joined.accepted || !joined.room) {
                      throw new Error(joined.reason ?? "加入观影房间失败");
                    }
                    setPendingWatchSync(joined.sync ?? null);
                    setWatchJoinPasswordOpen(false);
                    setPendingJoinRoom(null);
                    setWatchJoinPassword("");
                    setSelectedWatchRoomId(joined.room.room_id);
                    await refreshWatch(joined.room.room_id);
                  }, "已加入观影房间")
                }
              >
                加入
              </button>
            </div>
          </div>
        </div>
      )}
      {watchCreateOpen && (
        <div className="modal-backdrop">
          <div className="modal">
            <h2>创建观影房间</h2>
            <input
              value={watchTitleDraft}
              onChange={(event) => setWatchTitleDraft(event.target.value)}
              placeholder="房间名称（可选）"
              autoFocus
              maxLength={48}
            />
            <label>
              <span>访问方式</span>
              <select
                value={watchPrivateDraft ? "private" : "public"}
                onChange={(event) => setWatchPrivateDraft(event.target.value === "private")}
              >
                <option value="public">公开房间</option>
                <option value="private">私密房间</option>
              </select>
            </label>
            {watchPrivateDraft && (
              <input
                type="password"
                value={watchPasswordDraft}
                onChange={(event) => setWatchPasswordDraft(event.target.value)}
                placeholder="房间密码"
              />
            )}
            <div className="modal-actions">
              <button
                onClick={() => {
                  setWatchCreateOpen(false);
                  setWatchTitleDraft("");
                  setWatchPrivateDraft(false);
                  setWatchPasswordDraft("");
                }}
              >
                取消
              </button>
              <button
                className="primary"
                onClick={() =>
                  void runAction(async () => {
                    const room = await createWatchRoom(
                      watchTitleDraft,
                      watchPrivateDraft,
                      watchPrivateDraft ? await sha256Text(watchPasswordDraft) : null,
                    );
                    setWatchCreateOpen(false);
                    setWatchTitleDraft("");
                    setWatchPrivateDraft(false);
                    setWatchPasswordDraft("");
                    setChatSection("watch");
                    setSelectedWatchRoomId(room.room_id);
                    await refreshWatch(room.room_id);
                  }, "观影房间已创建")
                }
              >
                <Plus size={16} /> 创建
              </button>
            </div>
          </div>
        </div>
      )}
      {gameJoinPasswordOpen && pendingJoinGameRoom && (
        <div className="modal-backdrop">
          <div className="modal">
            <h2>加入密码五子棋房间</h2>
            <p className="muted">{pendingJoinGameRoom.room_name}</p>
            <input
              type="password"
              value={gameJoinPassword}
              onChange={(event) => setGameJoinPassword(event.target.value)}
              placeholder="输入房间密码"
              autoFocus
            />
            <div className="modal-actions">
              <button
                onClick={() => {
                  setGameJoinPasswordOpen(false);
                  setPendingJoinGameRoom(null);
                  setGameJoinPassword("");
                }}
              >
                取消
              </button>
              <button
                className="primary"
                onClick={() =>
                  void runAction(async () => {
                    const joined: GameJoinResponse = await joinGameRoom(
                      pendingJoinGameRoom.room_id,
                      await sha256Text(gameJoinPassword),
                      pendingJoinGameRoom.host_peer_id,
                    );
                    if (!joined.accepted || !joined.snapshot) {
                      throw new Error(joined.reason ?? "?????????");
                    }
                    setGameJoinPasswordOpen(false);
                    setPendingJoinGameRoom(null);
                    setGameJoinPassword("");
                    setChatSection("game");
                    setSelectedGameRoomId(joined.snapshot.room.room_id);
                    setGameSnapshot(joined.snapshot);
                    await refreshGame(joined.snapshot.room.room_id);
                  }, "????????")
                }
              >
                加入
              </button>
            </div>
          </div>
        </div>
      )}
      {gameCreateOpen && (
        <div className="modal-backdrop">
          <div className="modal">
            <h2>创建五子棋房间</h2>
            <input
              value={gameRoomNameDraft}
              onChange={(event) => setGameRoomNameDraft(event.target.value)}
              placeholder="房间名称（可选）"
              autoFocus
              maxLength={48}
            />
            <label>
              <span>访问方式</span>
              <select
                value={gamePrivateDraft ? "password" : "public"}
                onChange={(event) => setGamePrivateDraft(event.target.value === "password")}
              >
                <option value="public">公开房间</option>
                <option value="password">密码房间</option>
              </select>
            </label>
            {gamePrivateDraft && (
              <input
                type="password"
                value={gamePasswordDraft}
                onChange={(event) => setGamePasswordDraft(event.target.value)}
                placeholder="房间密码"
              />
            )}
            <div className="modal-actions">
              <button
                onClick={() => {
                  setGameCreateOpen(false);
                  setGameRoomNameDraft("");
                  setGamePrivateDraft(false);
                  setGamePasswordDraft("");
                }}
              >
                取消
              </button>
              <button
                className="primary"
                onClick={() =>
                  void runAction(async () => {
                    const snapshot = await createGameRoom(
                      gameRoomNameDraft,
                      gamePrivateDraft ? "password" : "public",
                      gamePrivateDraft ? await sha256Text(gamePasswordDraft) : null,
                    );
                    setGameCreateOpen(false);
                    setGameRoomNameDraft("");
                    setGamePrivateDraft(false);
                    setGamePasswordDraft("");
                    setChatSection("game");
                    setSelectedGameRoomId(snapshot.room.room_id);
                    setGameSnapshot(snapshot);
                    await refreshGame(snapshot.room.room_id);
                  }, "五子棋房间已创建")
                }
              >
                <Plus size={16} /> 创建
              </button>
            </div>
          </div>
        </div>
      )}
      {roomDialogOpen && (
        <div className="modal-backdrop">
          <div className="modal">
            <h2>创建聊天室</h2>
            <input
              value={roomNameDraft}
              onChange={(event) => setRoomNameDraft(event.target.value)}
              placeholder="聊天室名称"
              autoFocus
              maxLength={32}
            />
            <div className="member-picker">
              {devices.filter((device) => device.online).length === 0 ? (
                <p className="muted">暂无可邀请的在线设备</p>
              ) : (
                devices
                  .filter((device) => device.online)
                  .map((device) => (
                    <label className="member-option" key={device.id}>
                      <input
                        type="checkbox"
                        checked={roomMemberDraft.includes(device.id)}
                        onChange={(event) => {
                          setRoomMemberDraft((current) =>
                            event.target.checked
                              ? [...current, device.id]
                              : current.filter((id) => id !== device.id),
                          );
                        }}
                      />
                      <span>{device.note ? `${device.note} (${device.name})` : device.name}</span>
                    </label>
                  ))
              )}
            </div>
            <div className="modal-actions">
              <button
                onClick={() => {
                  setRoomDialogOpen(false);
                  setRoomMemberDraft([]);
                }}
              >
                取消
              </button>
              <button
                className="primary"
                disabled={busy}
                onClick={() =>
                  void runAction(async () => {
                    const room = await createChatRoom(roomNameDraft, roomMemberDraft);
                    setRoomDialogOpen(false);
                    setRoomNameDraft("");
                    setRoomMemberDraft([]);
                    await refreshChat(room.room_id);
                  }, "聊天室已创建")
                }
              >
                <Plus size={16} /> 创建
              </button>
            </div>
          </div>
        </div>
      )}
      {toast && <div className="toast">{toast}</div>}
    </main>
  );
}

function TabButton({
  active,
  icon,
  children,
  onClick,
}: {
  active: boolean;
  icon: React.ReactNode;
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button className={`tab ${active ? "active" : ""}`} onClick={onClick}>
      {icon}
      {children}
    </button>
  );
}

function DevicesTab(props: {
  devices: DeviceInfo[];
  selectedDeviceId: string | null;
  selectedDevice?: DeviceInfo;
  filePaths: string[];
  manualIp: string;
  busy: boolean;
  onSelectDevice: (id: string) => void;
  onManualIp: (value: string) => void;
  onAddFiles: (paths: string[]) => void;
  onChooseFiles: () => void;
  onChooseFolder: () => void;
  onProbe: () => void;
  onEditNote: (device: DeviceInfo) => void;
  onSend: () => void;
  onRemoveFile: (path: string) => void;
}) {
  return (
    <section className="grid two">
      <section className="panel devices-panel">
        <div className="panel-title">
          <h2>在线设备</h2>
          <span className="pill">{props.devices.length} 台</span>
        </div>
        <div className="manual-probe">
          <input
            value={props.manualIp}
            onChange={(event) => props.onManualIp(event.target.value)}
            placeholder="手动探测 IP，如 192.168.1.23"
          />
          <button className="icon-button" onClick={props.onProbe} title="探测 IP">
            <Search size={16} />
          </button>
        </div>
        <div className="device-list">
          {props.devices.length === 0 ? (
            <Empty icon={<Laptop size={34} />} text="等待局域网内的 QuickLAN 设备出现" />
          ) : (
            props.devices.map((device) => (
              <div
                key={device.id}
                className={`device ${props.selectedDeviceId === device.id ? "selected" : ""}`}
              >
                <button className="device-main" onClick={() => props.onSelectDevice(device.id)}>
                  <span className="device-avatar-wrap">
                    <img className="avatar" src={deviceAvatarSrc(device)} alt="" onError={useDefaultAvatar} />
                    <span className={`dot ${device.online ? "online" : ""}`} />
                  </span>
                  <span>
                    <strong>{device.note ? `${device.note}（${device.name}）` : device.name}</strong>
                    <small>
                      {device.is_local ? "本机" : `${device.ip}:${device.tcp_port}`} · {device.share_count} 个资源
                    </small>
                  </span>
                </button>
                <button className="icon-button" onClick={() => props.onEditNote(device)} title="编辑备注">
                  <StickyNote size={15} />
                </button>
              </div>
            ))
          )}
        </div>
      </section>

      <section className="panel stack">
        <div className="panel-title">
          <h2>文件快传</h2>
          <button className="secondary" onClick={props.onChooseFiles}>
            <FilePlus size={16} /> 选择文件
          </button>
          <button className="secondary" onClick={props.onChooseFolder}>
            <FolderOpen size={16} /> 文件夹
          </button>
        </div>
        <Dropzone
          icon={<UploadCloud size={42} />}
          title="拖入文件进行点对点快传"
          text="接收方确认后开始传输，完成后进行 SHA256 校验。"
          onDrop={props.onAddFiles}
        />
        <div className="sendbar">
          <div>
            <strong>{props.selectedDevice ? props.selectedDevice.name : "未选择设备"}</strong>
            <span>{props.selectedDevice?.is_local ? "不能向本机快传" : props.filePaths.length > 0 ? `已选择 ${props.filePaths.length} 个文件` : "添加文件并选择设备后即可发送"}</span>
          </div>
          <button className="primary" disabled={props.busy || !!props.selectedDevice?.is_local} onClick={props.onSend}>
            <Send size={17} /> 发送
          </button>
        </div>
        <PathList paths={props.filePaths} onRemove={props.onRemoveFile} />
      </section>
    </section>
  );
}

function StoreTab(props: {
  shares: ShareItem[];
  categories: string[];
  search: string;
  category: string;
  sort: string;
  busy: boolean;
  onSearch: (value: string) => void;
  onCategory: (value: string) => void;
  onSort: (value: string) => void;
  onDownload: (share: ShareItem) => void;
}) {
  return (
    <section className="panel stack">
      <div className="toolbar">
        <div className="searchbox">
          <Search size={16} />
          <input value={props.search} onChange={(event) => props.onSearch(event.target.value)} placeholder="搜索文件、分享者或 hash" />
        </div>
        <select value={props.category} onChange={(event) => props.onCategory(event.target.value)}>
          {props.categories.map((item) => (
            <option key={item}>{item}</option>
          ))}
        </select>
        <select value={props.sort} onChange={(event) => props.onSort(event.target.value)}>
          <option value="updated">最近更新</option>
          <option value="downloads">下载次数</option>
          <option value="size">文件大小</option>
          <option value="name">文件名</option>
        </select>
      </div>
      <div className="resource-list">
        {props.shares.length === 0 ? (
          <Empty icon={<Library size={34} />} text="暂无可下载的共享资源" />
        ) : (
          props.shares.map((share) => (
            <ShareRow
              key={share.share_id}
              share={share}
              action={
                <button className="primary compact" disabled={props.busy} onClick={() => props.onDownload(share)}>
                  <Download size={16} /> 下载
                </button>
              }
            />
          ))
        )}
      </div>
    </section>
  );
}

function MineTab(props: {
  shares: ShareItem[];
  draftPaths: string[];
  category: string;
  permission: string;
  password: string;
  busy: boolean;
  onCategory: (value: string) => void;
  onPermission: (value: string) => void;
  onPassword: (value: string) => void;
  onAddDraftPaths: (paths: string[]) => void;
  onChoosePaths: () => void;
  onChooseFolder: () => void;
  onRemoveDraftPath: (path: string) => void;
  onPublish: () => void;
  onUpdate: (shareId: string) => void;
  onRemove: (shareId: string) => void;
}) {
  return (
    <section className="grid two">
      <section className="panel stack">
        <div className="panel-title">
          <h2>发布共享索引</h2>
          <button className="secondary" onClick={props.onChoosePaths}>
            <FilePlus size={16} /> 选择文件
          </button>
          <button className="secondary" onClick={props.onChooseFolder}>
            <FolderOpen size={16} /> 文件夹
          </button>
        </div>
        <Dropzone
          icon={<HardDrive size={42} />}
          title="拖入文件加入 Shared Store"
          text="发布时会创建共享副本，原始文件后续修改不会影响已共享版本。"
          onDrop={props.onAddDraftPaths}
        />
        <div className="form-grid">
          <label>
            分类
            <select value={props.category} onChange={(event) => props.onCategory(event.target.value)}>
              <option>文档</option>
              <option>图片</option>
              <option>视频</option>
              <option>软件</option>
              <option>其他</option>
            </select>
          </label>
          <label>
            权限
            <select value={props.permission} onChange={(event) => props.onPermission(event.target.value)}>
              <option value="public">公开共享</option>
              <option value="password">密码共享</option>
            </select>
          </label>
          {props.permission === "password" && (
            <label>
              密码
              <input value={props.password} onChange={(event) => props.onPassword(event.target.value)} placeholder="访问密码" />
            </label>
          )}
        </div>
        <PathList paths={props.draftPaths} onRemove={props.onRemoveDraftPath} />
        <button className="primary" disabled={props.busy} onClick={props.onPublish}>
          <Share2 size={17} /> 发布共享
        </button>
      </section>

      <section className="panel stack">
        <div className="panel-title">
          <h2>我的共享</h2>
          <span className="pill">{props.shares.length} 个</span>
        </div>
        <div className="resource-list">
          {props.shares.length === 0 ? (
            <Empty icon={<Share2 size={34} />} text="还没有发布共享资源" />
          ) : (
            props.shares.map((share) => (
              <ShareRow
                key={share.share_id}
                share={share}
                action={
                  <div className="row-actions">
                    <button className="secondary compact" onClick={() => props.onUpdate(share.share_id)}>
                      <RefreshCw size={15} /> 更新
                    </button>
                    <button className="icon-button danger" onClick={() => props.onRemove(share.share_id)} title="取消共享">
                      <Trash2 size={15} />
                    </button>
                  </div>
                }
              />
            ))
          )}
        </div>
      </section>
    </section>
  );
}

function ChatTab(props: {
  rooms: ChatRoom[];
  messages: ChatMessage[];
  selectedRoom?: ChatRoom;
  devices: DeviceInfo[];
  draft: string;
  busy: boolean;
  onDraft: (value: string) => void;
  onSelectRoom: (roomId: string) => void;
  onOpenCreate: () => void;
  onDeleteRoom: (roomId: string) => void;
  onSend: () => void;
}) {
  const messagesRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const element = messagesRef.current;
    if (!element) return;
    element.scrollTop = element.scrollHeight;
  }, [props.selectedRoom?.room_id, props.messages.length]);

  return (
    <section className="chat-layout">
      <section className="panel stack chat-sidebar">
        <div className="panel-title">
          <h2>聊天室</h2>
          <button className="secondary compact" onClick={props.onOpenCreate}>
            <Plus size={15} /> 创建
          </button>
        </div>
        <div className="chat-room-list">
          {props.rooms.map((room) => (
            <button
              key={room.room_id}
              className={`chat-room ${props.selectedRoom?.room_id === room.room_id ? "active" : ""}`}
              onClick={() => props.onSelectRoom(room.room_id)}
            >
              <MessageCircle size={17} />
              <span>
                <strong>{room.name}</strong>
                <small>{room.is_main ? "所有在线用户" : `${room.member_ids.length} 位成员`}</small>
              </span>
            </button>
          ))}
        </div>
      </section>

      <section className="panel stack chat-panel">
        {props.selectedRoom ? (
          <>
            <div className="panel-title">
              <div>
                <h2>{props.selectedRoom.name}</h2>
                <p className="muted">{props.selectedRoom.is_main ? "主聊天室" : "私密聊天室"}</p>
              </div>
              {!props.selectedRoom.is_main && (
                <button
                  className="icon-button danger"
                  title="删除聊天室"
                  onClick={() => props.onDeleteRoom(props.selectedRoom!.room_id)}
                >
                  <Trash2 size={15} />
                </button>
              )}
            </div>
            <div className="chat-messages" ref={messagesRef}>
              {props.messages.length === 0 ? (
                <Empty icon={<MessageCircle size={34} />} text="暂无消息" />
              ) : (
                props.messages.map((message) => (
                  <article className="chat-message" key={message.message_id}>
                    <img className="avatar" src={messageAvatarSrc(message, props.devices)} alt="" onError={useDefaultAvatar} />
                    <div>
                      <div className="chat-message-head">
                        <strong>{messageSenderName(message, props.devices)}</strong>
                        <span>{formatDateTime(message.created_at)}</span>
                      </div>
                      <p>{message.body}</p>
                    </div>
                  </article>
                ))
              )}
            </div>
            <div className="chat-input">
              <textarea
                value={props.draft}
                onChange={(event) => props.onDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && !event.shiftKey) {
                    event.preventDefault();
                    props.onSend();
                  }
                }}
                placeholder="输入消息"
                rows={2}
              />
              <button className="primary" disabled={props.busy || !props.draft.trim()} onClick={props.onSend}>
                <Send size={16} /> 发送
              </button>
            </div>
          </>
        ) : (
          <Empty icon={<MessageCircle size={34} />} text="暂无聊天室" />
        )}
      </section>
    </section>
  );
}

function ChatPage(props: {
  section: ChatSection;
  onSectionChange: (value: ChatSection) => void;
  chatContent: React.ReactNode;
  watchContent: React.ReactNode;
  gameContent: React.ReactNode;
}) {
  return (
    <section className="stack">
      <div className="subtabs">
        <button
          className={`tab ${props.section === "chat" ? "active" : ""}`}
          onClick={() => props.onSectionChange("chat")}
        >
          聊天
        </button>
        <button
          className={`tab ${props.section === "watch" ? "active" : ""}`}
          onClick={() => props.onSectionChange("watch")}
        >
          观影
        </button>
        <button
          className={`tab ${props.section === "game" ? "active" : ""}`}
          onClick={() => props.onSectionChange("game")}
        >
          小游戏
        </button>
      </div>
      <div className={props.section === "chat" ? "" : "section-hidden"}>{props.chatContent}</div>
      <div className={props.section === "watch" ? "" : "section-hidden"}>{props.watchContent}</div>
      <div className={props.section === "game" ? "" : "section-hidden"}>{props.gameContent}</div>
    </section>
  );
}

function WatchTab(props: {
  localDeviceId: string | null;
  rooms: WatchRoom[];
  messages: WatchChatMessage[];
  selectedRoom: WatchRoom | null;
  activation: WatchActivation | null;
  devices: DeviceInfo[];
  draft: string;
  urlDraft: string;
  busy: boolean;
  viewportRef: React.RefObject<HTMLDivElement>;
  onDraft: (value: string) => void;
  onUrlDraft: (value: string) => void;
  onSelectRoom: (roomId: string) => void;
  onOpenCreate: () => void;
  onRoomAction: (room: WatchRoom) => void;
  onLeaveRoom: () => void;
  onEndRoom: () => void;
  onSend: () => void;
  onSubmitUrl: () => void;
}) {
  const canSend = !!props.selectedRoom && !!props.draft.trim();
  const isHost = props.activation?.is_host ?? false;
  const messagesRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const element = messagesRef.current;
    if (!element) return;
    element.scrollTop = element.scrollHeight;
  }, [props.selectedRoom?.room_id, props.messages.length]);

  return (
    <section className="watch-layout">
      <section className="panel stack watch-sidebar">
        <div className="panel-title">
          <h2>观影房间</h2>
          <button className="secondary compact" onClick={props.onOpenCreate}>
            <Plus size={15} /> 创建
          </button>
        </div>
        <div className="watch-room-list">
          {props.rooms.length === 0 ? (
            <Empty icon={<Library size={28} />} text="还没有可加入的观影房间" />
          ) : (
            props.rooms.map((room) => {
              const selected = props.selectedRoom?.room_id === room.room_id;
              const isHostRoom = !!props.localDeviceId && room.host_device_id === props.localDeviceId;
              const isMember = !!props.localDeviceId && room.member_ids.includes(props.localDeviceId);
              const actionLabel = isHostRoom ? "房主" : isMember ? "离开" : "进入";
              return (
                <article
                  className={`watch-room-card ${selected ? "active" : ""}`}
                  key={room.room_id}
                  onClick={() => props.onSelectRoom(room.room_id)}
                >
                  <div className="watch-room-card-head">
                    <strong>{room.title}</strong>
                    <span className="pill">{room.is_private ? "密码" : "公开"}</span>
                  </div>
                  <p className="muted">房主：{room.host_name} · {room.member_ids.length} 人</p>
                  <p className="muted">视频：{room.current_url ? simplifyVideoUrl(room.current_url) : "等待房主提交链接"}</p>
                  <button
                    className="primary compact"
                    disabled={props.busy || isHostRoom}
                    onClick={(event) => {
                      event.stopPropagation();
                      props.onRoomAction(room);
                    }}
                  >
                    {actionLabel}
                  </button>
                </article>
              );
            })
          )}
        </div>
      </section>

      <section className="panel stack watch-main">
        {props.selectedRoom ? (
          <>
            <div className="panel-title">
              <div>
                <h2>{props.selectedRoom.title}</h2>
                <p className="muted">
                  房主：{props.selectedRoom.host_name} · {props.selectedRoom.member_ids.length} 人 ·{" "}
                  {props.selectedRoom.is_private ? "密码房" : "公开房"}
                </p>
              </div>
              <div className="row-actions">
                {isHost && (
                  <button className="icon-button danger" title="结束房间" onClick={props.onEndRoom}>
                    <Trash2 size={15} />
                  </button>
                )}
              </div>
            </div>
            {isHost && (
              <div className="watch-url-bar">
                <input
                  value={props.urlDraft}
                  onChange={(event) => props.onUrlDraft(event.target.value)}
                  placeholder="提交或替换视频网页链接（http/https）"
                />
                <button className="primary" disabled={props.busy || !props.urlDraft.trim()} onClick={props.onSubmitUrl}>
                  提交链接
                </button>
              </div>
            )}
            <div className="watch-player-frame" ref={props.viewportRef}>
              {!props.selectedRoom.current_url ? (
                <div className="watch-player-placeholder">
                  <strong>等待房主提交视频链接</strong>
                  <p className="muted">房间和右侧聊天已经可用，房主提交后会直接加载到这里。</p>
                </div>
              ) : (
                <div className="watch-player-placeholder ready">
                  <strong>视频正在内嵌窗口中播放</strong>
                  <p className="muted">切换其它页面时会隐藏播放器，但后台不会暂停。</p>
                </div>
              )}
            </div>
          </>
        ) : (
          <Empty icon={<Library size={30} />} text="选择一个观影房间开始观看" />
        )}
      </section>

      <section className="panel stack watch-chat-panel">
        <div className="panel-title">
          <div>
            <h2>房间聊天</h2>
            <p className="muted">这是观影室独立聊天，不会进入普通聊天室。</p>
          </div>
        </div>
        <div className="chat-messages" ref={messagesRef}>
          {props.messages.length === 0 ? (
            <Empty icon={<MessageCircle size={30} />} text="暂时还没有观影聊天消息" />
          ) : (
            props.messages.map((message) => (
              <article className="chat-message" key={message.message_id}>
                <img className="avatar" src={messageAvatarSrc(message, props.devices)} alt="" onError={useDefaultAvatar} />
                <div>
                  <div className="chat-message-head">
                    <strong>{messageSenderName(message, props.devices)}</strong>
                    <span>{formatDateTime(message.created_at)}</span>
                  </div>
                  <p>{message.body}</p>
                </div>
              </article>
            ))
          )}
        </div>
        <div className="chat-input">
          <textarea
            value={props.draft}
            onChange={(event) => props.onDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && !event.shiftKey) {
                event.preventDefault();
                props.onSend();
              }
            }}
            placeholder="发送观影室聊天消息"
            rows={2}
          />
          <button className="primary" disabled={!canSend || props.busy} onClick={props.onSend}>
            <Send size={16} /> 发送
          </button>
        </div>
      </section>
    </section>
  );
}

function GameTab(props: {
  localDeviceId: string | null;
  rooms: GameRoomSummary[];
  selectedRoom: GameRoomSummary | null;
  activation: GameActivation | null;
  snapshot: GameRoomSnapshot | null;
  busy: boolean;
  onSelectRoom: (roomId: string) => void;
  onOpenCreate: () => void;
  onRoomAction: (room: GameRoomSummary) => void;
  onLeaveRoom: () => void;
  onCloseRoom: () => void;
  onMove: (x: number, y: number) => void;
  onRestart: () => void;
  onAcceptRestart: () => void;
  onSurrender: () => void;
}) {
  const isHost = props.activation?.is_host ?? false;
  const isMember = props.activation?.is_member ?? false;
  const winner = props.snapshot?.gomoku_state.winner ?? null;
  const myColor = props.localDeviceId
    ? props.snapshot?.gomoku_state.black_peer_id === props.localDeviceId
      ? 1
      : props.snapshot?.gomoku_state.white_peer_id === props.localDeviceId
        ? 2
        : null
    : null;
  const isMyTurn = !!myColor && props.snapshot?.gomoku_state.current_turn === myColor && !props.snapshot.gomoku_state.winner;
  const restartRequestedByOpponent =
    !!props.snapshot?.gomoku_state.restart_requested_by &&
    props.snapshot.gomoku_state.restart_requested_by !== props.localDeviceId;

  return (
    <section className="game-layout">
      <section className="panel stack game-sidebar">
        <div className="panel-title">
          <h2>五子棋房间</h2>
          <button className="secondary compact" onClick={props.onOpenCreate}>
            <Plus size={15} /> 创建
          </button>
        </div>
        <div className="watch-room-list">
          {props.rooms.length === 0 ? (
            <Empty icon={<Library size={28} />} text="还没有可加入的五子棋房间" />
          ) : (
            props.rooms.map((room) => {
              const selected = props.selectedRoom?.room_id === room.room_id;
              const isHostRoom = !!props.localDeviceId && room.host_peer_id === props.localDeviceId;
              const isJoined =
                !!props.localDeviceId &&
                (room.host_peer_id === props.localDeviceId || room.guest_peer_id === props.localDeviceId);
              const actionLabel = isHostRoom ? "房主" : isJoined ? "离开" : "进入";
              return (
                <article
                  className={`watch-room-card ${selected ? "active" : ""}`}
                  key={room.room_id}
                  onClick={() => props.onSelectRoom(room.room_id)}
                >
                  <div className="watch-room-card-head">
                    <strong>{room.room_name}</strong>
                    <span className="pill">{room.visibility === "password" ? "密码" : "公开"}</span>
                  </div>
                  <p className="muted">房主：{room.host_name} · {room.guest_peer_id ? "2/2" : "1/2"}</p>
                  <p className="muted">状态：{gameRoomStatusLabel(room.status)}</p>
                  <button
                    className="primary compact"
                    disabled={props.busy || isHostRoom}
                    onClick={(event) => {
                      event.stopPropagation();
                      props.onRoomAction(room);
                    }}
                  >
                    {actionLabel}
                  </button>
                </article>
              );
            })
          )}
        </div>
      </section>

      <section className="panel stack game-main">
        {props.selectedRoom ? (
          <>
            <div className="panel-title">
              <div>
                <h2>{props.selectedRoom.room_name}</h2>
                <p className="muted">
                  房主：{props.selectedRoom.host_name} · {props.selectedRoom.visibility === "password" ? "密码房" : "公开房"}
                </p>
              </div>
              <div className="row-actions">
                {isMember && !isHost && (
                  <button className="secondary compact" disabled={props.busy} onClick={props.onLeaveRoom}>
                    离开房间
                  </button>
                )}
                {isHost && (
                  <button className="icon-button danger" title="解散房间" onClick={props.onCloseRoom}>
                    <Trash2 size={15} />
                  </button>
                )}
              </div>
            </div>
            {!isMember || !props.snapshot ? (
              <div className="game-placeholder">
                <strong>进入房间后开始对局</strong>
                <p className="muted">未加入房间前不会加载棋盘。</p>
              </div>
            ) : (
              <>
                <div className="game-meta">
                  <span className="pill">你执{myColor === 1 ? "黑棋" : myColor === 2 ? "白棋" : "未加入"}</span>
                  <span className={`pill turn-pill ${isMyTurn ? "active" : ""}`}>
                    {isMyTurn ? "轮到你了" : props.snapshot.gomoku_state.status_text}
                  </span>
                  {props.snapshot.gomoku_state.last_move && (
                    <span className="pill">
                      最后一步：{props.snapshot.gomoku_state.last_move.x + 1},{props.snapshot.gomoku_state.last_move.y + 1}
                    </span>
                  )}
                </div>
                {winner && (
                  <div className="game-result-banner">
                    {winner === 1 ? "黑棋获胜" : "白棋获胜"}
                    {myColor && winner === myColor ? "，你赢了" : myColor ? "，你输了" : ""}
                  </div>
                )}
                <div className="game-topbar">
                  <div className="row-actions game-topbar-left">
                    {restartRequestedByOpponent ? (
                      <button className="primary compact" disabled={props.busy} onClick={props.onAcceptRestart}>
                        <Check size={15} /> 鍚屾剰閲嶅紑
                      </button>
                    ) : (
                      <button className="secondary compact" disabled={props.busy || !isMember} onClick={props.onRestart}>
                        <RefreshCw size={15} /> 閲嶆柊寮€濮?
                      </button>
                    )}
                    <button
                      className="secondary compact"
                      disabled={props.busy || !isMember || !!winner}
                      onClick={props.onSurrender}
                    >
                      璁よ緭
                    </button>
                  </div>
                  <div className="row-actions game-topbar-right">
                    {isMember && !isHost && (
                      <button className="secondary compact" disabled={props.busy} onClick={props.onLeaveRoom}>
                        退出
                      </button>
                    )}
                    {isHost && (
                      <button className="icon-button danger" title="瑙ｆ暎鎴块棿" onClick={props.onCloseRoom}>
                        <Trash2 size={15} />
                      </button>
                    )}
                  </div>
                </div>
                <div className="gomoku-board" role="grid" aria-label="Gomoku Board">
                  {props.snapshot.gomoku_state.board.map((row, y) =>
                    row.map((cell, x) => {
                      const isLastMove =
                        props.snapshot?.gomoku_state.last_move?.x === x &&
                        props.snapshot?.gomoku_state.last_move?.y === y;
                      return (
                        <button
                          key={`${x}-${y}`}
                          className={`gomoku-cell ${isLastMove ? "last" : ""} ${cell !== 0 ? "occupied" : ""}`}
                          disabled={props.busy || !isMyTurn || cell !== 0}
                          onClick={() => props.onMove(x, y)}
                        >
                          {cell === 1 ? <span className="gomoku-stone black" /> : null}
                          {cell === 2 ? <span className="gomoku-stone white" /> : null}
                        </button>
                      );
                    }),
                  )}
                </div>
                <div className="row-actions game-actions">
                  <button
                    className="secondary compact"
                    disabled={props.busy || !isMember || !!props.snapshot.gomoku_state.winner}
                    onClick={props.onSurrender}
                  >
                    认输
                  </button>
                  {restartRequestedByOpponent ? (
                    <button className="primary compact" disabled={props.busy} onClick={props.onAcceptRestart}>
                      <Check size={15} /> 同意重开
                    </button>
                  ) : (
                    <button className="secondary compact" disabled={props.busy || !isMember} onClick={props.onRestart}>
                      <RefreshCw size={15} /> 重新开始
                    </button>
                  )}
                </div>
              </>
            )}
          </>
        ) : (
          <div className="game-placeholder">
            <strong>选择一个五子棋房间开始</strong>
            <p className="muted">小游戏页面会在后台保持状态，切换页面不会重置棋盘。</p>
          </div>
        )}
      </section>
    </section>
  );
}

function GameTabFixed(props: {
  localDeviceId: string | null;
  rooms: GameRoomSummary[];
  selectedRoom: GameRoomSummary | null;
  activation: GameActivation | null;
  snapshot: GameRoomSnapshot | null;
  exitedRoomIds: string[];
  busy: boolean;
  onSelectRoom: (roomId: string) => void;
  onOpenCreate: () => void;
  onRoomAction: (room: GameRoomSummary) => void;
  onLeaveRoom: () => void;
  onCloseRoom: () => void;
  onMove: (x: number, y: number) => void;
  onRestart: () => void;
  onAcceptRestart: () => void;
  onSurrender: () => void;
}) {
  const isHost = props.activation?.is_host ?? false;
  const isMember = props.activation?.is_member ?? false;
  const winner = props.snapshot?.gomoku_state.winner ?? null;
  const myColor = props.localDeviceId
    ? props.snapshot?.gomoku_state.black_peer_id === props.localDeviceId
      ? 1
      : props.snapshot?.gomoku_state.white_peer_id === props.localDeviceId
        ? 2
        : null
    : null;
  const isMyTurn =
    !!myColor && props.snapshot?.gomoku_state.current_turn === myColor && !props.snapshot.gomoku_state.winner;
  const restartRequestedByOpponent =
    !!props.snapshot?.gomoku_state.restart_requested_by &&
    props.snapshot.gomoku_state.restart_requested_by !== props.localDeviceId;

  return (
    <section className="game-layout">
      <section className="panel stack game-sidebar">
        <div className="panel-title">
          <h2>五子棋房间</h2>
          <button className="secondary compact" onClick={props.onOpenCreate}>
            <Plus size={15} /> 创建
          </button>
        </div>
        <div className="watch-room-list">
          {props.rooms.length === 0 ? (
            <Empty icon={<Library size={28} />} text="还没有可加入的五子棋房间" />
          ) : (
            props.rooms.map((room) => {
              const selected = props.selectedRoom?.room_id === room.room_id;
              const isHostRoom = !!props.localDeviceId && room.host_peer_id === props.localDeviceId;
              const exitedLocally = props.exitedRoomIds.includes(room.room_id);
              const isJoined =
                !!props.localDeviceId &&
                (room.host_peer_id === props.localDeviceId || room.guest_peer_id === props.localDeviceId) &&
                !exitedLocally;
              const actionLabel = isHostRoom ? "房主" : isJoined ? "退出" : "进入";
              return (
                <article
                  className={`watch-room-card ${selected ? "active" : ""}`}
                  key={room.room_id}
                  onClick={() => props.onSelectRoom(room.room_id)}
                >
                  <div className="watch-room-card-head">
                    <strong>{room.room_name}</strong>
                    <span className="pill">{room.visibility === "password" ? "密码" : "公开"}</span>
                  </div>
                  <p className="muted">房主：{room.host_name} · {room.guest_peer_id ? "2/2" : "1/2"}</p>
                  <p className="muted">状态：{gameRoomStatusLabel(room.status)}</p>
                  <button
                    className="primary compact"
                    disabled={props.busy || isHostRoom}
                    onClick={(event) => {
                      event.stopPropagation();
                      props.onRoomAction(room);
                    }}
                  >
                    {actionLabel}
                  </button>
                </article>
              );
            })
          )}
        </div>
      </section>

      <section className="panel stack game-main">
        {props.selectedRoom ? (
          <>
            <div className="panel-title">
              <div>
                <h2>{props.selectedRoom.room_name}</h2>
                <p className="muted">
                  房主：{props.selectedRoom.host_name} · {props.selectedRoom.visibility === "password" ? "密码房" : "公开房"}
                </p>
              </div>
            </div>
            {!isMember || !props.snapshot ? (
              <div className="game-placeholder">
                <strong>进入房间后开始对局</strong>
                <p className="muted">未加入房间前不会加载棋盘。</p>
              </div>
            ) : (
              <>
                <div className="game-meta">
                  <span className="pill">你执{myColor === 1 ? "黑棋" : myColor === 2 ? "白棋" : "未加入"}</span>
                  <span className={`pill turn-pill ${isMyTurn ? "active" : ""}`}>
                    {isMyTurn ? "轮到你了" : props.snapshot.gomoku_state.status_text}
                  </span>
                  {props.snapshot.gomoku_state.last_move ? (
                    <span className="pill">
                      最后一步：{props.snapshot.gomoku_state.last_move.x + 1},{props.snapshot.gomoku_state.last_move.y + 1}
                    </span>
                  ) : null}
                </div>
                {winner ? (
                  <div className="game-result-banner">
                    {winner === 1 ? "黑棋获胜" : "白棋获胜"}
                    {myColor && winner === myColor ? "，你赢了" : myColor ? "，你输了" : ""}
                  </div>
                ) : null}
                <div className="game-topbar">
                  <div className="row-actions game-topbar-left">
                    {restartRequestedByOpponent ? (
                      <button className="primary compact" disabled={props.busy} onClick={props.onAcceptRestart}>
                        <Check size={15} /> 同意重开
                      </button>
                    ) : (
                      <button className="secondary compact" disabled={props.busy || !isMember} onClick={props.onRestart}>
                        <RefreshCw size={15} /> 重新开始
                      </button>
                    )}
                    <button
                      className="secondary compact"
                      disabled={props.busy || !isMember || !!winner}
                      onClick={props.onSurrender}
                    >
                      认输
                    </button>
                  </div>
                  <div className="row-actions game-topbar-right">
                    {isMember && !isHost ? (
                      <button className="secondary compact" disabled={props.busy} onClick={props.onLeaveRoom}>
                        退出
                      </button>
                    ) : null}
                    {isHost ? (
                      <button className="icon-button danger" title="解散房间" onClick={props.onCloseRoom}>
                        <Trash2 size={15} />
                      </button>
                    ) : null}
                  </div>
                </div>
                <div className="gomoku-board" role="grid" aria-label="Gomoku Board">
                  {props.snapshot.gomoku_state.board.map((row, y) =>
                    row.map((cell, x) => {
                      const isLastMove =
                        props.snapshot?.gomoku_state.last_move?.x === x &&
                        props.snapshot?.gomoku_state.last_move?.y === y;
                      return (
                        <button
                          key={`${x}-${y}`}
                          className={`gomoku-cell ${isLastMove ? "last" : ""}`}
                          disabled={props.busy || !isMyTurn || cell !== 0}
                          onClick={() => props.onMove(x, y)}
                        >
                          {cell === 1 ? <span className="gomoku-stone black" /> : null}
                          {cell === 2 ? <span className="gomoku-stone white" /> : null}
                        </button>
                      );
                    }),
                  )}
                </div>
              </>
            )}
          </>
        ) : (
          <div className="game-placeholder">
            <strong>选择一个五子棋房间开始</strong>
            <p className="muted">小游戏页面会在后台保持状态，切换页面不会重置棋盘。</p>
          </div>
        )}
      </section>
    </section>
  );
}

function SettingsTab(props: {
  settings: AppSettings;
  appInfo: AppInfo | null;
  updateInfo: UpdateInfo | null;
  librarySettings: LibrarySettings;
  nicknameDraft: string;
  networkStatus: NetworkStatus | null;
  controlApi: ControlApiInfo | null;
  busy: boolean;
  onNicknameDraft: (value: string) => void;
  onSaveNickname: () => void;
  onChooseDownloadDir: () => void;
  onChooseAvatar: () => void;
  onOpenDownloadDir: () => void;
  onUpdateApp: () => void;
  onLibrarySettings: (settings: LibrarySettings) => void;
}) {
  const update = (patch: Partial<LibrarySettings>) =>
    props.onLibrarySettings({ ...props.librarySettings, ...patch });

  return (
    <section className="grid two">
      <section className="panel stack">
        <h2>本机设置</h2>
        <div className="settings-row">
          <Info size={17} />
          <strong>当前版本</strong>
          <code title={props.updateInfo?.release_url ?? undefined}>
            {props.appInfo?.version ?? "0.1.0"}
            {props.updateInfo?.update_available ? ` → ${props.updateInfo.latest_version}` : ""}
          </code>
          <button className="secondary" disabled={props.busy} onClick={props.onUpdateApp}>
            <Download size={16} /> 更新
          </button>
        </div>
        <div className="settings-row">
          <Settings size={17} />
          <strong>昵称</strong>
          <input value={props.nicknameDraft} onChange={(event) => props.onNicknameDraft(event.target.value)} />
          <button className="secondary" disabled={props.busy} onClick={props.onSaveNickname}>
            <Save size={16} /> 保存
          </button>
        </div>
        <div className="settings-row">
          <UploadCloud size={17} />
          <strong>头像</strong>
          <div className="avatar-setting">
            <img src={localAvatarSrc(props.settings, props.networkStatus)} alt="" onError={useDefaultAvatar} />
            <span>{props.settings.avatar_hash ? "已设置头像" : "使用默认头像"}</span>
          </div>
          <button className="secondary" disabled={props.busy} onClick={props.onChooseAvatar}>
            <UploadCloud size={16} /> 上传
          </button>
        </div>
        <div className="settings-row">
          <FolderOpen size={17} />
          <strong>下载目录</strong>
          <button className="path-link" onClick={props.onOpenDownloadDir} title="打开下载目录">
            {props.settings.download_dir}
          </button>
          <button className="secondary" onClick={props.onChooseDownloadDir}>
            浏览
          </button>
        </div>
        <div className="diagnostics">
          <strong>网络</strong>
          <span>UDP {props.networkStatus?.udp_port ?? "--"}</span>
          <span>TCP {props.networkStatus?.tcp_port ?? "--"}</span>
          <span>HTTP {props.networkStatus?.api_port ?? "--"}</span>
          <span>{props.networkStatus?.local_ips.join(", ") || "无本机局域网 IP"}</span>
        </div>
        <footer className="control-api">
          Codex 本地控制 API <code>{props.controlApi?.enabled ? `http://${props.controlApi.bind}` : "未启用"}</code>
        </footer>
      </section>

      <section className="panel stack">
        <h2>资源加速与缓存</h2>
        <label className="toggle-row">
          <input
            type="checkbox"
            checked={props.librarySettings.acceleration_enabled}
            onChange={(event) => update({ acceleration_enabled: event.target.checked })}
          />
          参与资源加速
        </label>
        <div className="form-grid">
          <label>
            最大上传速度
            <select value={props.librarySettings.max_upload_speed} onChange={(event) => update({ max_upload_speed: event.target.value })}>
              <option value="unlimited">不限</option>
              <option value="10MB/s">10MB/s</option>
              <option value="50MB/s">50MB/s</option>
              <option value="100MB/s">100MB/s</option>
            </select>
          </label>
          <label>
            最大上传任务
            <select value={props.librarySettings.max_upload_tasks} onChange={(event) => update({ max_upload_tasks: Number(event.target.value) })}>
              {[1, 2, 3, 5].map((value) => (
                <option key={value} value={value}>
                  {value}
                </option>
              ))}
            </select>
          </label>
          <label>
            缓存上限
            <select value={props.librarySettings.cache_limit_gb} onChange={(event) => update({ cache_limit_gb: Number(event.target.value) })}>
              {[10, 50, 100, 500].map((value) => (
                <option key={value} value={value}>
                  {value}GB
                </option>
              ))}
            </select>
          </label>
        </div>
      </section>
    </section>
  );
}

function Dropzone(props: {
  icon: React.ReactNode;
  title: string;
  text: string;
  onDrop: (paths: string[]) => void;
}) {
  return (
    <div
      className="dropzone"
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => {
        event.preventDefault();
      }}
    >
      {props.icon}
      <h2>{props.title}</h2>
      <p>{props.text}</p>
    </div>
  );
}

function PathList({ paths, onRemove }: { paths: string[]; onRemove: (path: string) => void }) {
  if (paths.length === 0) return null;
  return (
    <div className="file-list">
      {paths.map((path) => (
        <div className="file-row" key={path}>
          <span title={path}>{basename(path)}</span>
          <button className="icon-button" title="移除" onClick={() => onRemove(path)}>
            <X size={15} />
          </button>
        </div>
      ))}
    </div>
  );
}

function ShareRow({ share, action }: { share: ShareItem; action: React.ReactNode }) {
  return (
    <article className="resource">
      <div className="resource-main">
        <strong>{share.name}</strong>
        <span>
          {share.owner_name} · v{share.latest_version} · {formatBytes(share.size)}
        </span>
        <small title={share.file_hash}>
          上传/更新 {formatDateTime(share.updated_at)} · {share.file_hash.slice(0, 16)} · {share.replica_count} 个副本节点
        </small>
      </div>
      <div className="resource-meta">
        <span className="pill">{share.category}</span>
        <span className="pill">{share.permission === "password" ? "密码" : "公开"}</span>
        <span>{share.download_count} 次下载</span>
      </div>
      {action}
    </article>
  );
}

function TransfersPanel({
  transfers,
  open,
  onToggle,
  onRemove,
  onClearFinished,
}: {
  transfers: TransferInfo[];
  open: boolean;
  onToggle: () => void;
  onRemove: (transferId: string) => void;
  onClearFinished: () => void;
}) {
  const activeCount = transfers.filter((transfer) =>
    ["pending", "waiting_for_receiver", "transferring"].includes(transfer.status),
  ).length;
  return (
    <section className="panel stack">
      <div className="panel-title">
        <button className="section-toggle" onClick={onToggle}>
          <Clock size={17} />
          <h2>传输任务</h2>
          <span className="pill">{activeCount} 进行中 / {transfers.length} 总计</span>
        </button>
        <button className="secondary compact" onClick={onClearFinished}>
          <Trash2 size={15} /> 清理已结束
        </button>
      </div>
      {!open ? null : transfers.length === 0 ? (
        <Empty icon={<Clock size={30} />} text="暂无传输任务" />
      ) : (
        <div className="transfer-grid">
          {transfers.slice(0, 8).map((transfer) => (
            <TransferRow key={transfer.id} transfer={transfer} onRemove={onRemove} />
          ))}
        </div>
      )}
    </section>
  );
}

function TransferRow({
  transfer,
  onRemove,
}: {
  transfer: TransferInfo;
  onRemove: (transferId: string) => void;
}) {
  const percent =
    transfer.file_size > 0 ? Math.min(100, (transfer.bytes_done / transfer.file_size) * 100) : 0;
  const canOpen = transfer.direction === "receiving" && transfer.save_path;
  return (
    <article className={`transfer ${transfer.status}`}>
      <div className="transfer-head">
        <div>
          <strong>{transfer.file_name}</strong>
          <span>
            {transfer.direction === "sending" ? "发送到" : "来自"} {transfer.peer_name}
          </span>
        </div>
        <div className="row-actions">
          <StatusBadge status={transfer.status} />
          <button className="icon-button" title="删除记录" onClick={() => onRemove(transfer.id)}>
            <Trash2 size={15} />
          </button>
        </div>
      </div>
      <div className="progress">
        <span style={{ width: `${percent}%` }} />
      </div>
      <div className="transfer-meta">
        <span>{formatBytes(transfer.bytes_done)} / {formatBytes(transfer.file_size)}</span>
        <span>{formatSpeed(transfer.speed_bps)}</span>
        <span>{transfer.eta_secs == null ? "ETA --" : `剩余 ${formatDuration(transfer.eta_secs)}`}</span>
      </div>
      {transfer.message && <p className="message">{transfer.message}</p>}
      {canOpen && (
        <button className="secondary fit" onClick={() => void openPathLocation(transfer.save_path!)}>
          <FolderOpen size={16} /> 打开位置
        </button>
      )}
    </article>
  );
}

function StatusBadge({ status }: { status: TransferInfo["status"] }) {
  const labels: Record<TransferInfo["status"], string> = {
    pending: "待处理",
    waiting_for_receiver: "等待接收",
    transferring: "传输中",
    completed: "完成",
    rejected: "已拒绝",
    failed: "失败",
  };
  return <span className={`badge ${status}`}>{labels[status]}</span>;
}

function IncomingWindow({ transferId }: { transferId: string }) {
  const [transfer, setTransfer] = useState<TransferInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void getTransfer(transferId).then(setTransfer);
  }, [transferId]);

  async function answer(accepted: boolean) {
    try {
      if (accepted) {
        await acceptTransfer(transferId);
      } else {
        await rejectTransfer(transferId);
      }
      await getCurrentWindow().close();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <main className="incoming-window">
      <h1>接收文件？</h1>
      {transfer ? (
        <>
          <p>
            <strong>{transfer.peer_name}</strong> 想发送文件给你
          </p>
          <div className="incoming-file">
            <strong>{transfer.file_name}</strong>
            <span>{formatBytes(transfer.file_size)}</span>
          </div>
          {error && <div className="error">{error}</div>}
          <div className="modal-actions">
            <button onClick={() => void answer(false)}>拒绝</button>
            <button className="primary" onClick={() => void answer(true)}>
              <Check size={17} /> 接收
            </button>
          </div>
        </>
      ) : (
        <p>正在读取传输请求...</p>
      )}
    </main>
  );
}

function Empty({ icon, text }: { icon: React.ReactNode; text: string }) {
  return (
    <div className="empty">
      {icon}
      <p>{text}</p>
    </div>
  );
}

function localAvatarSrc(settings: AppSettings, networkStatus: NetworkStatus | null) {
  if (!settings.avatar_hash || !networkStatus?.api_port) return defaultAvatarUrl;
  return `http://127.0.0.1:${networkStatus.api_port}/avatars/${encodeURIComponent(settings.avatar_hash)}?v=${encodeURIComponent(settings.avatar_hash)}`;
}

function deviceAvatarSrc(device: DeviceInfo) {
  if (!device.avatar_hash) return defaultAvatarUrl;
  const host = device.is_local ? "127.0.0.1" : device.ip;
  return `http://${host}:${device.api_port}/avatars/${encodeURIComponent(device.avatar_hash)}?v=${encodeURIComponent(device.avatar_hash)}`;
}

function messageAvatarSrc(message: ChatMessage, devices: DeviceInfo[]) {
  const device = devices.find((item) => item.id === message.sender_device_id);
  if (device) return deviceAvatarSrc(device);
  return defaultAvatarUrl;
}

function messageSenderName(message: ChatMessage, devices: DeviceInfo[]) {
  const device = devices.find((item) => item.id === message.sender_device_id);
  if (!device?.note) return message.sender_name;
  return `${device.note}（${message.sender_name}）`;
}

function useDefaultAvatar(event: React.SyntheticEvent<HTMLImageElement>) {
  if (event.currentTarget.src !== defaultAvatarUrl) {
    event.currentTarget.src = defaultAvatarUrl;
  }
}

function upsertRoom(rooms: ChatRoom[], room: ChatRoom) {
  const rest = rooms.filter((item) => item.room_id !== room.room_id);
  return [room, ...rest].sort((a, b) =>
    Number(b.is_main) - Number(a.is_main) || a.created_at - b.created_at || a.name.localeCompare(b.name),
  );
}

function upsertWatchRoom(rooms: WatchRoom[], room: WatchRoom) {
  const rest = rooms.filter((item) => item.room_id !== room.room_id);
  return [room, ...rest].sort((a, b) => b.created_at - a.created_at || a.title.localeCompare(b.title));
}

function upsertGameRoom(rooms: GameRoomSummary[], room: GameRoomSummary) {
  const rest = rooms.filter((item) => item.room_id !== room.room_id);
  return [room, ...rest].sort(
    (a, b) => b.created_at - a.created_at || a.room_name.localeCompare(b.room_name),
  );
}

function basename(path: string) {
  return path.split(/[\\/]/).pop() ?? path;
}

function gameRoomStatusLabel(status: GameRoomSummary["status"]) {
  const labels: Record<GameRoomSummary["status"], string> = {
    waiting: "等待中",
    playing: "对局中",
    finished: "已结束",
  };
  return labels[status];
}

function simplifyVideoUrl(url: string) {
  try {
    const parsed = new URL(url);
    return parsed.hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

async function sha256Text(value: string) {
  const data = new TextEncoder().encode(value);
  const digest = await crypto.subtle.digest("SHA-256", data);
  return Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

function formatDateTime(seconds: number) {
  if (!seconds) return "--";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(seconds * 1000));
}

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let size = value / 1024;
  let unit = units[0];
  for (let i = 1; i < units.length && size >= 1024; i += 1) {
    size /= 1024;
    unit = units[i];
  }
  return `${size.toFixed(size >= 10 ? 1 : 2)} ${unit}`;
}

function formatSpeed(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "--/s";
  return `${formatBytes(value)}/s`;
}

function formatDuration(seconds: number) {
  if (seconds < 60) return `${seconds}s`;
  const mins = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return `${mins}m ${rest}s`;
}
