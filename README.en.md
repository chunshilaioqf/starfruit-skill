# starfruit — Multi-Node WebSocket Collaboration Skill

![starfruit overview](docs/assets/starfruit-promo.png)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](master/)
[![Python](https://img.shields.io/badge/Python-3.8%2B-blue.svg)](nodes/python/)
[![Node.js](https://img.shields.io/badge/Node.js-%E2%89%A518-green.svg)](nodes/nodejs/)

⭐ **A star-topology message hub**: **one master node (Rust) + any number of worker nodes
(Rust / Python / Node.js)**. Built for LAN multi-node collaboration (multiple nodes per machine),
with encrypted / non-encrypted dual modes (a mode mismatch is **reported back to the peer**),
a read-only web dashboard (animated message flow), an offline queue, message history and wake hooks.

- **Runs as installed**: Rust components are built locally with `cargo build` (no prebuilt binaries);
  the Python / Node.js workers are **dependency-free**;
- **Runs in the background**: every node is a daemon detached from any agent session; wake hooks are
  left for the agent to wire up;
- **Open source**: licensed under [MIT](LICENSE); contributions are welcome (see [CONTRIBUTING.md](CONTRIBUTING.md)).

> This is the English translation of [README.md](README.md). If the two disagree, the Chinese
> version is authoritative.

---

## Table of contents

- [Key features](#key-features)
- [Architecture](#architecture)
- [Message flow (sequence)](#message-flow-sequence)
- [Connection & mode validation](#connection--mode-validation)
- [Quick start](#quick-start)
- [Detailed usage](#detailed-usage)
  - [Master options](#master-options-starfruit-master)
  - [Worker options](#worker-options)
  - [Run modes (plain / tls)](#run-modes-plain--tls)
  - [Bind address rules (must follow)](#bind-address-rules-must-follow)
  - [Three deployment scenarios](#three-deployment-scenarios)
  - [Wake hooks](#wake-hooks-agent-integration-point)
  - [Running as a daemon](#running-as-a-daemon)
  - [Read-only web dashboard](#read-only-web-dashboard)
- [Protocol summary](#protocol-summary)
- [Repository layout](#repository-layout)
- [Development & testing](#development--testing)
- [Documentation map](#documentation-map)
- [License](#license)

---

## Key features

| Feature | Description |
|---|---|
| **Star topology** | A single master node (hub) routes all messages; any number of workers connect; **one machine can host several nodes** |
| **Multi-language workers** | Three equivalent implementations in Rust / Python 3 / Node.js ≥18, identical protocol, pick any one |
| **Dual run modes** | `plain` (unencrypted `ws://`) and `tls` (encrypted `wss://`); on a mode mismatch the hub **reports the error to the peer** and disconnects |
| **Offline queue** | Messages for an offline peer are queued (200 FIFO entries per peer) and replayed once that peer reconnects and registers |
| **Message history** | In-memory ring buffer (5000 entries by default) plus an unbounded JSONL file; the dashboard pages through it |
| **Read-only dashboard** | Node topology, animated message flow, history and stats; **no auth, read-only**, exposes no write operations |
| **Wake hooks** | On `type=wake` a node runs a command / HTTP callback / sentinel file write, so an agent can be woken |
| **Daemon mode** | `--daemon` detaches from the terminal and writes a pidfile plus logs; systemd / nohup also documented |
| **No extra dependencies** | Workers use only their standard library (hand-rolled RFC6455); the hub embeds its static assets into the binary |

---

## Architecture

```mermaid
flowchart TB
    subgraph LAN["LAN / local machine (multiple nodes per host)"]
        subgraph MASTER["Master node starfruit-master (Rust, one per network)"]
            HUB["WS message hub<br/>--hub-addr 127.0.0.1:9501"]
            TLSX["TLS endpoint<br/>--mode tls (self-signed or own certs)"]
            QUEUE["Offline queue<br/>200 FIFO entries per peer"]
            HIST["Message history<br/>ring buffer + JSONL"]
            WAKE["Wake event dispatch"]
            WEB["Read-only dashboard<br/>--web-addr 127.0.0.1:9502<br/>HTTP + SSE"]
        end
        N1["worker-a<br/>Rust: starfruit-node"]
        N2["worker-b<br/>Python: starfruit_node.py"]
        N3["worker-c<br/>Node.js: starfruit-node.js"]
    end

    BROWSER["Browser (read-only observer)"]

    N1 -- "ws:// / wss://" --> HUB
    N2 -- "ws:// / wss://" --> HUB
    N3 -- "ws:// / wss://" --> HUB
    HUB --- TLSX
    HUB --- QUEUE
    HUB --- HIST
    HUB --- WAKE
    HIST --- WEB
    BROWSER -- "HTTP / SSE" --> WEB
```

**Roles**

| Role | Count | Responsibility |
|---|---|---|
| **Master (manager / hub)** | exactly one | Message routing, offline queue, history, TLS endpoint, read-only web dashboard, wake dispatch, built-in manager endpoint |
| **Worker** | any | Connects, registers, takes tasks, executes and reports; implemented in any language by an agent, script or tool |

> Node names are **unique network-wide**: reconnecting with the same name **evicts the old connection**
> (which receives a Close frame).

---

## Message flow (sequence)

```mermaid
sequenceDiagram
    autonumber
    participant A as worker-a
    participant M as master hub
    participant B as worker-b

    A->>M: register {from, role, mode, ver}
    M-->>A: registered {backlog, mode, ver}
    B->>M: register {from, role, mode, ver}
    M-->>B: registered {backlog, mode, ver}

    A->>M: {to: "worker-b", type: "task", text: "..."}
    alt worker-b online
        M->>B: deliver message
        M-->>A: sent
        B->>M: {to: "worker-a", type: "done", text: "..."}
        M->>A: deliver result
        M-->>B: sent
    else worker-b offline
        M-->>A: queued
        Note over M: Queue the message (200 FIFO entries per peer)
        B->>M: register
        M->>B: replay queued messages
        M-->>A: queued_delivered (receipt goes to the original sender)
    end

    A->>M: {to: "all", type: "notice", text: "broadcast"}
    M-->>A: broadcast_ok (detail carries the target count)
    M->>B: broadcast delivery (a sender never receives its own broadcast)

    A->>M: {to: "worker-b", type: "wake", text: "wake up"}
    M->>B: deliver wake message
    Note over B: Trigger the wake hook<br/>--wake-cmd / --wake-url / wake.log
```

---

## Connection & mode validation

The master validates encrypted / non-encrypted compatibility with **two independent layers**
(see [docs/protocol.md](docs/protocol.md) §2.1):

```mermaid
flowchart TD
    C["Worker connects"] --> P{"Master peeks first byte"}
    P -->|"first byte = 0x16<br/>(TLS ClientHello)"| S1{"Master mode"}
    P -->|"first byte != 0x16<br/>(plaintext HTTP/WS)"| S2{"Master mode"}

    S1 -->|"plain"| E1["Reply with a plaintext error line<br/>STARFRUIT_ERR mode_mismatch<br/>then disconnect"]
    S1 -->|"tls"| OK1["Continue the TLS handshake"]

    S2 -->|"tls"| E2["Reply with a plaintext error line<br/>STARFRUIT_ERR mode_mismatch<br/>then disconnect"]
    S2 -->|"plain"| OK2["Continue the HTTP upgrade"]

    OK1 --> R["Read the register frame<br/>register{mode, ver}"]
    OK2 --> R
    R --> M{"register.mode == server mode ?"}
    M -->|"no"| E3["Reply mode_mismatch, then close"]
    M -->|"yes"| DONE["registered, start exchanging messages"]
```

---

## Quick start

> The full skill description is in [SKILL.md](SKILL.md) (Chinese); daemon and wake details are in
> [docs/daemon.md](docs/daemon.md) (Chinese).

### 1) Build (Rust components, run locally)

```bash
cd master      && cargo build --release     # produces master/target/release/starfruit-master
cd nodes/rust  && cargo build --release     # produces nodes/rust/target/release/starfruit-node
```

> The Python / Node.js workers need **no build step** — just run them with the corresponding interpreter.

### 2) Start the master (the single manager)

```bash
# Local-only by default (safest): hub 127.0.0.1:9501 + dashboard 127.0.0.1:9502
./master/target/release/starfruit-master --mode plain --daemon

# Encrypted mode: a self-signed certificate is generated on first start (or supply --cert/--key)
./master/target/release/starfruit-master --mode tls --daemon

# Other machines on the LAN need to join: bind your LAN IP (do not use 0.0.0.0)
./master/target/release/starfruit-master --mode plain --hub-addr 192.168.1.10:9501 --daemon
```

On success the master prints a **startup notice** in the session: master name, run mode, hub and
dashboard addresses, data directory, join commands for the three worker implementations, and the
bind-address rules.

### 3) Start workers (pick any implementation — they are equivalent)

```bash
# Rust
./nodes/rust/target/release/starfruit-node --name worker-a --mode plain \
  --wake-cmd 'echo "$SF_FROM: $SF_TEXT" >> /path/to/agent-inbox.log'

# Python (dependency-free)
python3 nodes/python/starfruit_node.py --name worker-b --mode plain

# Node.js (dependency-free)
node nodes/nodejs/starfruit-node.js --name worker-c --mode plain
```

> **Which implementation to use is the agent's decision, based on the machine's environment**:
> use Rust where a Rust toolchain exists, Python where only Python is available, Node.js where only
> Node is available. All three speak the exact same protocol.

### 4) Open the dashboard

```bash
open http://127.0.0.1:9502     # https:// under tls mode; the browser must trust the self-signed cert
```

---

## Detailed usage

### Master options (`starfruit-master`)

| Option | Default | Description |
|---|---|---|
| `--mode plain\|tls` | `plain` | Run mode: unencrypted `ws://` / encrypted `wss://` |
| `--hub-addr <addr>` | `127.0.0.1:9501` | Bind address of the WS hub (`0.0.0.0` is **rejected**) |
| `--web-addr <addr>` | `127.0.0.1:9502` | Bind address of the read-only dashboard (`0.0.0.0` is **rejected**) |
| `--cert <path>` | auto-generated | TLS certificate PEM (a self-signed cert is created under `--data-dir` by default) |
| `--key <path>` | auto-generated | TLS private key PEM (provide together with `--cert`) |
| `--data-dir <dir>` | `./starfruit-data` | Runtime data directory (logs / pid / history / wake) |
| `--manager-name <name>` | `manager` | Registration name of the built-in manager endpoint |
| `--wake-cmd <shell>` | none | Wake hook: run a command (`SF_FROM/SF_TO/SF_TEXT/SF_TS` injected) |
| `--wake-url <url>` | none | Wake hook: HTTP POST JSON |
| `--history-cap <n>` | `5000` | In-memory history entries for the dashboard (the JSONL file is unbounded) |
| `--daemon` | off | Run in the background (detached; logs to `data-dir/starfruit-master.log`) |
| `-h, --help` | — | Show help |

### Worker options

All three implementations share the same options:

| Option | Default | Description |
|---|---|---|
| `--server <url>` | `ws://127.0.0.1:9501` | Master address (`ws://` or `wss://`; use the master's LAN IP / tunnel domain across machines) |
| `--name <name>` | required | Node name, **unique network-wide** (reconnecting with the same name evicts the old connection) |
| `--role <role>` | `worker` | Role label (`manager` / `worker`, display only) |
| `--mode plain\|tls` | `plain` | **Must match the master**, otherwise the master reports an error and disconnects |
| `--data-dir <dir>` | `./starfruit-data` | Runtime data directory (logs / pid / `wake.log`) |
| `--wake-cmd <sh>` | none | Wake hook: shell command (`SF_FROM/SF_TO/SF_TEXT/SF_TS` injected) |
| `--wake-url <url>` | none | Wake hook: HTTP POST JSON `{"from","to","text","ts"}` |
| `--ca-cert <path>` | none | TLS CA certificate (self-signed certs are trusted by default, which suits LAN use) |
| `--daemon` | off | Run in the background (detached; pidfile + logs) |
| `-h, --help` | — | Show help |

### Run modes (plain / tls)

| Mode | Transport | Suitable for |
|---|---|---|
| `plain` | `ws://` | Trusted LAN / local machine only |
| `tls` | `wss://` | Across machines, public exposure (tunnel) or untrusted networks |

- The master's `--mode` selects the service mode; a worker's `--mode` **must match**, otherwise the
  master **reports the error to that worker** (a transport-level error line plus an application-level
  `mode_mismatch` receipt — two independent layers, see [protocol §2.1](docs/protocol.md)).
- In `tls` mode a self-signed certificate is generated on first start and can be replaced with your own
  via `--cert` / `--key`; workers trust self-signed certificates by default, or can pin a CA with `--ca-cert`.

### Bind address rules (must follow)

| Scenario | `--hub-addr` / `--web-addr` | Notes |
|---|---|---|
| Local multi-agent use only | `127.0.0.1` (**default**) | Safest; other machines cannot connect |
| Multi-machine LAN collaboration | the host's LAN IP (e.g. `192.168.1.10`) | Lets other LAN machines join |
| Public access required | local address + **Cloudflare Tunnel** | The tunnel exposes it publicly; `--mode tls` is **required** |

> ⛔ **Never bind `0.0.0.0`** (nor `::`): the master **refuses** wildcard addresses and exits with
> code 1, preventing accidental exposure on every interface. When bound to a non-local address in
> `plain` mode, the master additionally prints a `[warn]` line.

### Three deployment scenarios

**Scenario A: local multi-agent (default, safest)**

```bash
# Master: bind 127.0.0.1 only
./master/target/release/starfruit-master --mode plain --daemon

# Workers on the same machine (names must be unique)
python3 nodes/python/starfruit_node.py --name agent-1 --mode plain --daemon
node    nodes/nodejs/starfruit-node.js --name agent-2 --mode plain --daemon
```

**Scenario B: multi-machine LAN collaboration**

```bash
# Master: bind the host's LAN IP (example: 192.168.1.10)
./master/target/release/starfruit-master --mode plain \
  --hub-addr 192.168.1.10:9501 --web-addr 192.168.1.10:9502 --daemon

# Workers on other machines: point --server at the master's LAN IP
python3 nodes/python/starfruit_node.py --name worker-b --mode plain \
  --server ws://192.168.1.10:9501 --daemon
```

**Scenario C: public access (Cloudflare Tunnel + encryption)**

```bash
# Master: encrypted mode, still bound to a local address (the tunnel forwards traffic;
# do not bind 0.0.0.0)
./master/target/release/starfruit-master --mode tls --hub-addr 127.0.0.1:9501 --daemon

# Expose local port 9501 through Cloudflare Tunnel (example)
cloudflared tunnel --url http://127.0.0.1:9501
# Remote workers connect to the tunnel domain over wss:// with --mode tls
```

> Public exposure **must** use `tls`; `plain` is only advisable on a trusted LAN.

### Wake hooks (agent integration point)

When a node receives a `type=wake` message, it triggers one of the configured hooks:

| Kind | Configuration | Behaviour |
|---|---|---|
| Command | `--wake-cmd '<shell>'` | Runs as a child process; `SF_FROM` / `SF_TO` / `SF_TEXT` / `SF_TS` are injected |
| HTTP | `--wake-url <url>` | `POST` JSON `{"from","to","text","ts"}` |
| Sentinel file | on by default | Appends one JSONL line to `<data-dir>/wake.log`; an agent can `tail -f` it to wake a session |

**Recommended integration**: point `--wake-cmd` at your own wake entrypoint (the CLI / script that
starts a session), or watch `wake.log`. The node itself is not tied to any specific agent.

### Running as a daemon

```bash
# Master in the background (writes data-dir/starfruit-master.pid and starfruit-master.log)
./master/target/release/starfruit-master --mode plain --daemon

# Worker in the background (writes data-dir/<name>.pid and <name>.log)
python3 nodes/python/starfruit_node.py --name worker-b --mode plain --daemon
```

Processes are independent of any agent session; systemd unit examples and the nohup approach are in
[docs/daemon.md](docs/daemon.md) (Chinese).

### Read-only web dashboard

- URL: `http://127.0.0.1:9502` (`https://` in `tls` mode);
- **No auth, read-only**: node topology (canvas particle animation), message flow, history
  (paging / filtering) and stats;
- Main endpoints: `GET /`, `GET /api/nodes`, `GET /api/messages?limit=&before=`, `GET /events` (SSE).

---

## Protocol summary

See [docs/protocol.md](docs/protocol.md) for the full starfruit protocol v2.

- **Frame**: JSON text `{"from","to","type","text","ts"}`; `to` accepts `"all"` for broadcast
  (a sender never receives its own broadcast);
- **Types**: `task / status / done / blocked / idle / notice / chat / wake`;
- **Register**: first frame `{"action":"register","from":"<name>","role":"worker|manager","mode":"plain|tls","ver":2}`;
- **Receipts**: `registered / sent / queued / queued_delivered / broadcast_ok / no_such_peer / bad_msg / warn / mode_mismatch`;
- **Offline queue**: messages for an offline peer are queued (200 FIFO entries per peer) and delivered
  after it registers; broadcasts are not replayed.

---

## Repository layout

```
starfruit/
├── SKILL.md / README.md / README.en.md
├── LICENSE / CHANGELOG.md / VERSION / TODO.md
├── CONTRIBUTING.md / CODE_OF_CONDUCT.md / SECURITY.md
├── docs/            # protocol, methodology, plan, naming, daemon, workflow
│   └── assets/      # static assets such as the README promo image
├── master/          # master node (Rust): hub + TLS + history + web dashboard + daemon
├── nodes/
│   ├── rust/        # Rust worker (cargo build)
│   ├── python/      # Python worker (dependency-free)
│   └── nodejs/      # Node.js worker (dependency-free, Node >= 18)
├── tests/           # end-to-end script (tests/e2e.sh) and dual-mode check
└── reference/       # archived history (legacy ws-hub v0.2.0, original protocol/notes)
```

---

## Development & testing

```bash
# Master: build / lint / unit tests
cd master      && cargo build --release && cargo clippy -- -D warnings && cargo test

# Rust worker
cd nodes/rust  && cargo build --release && cargo clippy -- -D warnings && cargo test

# End-to-end test (master + workers in three languages: register, send/receive,
# offline queue, bind-address rules, ...)
bash tests/e2e.sh
```

Project conventions (commit messages, documentation language, verification requirements) are in
[docs/workflow-optimized.md](docs/workflow-optimized.md) (Chinese).

---

## Documentation map

| Document | Contents |
|---|---|
| [SKILL.md](SKILL.md) | Skill entry point: components, quickstart, protocol summary, operations notes (Chinese) |
| [docs/protocol.md](docs/protocol.md) | starfruit protocol v2 (topology / dual mode / error semantics / receipts / wake) |
| [docs/2026-11-12-多Agent-WS协同开发全流程经验总结.md](docs/2026-11-12-多Agent-WS协同开发全流程经验总结.md) | Methodology source (manager / workers / inspection / acceptance / pitfalls) |
| [docs/skill-plan.md](docs/skill-plan.md) | Skill plan (feature list / design decisions / acceptance criteria) |
| [docs/naming.md](docs/naming.md) | Naming rationale (starfruit = a star-shaped cross-section = star topology) |
| [docs/daemon.md](docs/daemon.md) | Daemon operation (`--daemon` / systemd / nohup) and wake hooks |
| [docs/workflow-optimized.md](docs/workflow-optimized.md) | Workflow constraints for AI coding tools |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Contribution guide |
| [SECURITY.md](SECURITY.md) | Security policy and vulnerability reporting |

---

## License

Released under the [MIT License](LICENSE). Issues and pull requests are welcome; please read
[CONTRIBUTING.md](CONTRIBUTING.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) first.

For security issues, please report privately as described in [SECURITY.md](SECURITY.md).
