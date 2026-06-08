# QuickLAN

QuickLAN is a Windows 10/11 LAN file sharing and transfer desktop app created by xiaolong su.

## Overview

QuickLAN focuses on local-network device discovery, fast file transfer, and a distributed shared file library. Shared resources publish indexes and immutable shared-store snapshots instead of uploading original files to a central server.

Core ideas:

- LAN device discovery with UDP broadcast.
- Direct TCP file transfer with receiver confirmation for quick transfer.
- Distributed manifest sync for shared resources.
- Shared Store snapshots at `QuickLANData/shared_store/{file_hash}/content.bin`.
- Downloaders automatically become replica nodes after SHA256 verification.
- No central server dependency.

## Tech Stack

- Frontend: React, TypeScript, Vite
- Desktop: Tauri
- Backend: Rust
- Local database: SQLite
- Network: UDP broadcast, TCP transfer, LAN HTTP API

## Development

```bash
npm install
npm run typecheck
npm run app:build
```

Rust checks:

```bash
cd src-tauri
cargo check
cargo test
```

## Network Ports

- UDP discovery: `45454`
- TCP transfer: starts at `45455` and may fall back through a port range
- LAN HTTP API: starts at `45457` and may fall back through a port range

## Version

Current version: `0.1.1`
