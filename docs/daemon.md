# 后台运行与唤醒钩子（daemon & wake）

> starfruit 所有节点（主节点 + 三语言子节点）都是**完全脱离 agent 会话的后台进程**；
> "唤醒对话"通过 wake 钩子留给 agent 自行对接。对应 SKILL 需求第 6 条。

## 1. 后台运行方式

### 1.1 `--daemon`（内置，推荐）

所有节点均支持 `--daemon`：脱离终端（`setsid` / `detached`）、stdio 重定向到
`<data-dir>/<名字>.log`、写 `<data-dir>/<名字>.pid`。

```bash
# 主节点（默认 --deploy local，仅监听 127.0.0.1）
./master/target/release/starfruit-master --deploy local --mode plain --daemon
# Rust 子节点
./nodes/rust/target/release/starfruit-node --name worker-a --daemon
# Python / Node.js 子节点
python3 nodes/python/starfruit_node.py --name worker-b --daemon
node nodes/nodejs/starfruit-node.js --name worker-c --daemon
```

停止进程：**只按 PID/端口精确清理**（实战教训：禁止宽模式 `pkill -f`）。

```bash
kill "$(cat starfruit-data/starfruit-master.pid)"
kill "$(cat starfruit-data/worker-a.pid)"
```

### 1.2 nohup

```bash
nohup ./master/target/release/starfruit-master --mode plain \
  >> starfruit-data/starfruit-master.log 2>&1 &
echo $! > starfruit-data/starfruit-master.pid
```

### 1.3 systemd（示例）

`/etc/systemd/system/starfruit-master.service`：

```ini
[Unit]
Description=starfruit master node
After=network.target

[Service]
Type=simple
WorkingDirectory=/opt/starfruit
ExecStart=/opt/starfruit/master/target/release/starfruit-master --mode plain --data-dir /var/lib/starfruit
Restart=on-failure
RestartSec=3

[Install]
WantedBy=multi-user.target
```

子节点同理替换 `ExecStart`（如 `starfruit-node --name worker-a --mode plain ...`）。
`systemctl enable --now starfruit-master` 开机自启。

## 2. 唤醒钩子（agent 对接口子）

节点收到 `type=wake` 消息时触发以下钩子（可叠加，全部可选）：

| 钩子 | 配置 | 行为 |
|---|---|---|
| 哨兵文件 | 默认开启 | 追加一行 JSONL 到 `<data-dir>/wake.log` |
| 命令 | `--wake-cmd '<shell>'` | 子进程执行；环境变量 `SF_FROM` `SF_TO` `SF_TEXT` `SF_TS` |
| HTTP | `--wake-url <url>` | POST JSON `{"from","to","text","ts"}` |

主节点（内置管理者端点 `manager`）同样支持以上钩子；此外提供文件双通道：

| 文件 | 方向 | 说明 |
|---|---|---|
| `<data-dir>/manager.inbox.jsonl` | hub → agent | 收到的每条消息追加一行 JSON |
| `<data-dir>/manager.outbox.jsonl` | agent → hub | agent 追加 `{"to","type","text"}`，节点自动以 `manager` 身份发出 |
| `<data-dir>/wake.log` | 唤醒哨兵 | `type=wake` 消息触发时追加 |

子节点同构：`<data-dir>/<名字>.inbox.jsonl` / `<名字>.outbox.jsonl`。

## 3. agent 对接建议

1. **唤醒入口**：把 `--wake-cmd` 指向唤醒脚本（例如写入 agent 的收件箱 / 调用拉起会话的 CLI），
   或 `tail -f <data-dir>/wake.log` 作为守望者；
2. **发消息**：向 `outbox.jsonl` 追加一行 JSON（`echo '{"to":"worker-a","type":"task","text":"..."}' >> outbox.jsonl`）；
3. **收消息**：tail `inbox.jsonl`；
4. 进程生命周期与 agent 会话无关——agent 重启/离线不影响节点在线与离线队列语义。

## 4. 运行拓扑速记

- 一机多节点：每进程一个 `--name`，端口共享主节点监听端口；
- 主节点全网唯一；子节点 `--mode` 必须与主节点一致（不一致时主节点会**向子节点报错**）；
- 面板只读：`http://127.0.0.1:10002`（默认仅本机可访问）。

### 部署模式与监听地址规则（务必遵守）

主节点 `--deploy local|lan|public`（默认 `local`）决定监听地址与安全约束；
默认端口 hub `10001`、Web 面板 `10002`，**所有模式端口必须 ≥ 10001**，被占用自动 `+1`（最多 100 次）。

| 部署模式 | `--hub-addr` / `--web-addr` | 说明 |
|---|---|---|
| `local` | `127.0.0.1`（默认） | 仅本机可用，最安全；tls/jwt 均可选 |
| `lan` | 本机局域网 IP（如 `192.168.x.x`） | 供局域网其它机器加入；**禁止 `0.0.0.0`**；tls/jwt 均可选 |
| `public` | **必须指定具体 IP** | 配 **Cloudflare Tunnel**（监听 `127.0.0.1` 作为隧道指向即可）；**必须 `--mode tls` 且强制 JWT** |

```bash
# 局域网：监听本机局域网 IP（不要用 0.0.0.0），端口 ≥ 10001
./master/target/release/starfruit-master --deploy lan --mode plain \
  --hub-addr 192.168.1.10:10001 --web-addr 192.168.1.10:10002 --daemon

# 公网：必须 tls + JWT；Cloudflare Tunnel 指向本机 127.0.0.1
./master/target/release/starfruit-master --deploy public --mode tls \
  --hub-addr 192.168.1.10:10001 --require-jwt --daemon
```

> ⛔ **禁止直接监听 `0.0.0.0`**（含 `::`）：`lan` / `public` 模式均会拒绝通配地址并提示正确做法；
> 明文监听非本机地址时会输出 `[warn]`（公网请用 Cloudflare Tunnel + `--mode tls`）。
> 启动成功后，会话中会打印启动说明（主节点名称、部署模式、监听/面板地址、子节点加入方式、本规则）。

### JWT 令牌（需要时开启）

主节点开启 JWT（`--require-jwt`，`public` 模式强制）时，令牌**只能由主节点签发**：

```bash
# 签发：打印令牌后即退出，不启动服务
./master/target/release/starfruit-master --issue-token worker-a --jwt-ttl 86400
# 子节点携带令牌注册（三语言一致；校验失败以退出码 3 退出）
./nodes/rust/target/release/starfruit-node --name worker-a --token '<令牌>' --daemon
```

- 算法 `HS256`，`iss=starfruit-master`，`sub=<节点名>`，含 `iat` / `exp`（默认 `--jwt-ttl=86400` 秒）；
- 令牌**不可跨节点借用**（校验 `sub` 一致性），过期同样拒绝；
- 密钥由 `--jwt-secret` 指定，或缺省自动生成 `<data-dir>/jwt-secret`（32 字节随机 hex，权限 `0600`）。
