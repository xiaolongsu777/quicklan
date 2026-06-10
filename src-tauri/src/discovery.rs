use crate::{
    lan_api,
    library::LibraryService,
    protocol::{
        DeviceInfo, DiscoveryPacket, KnownPeerHint, NetworkStatus, DEVICE_TTL_SECS, DISCOVERY_PORT,
        LIBRARY_ANNOUNCE_INTERVAL_SECS, PRESENCE_INTERVAL_SECS,
    },
    settings::SettingsService,
    storage,
};
use get_if_addrs::{get_if_addrs, IfAddr};
use std::{
    collections::HashMap,
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

const KNOWN_PROBE_INTERVAL_SECS: u64 = 3;
const INTRO_HINT_LIMIT: usize = 6;

#[derive(Clone)]
pub struct DiscoveryService {
    app: AppHandle,
    device_id: String,
    tcp_port: u16,
    api_port: u16,
    settings: SettingsService,
    library: LibraryService,
    devices: Arc<Mutex<HashMap<String, DeviceEntry>>>,
    candidate_ips: Arc<Mutex<HashMap<String, CandidatePeer>>>,
}

#[derive(Clone)]
struct DeviceEntry {
    info: DeviceInfo,
    last_seen: Instant,
}

#[derive(Clone)]
struct CandidatePeer {
    source: CandidateSource,
    last_seen: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CandidateSource {
    Manual,
    Introduced,
}

impl CandidateSource {
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Manual, _) | (_, Self::Manual) => Self::Manual,
            _ => Self::Introduced,
        }
    }
}

