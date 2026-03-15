# merlionos-playground-server

Backend server for MerlionOS browser playground. Manages a pool of QEMU instances with a queue system.

## Architecture

```
Browser ←WebSocket→ Server ←stdio→ QEMU instances
                      │
                 Pool + Queue
                 ├─ Max N concurrent instances
                 ├─ Session timeout (10 min)
                 ├─ Idle timeout (2 min)
                 └─ FIFO queue when full
```

## Modules

- `config.rs` — Environment-based configuration
- `qemu.rs` — QEMU process spawn/kill, serial I/O via stdin/stdout
- `pool.rs` — Instance pool with queue, session management, reaper
- `ws.rs` — WebSocket handler bridging browser ↔ QEMU serial
- `main.rs` — Axum HTTP server with /ws, /health, /status endpoints

## API

- `GET /ws` — WebSocket endpoint for playground sessions
- `GET /health` — Health check
- `GET /status` — Pool status (active, max, queue_length)

## WebSocket Protocol

### Server → Client
```json
{"type": "status", "active": 3, "max": 5, "queue_length": 0}
{"type": "queued", "position": 2, "message": "You are #2 in queue..."}
{"type": "ready", "session_id": "...", "timeout_secs": 600}
```
Binary frames: raw serial output bytes from QEMU

### Client → Server
```json
{"type": "input", "data": "ls\n"}
```
Binary frames: raw bytes forwarded to QEMU stdin

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `3020` | HTTP server port |
| `MAX_INSTANCES` | `5` | Max concurrent QEMU sessions |
| `SESSION_TIMEOUT` | `600` | Max session duration (seconds) |
| `IDLE_TIMEOUT` | `120` | Idle timeout (seconds) |
| `QEMU_BINARY` | `qemu-system-x86_64` | Path to QEMU |
| `KERNEL_IMAGE` | `./images/merlionos.bin` | Path to bootimage |
| `QEMU_MEMORY` | `128M` | RAM per QEMU instance |

## Build & Run

```bash
cargo build --release
cp ../merlion-kernel/target/x86_64-unknown-none/debug/bootimage-merlion-kernel.bin images/merlionos.bin
KERNEL_IMAGE=./images/merlionos.bin ./target/release/merlionos-playground-server
```