impl DiscoveryService {
    pub fn new(
        app: AppHandle,
        tcp_port: u16,
        api_port: u16,
        settings: SettingsService,
        library: LibraryService,
    ) -> Self {
        Self {
            app,
            device_id: library.device_id(),
            tcp_port,
            api_port,
            settings,
            library,
            devices: Arc::new(Mutex::new(HashMap::new())),
            candidate_ips: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn start(&self) {
        self.preload_known_candidates();
        self.start_presence_loop();
        self.start_library_announce_loop();
        self.start_known_peer_probe_loop();
        self.start_listen_loop();
        self.start_prune_loop();
    }

    pub fn broadcast_now(&self) {
        let packet = self.packet("library", None, false);
        let _ = broadcast_packet(&packet);
    }

    pub fn list_devices(&self) -> Vec<DeviceInfo> {
        self.devices
            .lock()
            .map(|map| self.snapshot_devices(&map))
            .unwrap_or_default()
    }

    pub fn find_device(&self, id: &str) -> Option<DeviceInfo> {
        if id == self.device_id {
            return Some(self.local_device_info());
        }
        self.devices
            .lock()
            .ok()
            .and_then(|map| map.get(id).map(|entry| entry.info.clone()))
    }

    pub fn update_device_note(
        &self,
        device_id: String,
        note: String,
    ) -> Result<Vec<DeviceInfo>, String> {
        let note = self.library.update_device_note(&device_id, note)?;
        let snapshot = if let Ok(mut map) = self.devices.lock() {
            if let Some(entry) = map.get_mut(&device_id) {
                entry.info.note = note;
            }
            self.snapshot_devices(&map)
        } else {
            Vec::new()
        };
        let _ = self.app.emit("devices-updated", snapshot.clone());
        Ok(snapshot)
    }

    pub fn emit_devices(&self) {
        let snapshot = self.list_devices();
        let _ = self.app.emit("devices-updated", snapshot);
    }

    pub fn probe_ip(&self, ip: String) -> Result<(), String> {
        let target_ip: IpAddr = ip.parse().map_err(|_| "IP 鍦板潃鏃犳晥".to_string())?;
        self.remember_candidate_ip(target_ip, CandidateSource::Manual);
        let packet = self.packet("library", None, true);
        send_discovery_packet(&packet, SocketAddr::new(target_ip, DISCOVERY_PORT))
    }

    pub fn network_status(&self) -> NetworkStatus {
        NetworkStatus {
            udp_port: DISCOVERY_PORT,
            tcp_port: self.tcp_port,
            api_port: self.api_port,
            local_ips: local_ipv4_addresses()
                .into_iter()
                .map(|ip| ip.to_string())
                .collect(),
            broadcast_targets: broadcast_targets()
                .into_iter()
                .map(|addr| addr.to_string())
                .collect(),
        }
    }

    pub fn local_sender(&self) -> crate::protocol::SenderInfo {
        crate::protocol::SenderInfo {
            device_id: self.device_id.clone(),
            device_name: self.settings.nickname(),
        }
    }

    fn packet(
        &self,
        packet_type: &str,
        target_device_id: Option<&str>,
        include_known_peers: bool,
    ) -> DiscoveryPacket {
        let summary = self.library.summary();
        let known_peers = if include_known_peers {
            let mut exclude = vec![self.device_id.as_str()];
            if let Some(target_id) = target_device_id {
                exclude.push(target_id);
            }
            self.library
                .known_peer_hints(INTRO_HINT_LIMIT, &exclude)
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        DiscoveryPacket {
            app: "quicklan".to_string(),
            version: 1,
            packet_type: packet_type.to_string(),
            device_id: self.device_id.clone(),
            device_name: self.settings.nickname(),
            tcp_port: self.tcp_port,
            api_port: self.api_port,
            library_version: summary.library_version,
            share_count: summary.share_count,
            manifest_hash: summary.manifest_hash,
            upload_tasks: 0,
            avatar_hash: self.settings.avatar_hash(),
            known_peers,
        }
    }

    fn preload_known_candidates(&self) {
        if let Ok(known_devices) = self.library.list_known_devices() {
            for known in known_devices {
                let source = if known.pinned {
                    CandidateSource::Manual
                } else {
                    CandidateSource::Introduced
                };
                if let Ok(ip) = known.ip.parse::<IpAddr>() {
                    self.remember_candidate_ip(ip, source);
                }
            }
        }
    }

    fn start_presence_loop(&self) {
        let service = self.clone();
        thread::spawn(move || loop {
            let packet = service.packet("presence", None, false);
            let _ = broadcast_packet(&packet);
            thread::sleep(Duration::from_secs(PRESENCE_INTERVAL_SECS));
        });
    }

    fn start_library_announce_loop(&self) {
        let service = self.clone();
        thread::spawn(move || loop {
            service.broadcast_now();
            thread::sleep(Duration::from_secs(LIBRARY_ANNOUNCE_INTERVAL_SECS));
        });
    }

    fn start_known_peer_probe_loop(&self) {
        let service = self.clone();
        thread::spawn(move || {
            let mut round: u64 = 0;
            loop {
                round = round.wrapping_add(1);

                if let Ok(known_devices) = service.library.list_known_devices() {
                    for known in known_devices {
                        if let Ok(ip) = known.ip.parse::<IpAddr>() {
                            let source = if known.pinned {
                                CandidateSource::Manual
                            } else {
                                CandidateSource::Introduced
                            };
                            service.remember_candidate_ip(ip, source);
                            let packet_type = if round % 4 == 0 {
                                "library"
                            } else {
                                "presence"
                            };
                            let packet = service.packet(packet_type, Some(&known.device_id), true);
                            let _ =
                                send_discovery_packet(&packet, SocketAddr::new(ip, DISCOVERY_PORT));
                        }
                    }
                }

                let candidates = service.candidate_targets();
                for (ip, source) in candidates {
                    let packet_type = if source == CandidateSource::Manual || round % 4 == 0 {
                        "library"
                    } else {
                        "presence"
                    };
                    let packet = service.packet(packet_type, None, true);
                    let _ = send_discovery_packet(&packet, SocketAddr::new(ip, DISCOVERY_PORT));
                }

                thread::sleep(Duration::from_secs(KNOWN_PROBE_INTERVAL_SECS));
            }
        });
    }

    fn start_listen_loop(&self) {
        let app = self.app.clone();
        let devices = self.devices.clone();
        let own_id = self.device_id.clone();
        let service = self.clone();
        thread::spawn(move || {
            let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT)) {
                Ok(socket) => socket,
                Err(err) => {
                    eprintln!("UDP discovery failed on {DISCOVERY_PORT}: {err}");
                    return;
                }
            };
            let mut buf = [0_u8; 8192];

            loop {
                let Ok((len, addr)) = socket.recv_from(&mut buf) else {
                    continue;
                };
                let Ok(packet) = serde_json::from_slice::<DiscoveryPacket>(&buf[..len]) else {
                    continue;
                };
                if !packet.is_quicklan() || packet.device_id == own_id {
                    continue;
                }

                let remote_ip = addr.ip();
                let discovered_via = service.candidate_source_for_ip(&remote_ip);
                let is_manual = matches!(discovered_via, Some(CandidateSource::Manual));
                let is_introduced = matches!(discovered_via, Some(CandidateSource::Introduced));
                let discovered_via_label = if is_manual {
                    "manual"
                } else if is_introduced {
                    "introduced"
                } else {
                    "broadcast"
                };

                let response = service.packet("presence", Some(&packet.device_id), true);
                let _ = send_discovery_packet(&response, addr);

                service.remember_known_peer_hints(&packet.known_peers, &packet.device_id);

                let previous_avatar_hash = devices.lock().ok().and_then(|map| {
                    map.get(&packet.device_id)
                        .and_then(|entry| entry.info.avatar_hash.clone())
                });

                let info = DeviceInfo {
                    id: packet.device_id.clone(),
                    name: packet.device_name.clone(),
                    ip: remote_ip.to_string(),
                    tcp_port: packet.tcp_port,
                    api_port: packet.api_port,
                    online: true,
                    last_seen_ms: now_ms(),
                    share_count: packet.share_count,
                    library_version: packet.library_version,
                    manifest_hash: packet.manifest_hash.clone(),
                    upload_tasks: packet.upload_tasks,
                    latency_ms: None,
                    note: service.library.device_note(&packet.device_id),
                    avatar_hash: packet.avatar_hash.clone().or(previous_avatar_hash),
                    is_local: false,
                    is_known: true,
                    discovered_via: Some(discovered_via_label.to_string()),
                };

                let _ = service.library.observe_device(&packet, info.ip.clone());
                let _ = service.library.upsert_known_device_from_packet(
                    &packet,
                    &info.ip,
                    discovered_via_label,
                    is_manual,
                );
                let _ = service.library.touch_known_device_seen(
                    &packet.device_id,
                    &info.ip,
                    packet.tcp_port,
                    packet.api_port,
                    &packet.device_name,
                );
                service.consume_candidate_ip(&remote_ip);

                let has_existing_remote_owner =
                    service.library.has_active_remote_owner(&packet.device_id);
                let should_sync = match packet.packet_type.as_str() {
                    "library" => true,
                    "presence" => has_existing_remote_owner || is_manual || is_introduced,
                    _ => is_manual || is_introduced,
                };

                if let Ok(mut map) = devices.lock() {
                    map.insert(
                        info.id.clone(),
                        DeviceEntry {
                            info: info.clone(),
                            last_seen: Instant::now(),
                        },
                    );
                    let snapshot = service.snapshot_devices(&map);
                    let _ = app.emit("devices-updated", snapshot);
                }

                if should_sync {
                    let library = service.library.clone();
                    let app = app.clone();
                    let ip = info.ip.clone();
                    let tcp_port = packet.tcp_port;
                    let api_port = packet.api_port;
                    let device_id = packet.device_id.clone();
                    thread::spawn(move || {
                        if let Ok(manifest) = lan_api::fetch_manifest_blocking(&ip, api_port) {
                            let _ =
                                library.merge_manifest(manifest, ip.clone(), tcp_port, api_port);
                            let _ = library.mark_known_device_intro(&device_id);
                            let _ = app.emit(
                                "library-updated",
                                library.list_shared_resources().unwrap_or_default(),
                            );
                        }
                    });
                }
            }
        });
    }

    fn start_prune_loop(&self) {
        let service = self.clone();
        let app = self.app.clone();
        let devices = self.devices.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(5));
            let mut changed = false;
            let snapshot = if let Ok(mut map) = devices.lock() {
                for entry in map.values_mut() {
                    let online = entry.last_seen.elapsed() < Duration::from_secs(DEVICE_TTL_SECS);
                    if entry.info.online != online {
                        entry.info.online = online;
                        changed = true;
                    }
                }
                service.snapshot_devices(&map)
            } else {
                Vec::new()
            };
            if changed {
                let _ = app.emit("devices-updated", snapshot);
            }
        });
    }

    fn local_device_info(&self) -> DeviceInfo {
        let summary = self.library.summary();
        DeviceInfo {
            id: self.device_id.clone(),
            name: self.settings.nickname(),
            ip: "127.0.0.1".to_string(),
            tcp_port: self.tcp_port,
            api_port: self.api_port,
            online: true,
            last_seen_ms: now_ms(),
            share_count: summary.share_count,
            library_version: summary.library_version,
            manifest_hash: summary.manifest_hash,
            upload_tasks: 0,
            latency_ms: Some(0),
            note: self.library.device_note(&self.device_id),
            avatar_hash: self.settings.avatar_hash(),
            is_local: true,
            is_known: true,
            discovered_via: Some("local".to_string()),
        }
    }

    fn snapshot_devices(&self, map: &HashMap<String, DeviceEntry>) -> Vec<DeviceInfo> {
        let mut devices: Vec<DeviceInfo> = map.values().map(|entry| entry.info.clone()).collect();
        devices.push(self.local_device_info());
        devices.sort_by(|a, b| {
            b.is_local
                .cmp(&a.is_local)
                .then_with(|| b.online.cmp(&a.online))
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.ip.cmp(&b.ip))
        });
        devices
    }

    fn remember_known_peer_hints(&self, hints: &[KnownPeerHint], source_device_id: &str) {
        for hint in hints {
            if hint.device_id == self.device_id || hint.device_id == source_device_id {
                continue;
            }
            if let Ok(ip) = hint.ip.parse::<IpAddr>() {
                self.remember_candidate_ip(ip, CandidateSource::Introduced);
                let _ = self.library.upsert_known_device(
                    &hint.device_id,
                    &hint.device_name,
                    &hint.ip,
                    hint.tcp_port,
                    hint.api_port,
                    "introduced",
                    false,
                );
            }
        }
    }

    fn remember_candidate_ip(&self, target_ip: IpAddr, source: CandidateSource) {
        if target_ip.is_loopback() || self.is_local_ip(target_ip) {
            return;
        }
        if let Ok(mut candidates) = self.candidate_ips.lock() {
            let key = target_ip.to_string();
            let entry = candidates.entry(key).or_insert(CandidatePeer {
                source,
                last_seen: Instant::now(),
            });
            entry.source = entry.source.merge(source);
            entry.last_seen = Instant::now();
        }
    }

    fn consume_candidate_ip(&self, target_ip: &IpAddr) {
        if let Ok(mut candidates) = self.candidate_ips.lock() {
            candidates.remove(&target_ip.to_string());
        }
    }

    fn candidate_source_for_ip(&self, target_ip: &IpAddr) -> Option<CandidateSource> {
        self.candidate_ips.lock().ok().and_then(|candidates| {
            candidates
                .get(&target_ip.to_string())
                .map(|peer| peer.source)
        })
    }

    fn candidate_targets(&self) -> Vec<(IpAddr, CandidateSource)> {
        self.candidate_ips
            .lock()
            .map(|candidates| {
                candidates
                    .iter()
                    .filter_map(|(ip, peer)| {
                        ip.parse::<IpAddr>().ok().map(|addr| (addr, peer.source))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn is_local_ip(&self, ip: IpAddr) -> bool {
        local_ipv4_addresses()
            .into_iter()
            .any(|local_ip| IpAddr::V4(local_ip) == ip)
    }
}

fn broadcast_packet(packet: &DiscoveryPacket) -> Result<(), String> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .map_err(|err| format!("缁戝畾 UDP 骞挎挱澶辫触: {err}"))?;
    let _ = socket.set_broadcast(true);
    let payload =
        serde_json::to_vec(packet).map_err(|err| format!("缂栫爜鍙戠幇骞挎挱澶辫触: {err}"))?;
    for target in broadcast_targets() {
        let _ = socket.send_to(
            &payload,
            SocketAddr::new(IpAddr::V4(target), DISCOVERY_PORT),
        );
    }
    Ok(())
}

fn send_discovery_packet(packet: &DiscoveryPacket, target: SocketAddr) -> Result<(), String> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .map_err(|err| format!("缁戝畾 UDP 鎺㈡祴澶辫触: {err}"))?;
    let _ = socket.set_broadcast(true);
    let payload =
        serde_json::to_vec(packet).map_err(|err| format!("缂栫爜鍙戠幇鎺㈡祴澶辫触: {err}"))?;
    socket
        .send_to(&payload, target)
        .map_err(|err| format!("鍙戦€佸彂鐜版帰娴嬪け璐? {err}"))?;
    Ok(())
}

fn broadcast_targets() -> Vec<Ipv4Addr> {
    let mut targets = vec![Ipv4Addr::BROADCAST];
    if let Ok(ifaces) = get_if_addrs() {
        for iface in ifaces {
            let IfAddr::V4(v4) = iface.addr else {
                continue;
            };
            if v4.ip.is_loopback() {
                continue;
            }
            targets.push(
                v4.broadcast
                    .unwrap_or_else(|| subnet_broadcast(v4.ip, v4.netmask)),
            );
        }
    }
    targets.sort();
    targets.dedup();
    targets
}

fn local_ipv4_addresses() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    if let Ok(ifaces) = get_if_addrs() {
        for iface in ifaces {
            let IfAddr::V4(v4) = iface.addr else {
                continue;
            };
            if !v4.ip.is_loopback() {
                ips.push(v4.ip);
            }
        }
    }
    ips.sort();
    ips.dedup();
    ips
}

fn subnet_broadcast(ip: Ipv4Addr, mask: Ipv4Addr) -> Ipv4Addr {
    let ip = u32::from(ip);
    let mask = u32::from(mask);
    Ipv4Addr::from(ip | !mask)
}

pub fn load_or_create_device_id() -> String {
    let path = device_id_path();
    if let Ok(value) = fs::read_to_string(&path) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let id = Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, &id);
    id
}

fn device_id_path() -> PathBuf {
    storage::config_dir().join("device_id")
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}
