/* starfruit panel —— 只读可视化：拓扑动画 + 实时流 + 历史弹窗 + 中英双语 */
/* 零依赖、零构建。数据来源：/api/* 轮询 + /events SSE。 */

(() => {
  "use strict";

  // ─────────────────────────── i18n ───────────────────────────
  const I18N = {
    zh: {
      subtitle: "多节点 WS 协同面板", mode: "模式", online: "在线节点", msgs: "消息总数",
      uptime: "运行时长", history: "消息历史", nodes: "节点", topology: "消息流转拓扑",
      feed: "实时消息", feedHint: "最新 60 条", legendHub: "主节点 hub", legendOnline: "在线",
      legendOffline: "离线", legendFlow: "消息流动", topoHint: "点击节点查看最近消息",
      from: "从", type: "类型", contains: "包含", all: "全部", filterPh: "过滤文本…",
      loadMore: "加载更早", thTime: "时间", thFrom: "发送方", thTo: "接收方", thType: "类型",
      thStatus: "状态", thContent: "内容", foot: "无鉴权 · 全部只读",
      connecting: "连接中…", live: "实时连接", reconnecting: "重连中…",
      tls: "TLS 加密", plain: "plain 明文", recv: "收", sent: "发", backlog: "积压",
      colorMono: "单色配色", colorMulti: "多彩配色", noMessages: "暂无",
      recentMsgs: "最近 5 条消息", loaded: "已加载", current: "当前筛选", items: "条",
      nodesUnit: "个", updateFound: "发现新版本", currentVer: "当前",
      viewRepo: "前往仓库", updateHint: "运行 scripts/starfruit-update.sh 更新（含数据迁移检查）",
    },
    en: {
      subtitle: "Multi-node WS collaboration panel", mode: "Mode", online: "Online", msgs: "Messages",
      uptime: "Uptime", history: "History", nodes: "Nodes", topology: "Message Topology",
      feed: "Live Feed", feedHint: "latest 60", legendHub: "master hub", legendOnline: "online",
      legendOffline: "offline", legendFlow: "message flow", topoHint: "click a node for recent messages",
      from: "From", type: "Type", contains: "Contains", all: "All", filterPh: "filter text…",
      loadMore: "Load older", thTime: "Time", thFrom: "From", thTo: "To", thType: "Type",
      thStatus: "Status", thContent: "Content", foot: "no auth · read-only",
      connecting: "Connecting…", live: "Live", reconnecting: "Reconnecting…",
      tls: "TLS encrypted", plain: "plain text", recv: "in", sent: "out", backlog: "queued",
      colorMono: "Mono color", colorMulti: "Multi color", noMessages: "None yet",
      recentMsgs: "Last 5 messages", loaded: "Loaded", current: "filtered", items: "",
      nodesUnit: "", updateFound: "New version available", currentVer: "current",
      viewRepo: "Open repository", updateHint: "run scripts/starfruit-update.sh (with data migration check)",
    },
  };
  let lang = localStorage.getItem("sf-lang") === "en" ? "en" : "zh";
  const t = (k) => I18N[lang][k] ?? I18N.zh[k] ?? k;

  function applyLang() {
    document.documentElement.lang = lang === "zh" ? "zh-CN" : "en";
    document.querySelectorAll("[data-i18n]").forEach((el) => { el.textContent = t(el.dataset.i18n); });
    document.querySelectorAll("[data-i18n-ph]").forEach((el) => { el.placeholder = t(el.dataset.i18nPh); });
    $("lang-label").textContent = lang === "zh" ? "EN" : "中文";
    $("color-btn").textContent = state.colorMode === "mono" ? t("colorMulti") : t("colorMono");
    renderNodes(); renderFeed(); renderHistory(); refreshStatsText(); updateLiveText();
  }

  // ─────────────────────────── 全局状态 ───────────────────────────
  const state = {
    nodes: new Map(),
    feed: [],
    history: [],
    historyTotal: 0,
    oldestSeq: null,
    stats: {},
    mode: "plain",
    colorMode: localStorage.getItem("sf-color") === "multi" ? "multi" : "mono",
    sseOn: false,
  };

  const $ = (id) => document.getElementById(id);

  // ─────────────────────────── 数据拉取 ───────────────────────────
  async function fetchJSON(url) {
    const r = await fetch(url);
    return r.json();
  }

  async function refreshNodes() {
    const data = await fetchJSON("/api/nodes");
    state.nodes.clear();
    for (const n of data.nodes || []) state.nodes.set(n.name, n);
    renderNodes();
    renderFromFilter();
    layoutTopo();
  }

  async function refreshStats() {
    state.stats = await fetchJSON("/api/stats");
    refreshStatsText();
  }

  function refreshStatsText() {
    const s = state.stats;
    $("stat-online").textContent = `${s.online ?? 0} / ${s.registered ?? 0}`;
    $("stat-msgs").textContent = fmtNum(s.total_messages ?? 0);
    $("stat-uptime").textContent = fmtDur(s.uptime_sec ?? 0);
    $("stat-mode").textContent = state.mode === "tls" ? t("tls") : t("plain");
    $("foot-mode").textContent = `mode=${state.mode}`;
    if (s.repo_url) {
      $("repo-link").href = s.repo_url;
      $("update-link").href = s.repo_url;
    }
    $("foot-version").textContent = s.version ? `v${s.version}` : "";
    const up = s.update;
    if (up && up.update_available) {
      $("update-text").textContent =
        `⭐ ${t("updateFound")} v${up.latest_version}（${t("currentVer")} v${up.current_version}）· ${t("updateHint")}`;
      $("update-banner").hidden = false;
    } else {
      $("update-banner").hidden = true;
    }
  }

  async function fetchHistory(limit = 120, before = "") {
    const url = `/api/messages?limit=${limit}` + (before ? `&before=${before}` : "");
    const data = await fetchJSON(url);
    const msgs = data.messages || []; // 服务端返回新→旧
    state.historyTotal = data.total ?? state.historyTotal;
    for (const m of msgs.reverse()) {
      if (!state.history.some((x) => x.seq === m.seq)) state.history.push(m);
    }
    state.history.sort((a, b) => a.seq - b.seq);
    if (state.history.length) state.oldestSeq = state.history[0].seq;
    renderHistory();
  }

  // ─────────────────────────── 实时事件（SSE） ───────────────────────────
  function updateLiveText() {
    $("live-state").textContent = state.sseOn ? t("live") : t("reconnecting");
  }

  function connectSSE() {
    const es = new EventSource("/events");
    es.onopen = () => {
      state.sseOn = true;
      $("live-dot").className = "live-dot on";
      updateLiveText();
    };
    es.onerror = () => {
      state.sseOn = false;
      $("live-dot").className = "live-dot off";
      updateLiveText();
    };
    es.onmessage = (e) => {
      let ev;
      try { ev = JSON.parse(e.data); } catch { return; }
      if (ev.ev === "message") onMessage(ev.data);
      else if (ev.ev === "node_join" || ev.ev === "node_leave") {
        refreshNodes();
        refreshStats();
        const now = performance.now();
        if (ev.ev === "node_join") {
          topo.hubBlinkUntil = now + 10000;   // 新节点上线：HUB 闪烁 10s
          if (ev.data && ev.data.name) flashNode(ev.data.name);
        } else {
          topo.hubJitterUntil = now + 10000;  // 节点离线：HUB 红色剧烈跳动 10s
        }
      } else if (ev.ev === "update") {
        refreshStats();
      }
    };
  }

  function onMessage(m) {
    state.feed.unshift(m);
    if (state.feed.length > 60) state.feed.pop();
    renderFeed();
    if (popupNode) renderPopup(popupNode);

    if (m.seq && !state.history.some((x) => x.seq === m.seq)) {
      state.history.push(m);
      state.historyTotal = Math.max(state.historyTotal, m.seq);
      if (!$("history-mask").hidden) renderHistory();
    }

    // 拓扑：消息沿 src→hub→dst 的折线连续流动（在 hub 处拐弯）+ 工作状态 + HUB 快转 5s
    const now = performance.now();
    topo.hubSpinUntil = now + 5000;
    const src = m.from;
    const dst = m.to;
    if (dst === "all") {
      for (const name of state.nodes.keys()) {
        if (name !== src) spawnPath([src, "hub", name]);
      }
    } else {
      spawnPath([src, "hub", dst]);
    }
    if (src) { flashNode(src); topo.activity.set(src, now); }
    if (dst && dst !== "all") { flashNode(dst); topo.activity.set(dst, now); }
    if (dst === "all") {
      for (const name of state.nodes.keys()) if (name !== src) topo.activity.set(name, now);
    }
    refreshStats();
  }

  // ─────────────────────────── 渲染：节点列表 ───────────────────────────
  function renderNodes() {
    const box = $("node-list");
    box.innerHTML = "";
    const names = [...state.nodes.keys()].sort();
    $("node-count").textContent = `${names.length} ${t("nodesUnit")}`.trim();
    for (const name of names) {
      const n = state.nodes.get(name);
      const card = document.createElement("div");
      card.className = "node-card" + (n.online ? "" : " offline");
      card.dataset.name = name;
      const dotCls = n.internal ? "internal" : (n.online ? "online" : "");
      const roleCls = n.role === "manager" ? "manager" : "";
      card.innerHTML = `
        <div class="node-top">
          <span class="status-dot ${dotCls}"></span>
          <span class="node-name">${esc(name)}</span>
          <span class="node-role ${roleCls}">${esc(n.role)}</span>
        </div>
        <div class="node-meta">
          <span>${t("sent")} <b>${fmtNum(n.msgs_out)}</b></span>
          <span>${t("recv")} <b>${fmtNum(n.msgs_in)}</b></span>
          <span>${t("backlog")} <b>${fmtNum(n.backlog)}</b></span>
          <span>mode <b>${esc(n.mode)}</b></span>
        </div>`;
      box.appendChild(card);
    }
  }

  function renderFromFilter() {
    const sel = $("f-from");
    const cur = sel.value;
    sel.innerHTML = "";
    const all = document.createElement("option");
    all.value = "";
    all.textContent = t("all");
    sel.appendChild(all);
    for (const name of [...state.nodes.keys()].sort()) {
      const o = document.createElement("option");
      o.value = name;
      o.textContent = name;
      sel.appendChild(o);
    }
    sel.value = cur;
  }

  // ─────────────────────────── 渲染：实时消息流 ───────────────────────────
  function renderFeed() {
    const box = $("feed");
    box.innerHTML = "";
    for (const m of state.feed) {
      const div = document.createElement("div");
      const kind = m.status === "broadcast" ? "broadcast" : (m.type || "chat");
      div.className = `feed-item ${kind}`;
      div.innerHTML = `
        <div class="feed-head">
          <span class="arrow">${esc(m.from)}</span>
          <span>→</span>
          <span class="arrow">${esc(m.to)}</span>
          <span class="type-badge ${esc(kind)}">${esc(kind)}</span>
          <span class="feed-time">${fmtTime(m.ts_ms)}</span>
        </div>
        <p class="feed-text">${esc(trunc(m.text, 160))}</p>`;
      box.appendChild(div);
    }
  }

  // ─────────────────────────── 渲染：历史表（弹窗内） ───────────────────────────
  function renderHistory() {
    const body = $("history-body");
    const fromF = $("f-from").value;
    const typeF = $("f-type").value;
    const textF = $("f-text").value.trim().toLowerCase();

    const rows = state.history.filter((m) => {
      if (fromF && m.from !== fromF) return false;
      if (typeF && (m.type || "chat") !== typeF) return false;
      if (textF && !String(m.text || "").toLowerCase().includes(textF)) return false;
      return true;
    }).slice(-300).reverse();

    body.innerHTML = "";
    for (const m of rows) {
      const tr = document.createElement("tr");
      const kind = m.type || "chat";
      tr.innerHTML = `
        <td>${m.seq ?? "-"}</td>
        <td>${fmtTime(m.ts_ms)}</td>
        <td><b>${esc(m.from)}</b></td>
        <td>${esc(m.to)}</td>
        <td><span class="type-badge ${esc(kind)}">${esc(kind)}</span></td>
        <td><span class="status-tag ${esc(m.status || "")}">${esc(m.status || "")}</span></td>
        <td class="cell-text">${esc(trunc(m.text, 220))}</td>`;
      body.appendChild(tr);
    }
    $("history-hint").textContent =
      `${t("loaded")} ${state.history.length} / ${state.historyTotal} ${t("items")} · ${t("current")} ${rows.length} ${t("items")}`.replace(/\s+/g, " ").trim();
  }

  // ─────────────────────────── 历史弹窗 ───────────────────────────
  function openHistory() {
    $("history-mask").hidden = false;
    if (!state.history.length) fetchHistory(120);
    else renderHistory();
  }
  function closeHistory() { $("history-mask").hidden = true; }

  // ─────────────────────────── 拓扑画布 ───────────────────────────
  const canvas = $("topo");
  const ctx = canvas.getContext("2d");
  const logoImg = new Image();
  logoImg.src = "/static/img/logo.svg";

  const topo = {
    w: 0, h: 0, cx: 0, cy: 0, r: 0,
    pos: new Map(),
    flash: new Map(),       // name -> ts（消息放大高亮，5s 缓慢回落）
    activity: new Map(),    // name -> ts（最近收发 → 工作中齿轮旋转）
    edgeFlash: new Map(),
    particles: [],
    hubSpinUntil: 0,        // 新消息：HUB 快转 5s
    hubBlinkUntil: 0,       // 新节点上线：HUB 闪烁 10s
    hubJitterUntil: 0,      // 节点离线：HUB 红色剧烈跳动 10s
    hubRot: 0,
    lastFrame: performance.now(),
    birth: performance.now(),
  };

  function resize() {
    const wrap = canvas.parentElement;
    const dpr = window.devicePixelRatio || 1;
    topo.dpr = dpr;
    topo.w = wrap.clientWidth;
    topo.h = wrap.clientHeight;
    canvas.width = topo.w * dpr;
    canvas.height = topo.h * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    topo.cx = topo.w / 2;
    topo.cy = topo.h / 2;
    topo.r = Math.min(topo.w, topo.h) * 0.36;
    layoutTopo();
  }

  function layoutTopo() {
    topo.pos.clear();
    topo.pos.set("hub", { x: topo.cx, y: topo.cy });
    const names = [...state.nodes.keys()].sort();
    const n = Math.max(names.length, 1);
    names.forEach((name, i) => {
      const ang = -Math.PI / 2 + (i * 2 * Math.PI) / n;
      topo.pos.set(name, {
        x: topo.cx + topo.r * Math.cos(ang),
        y: topo.cy + topo.r * Math.sin(ang),
      });
    });
  }

  function flashNode(name) { topo.flash.set(name, performance.now()); }

  // 折线路径粒子：src→hub→dst 连续运动，在 hub 处拐弯；全程 ease-in-out 加减速。
  const FLOW_DURATION_MS = 2600;
  const easeIO = (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);

  function spawnPath(names, delayMs = 0) {
    const go = () => {
      const pts = [];
      for (const n of names) {
        const p = topo.pos.get(n) || topo.pos.get("hub");
        if (!p) return;
        pts.push({ x: p.x, y: p.y });
      }
      if (pts.length < 2) return;
      const seg = [];
      let total = 0;
      for (let i = 1; i < pts.length; i++) {
        const L = Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y);
        seg.push(L);
        total += L;
      }
      if (total <= 0) return;
      topo.particles.push({ pts, seg, total, t: 0, dur: FLOW_DURATION_MS, color: "#57b6ff", r: 3.4 });
      const now = performance.now();
      topo.edgeFlash.set(names[0], now);
      topo.edgeFlash.set(names[names.length - 1], now);
    };
    if (delayMs > 0) setTimeout(go, delayMs); else go();
  }

  // 按折线累计长度取 frac(0..1) 处的坐标。
  function pointAt(pt, frac) {
    let d = frac * pt.total;
    for (let i = 0; i < pt.seg.length; i++) {
      if (d <= pt.seg[i] || i === pt.seg.length - 1) {
        const a = pt.pts[i], b = pt.pts[i + 1];
        const f = pt.seg[i] ? Math.min(1, d / pt.seg[i]) : 0;
        return { x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f };
      }
      d -= pt.seg[i];
    }
    return pt.pts[pt.pts.length - 1];
  }

  // ── 配色：单色（主节点/工作节点各一色）或多彩（名称哈希 → HSL） ──
  function nameHash(s) {
    let h = 0;
    for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0;
    return h;
  }

  function hslToHex(h, s, l) {
    const a = s * Math.min(l, 1 - l);
    const f = (n) => {
      const k = (n + h / 30) % 12;
      const c = l - a * Math.max(-1, Math.min(k - 3, Math.min(9 - k, 1)));
      return Math.round(255 * c).toString(16).padStart(2, "0");
    };
    return `#${f(0)}${f(8)}${f(4)}`;
  }

  function nodeColor(name, meta) {
    if (state.colorMode === "multi") return hslToHex(nameHash(name) % 360, 0.62, 0.6);
    return meta && meta.internal ? "#b48bff" : "#8fd94d";
  }

  // 字符颜色 = 节点背景反色，再向白色混合 75%。
  function charColor(bgHex) {
    const h = bgHex.replace("#", "");
    const mix = (i) => {
      const inv = 255 - parseInt(h.slice(i, i + 2), 16);
      return Math.round(inv + (255 - inv) * 0.75);
    };
    return `rgb(${mix(0)},${mix(2)},${mix(4)})`;
  }

  function drawGrid() {
    ctx.strokeStyle = "rgba(128,128,128,0.2)";
    ctx.lineWidth = 1;
    const step = 26;
    ctx.beginPath();
    for (let x = step; x < topo.w; x += step) { ctx.moveTo(x, 0); ctx.lineTo(x, topo.h); }
    for (let y = step; y < topo.h; y += step) { ctx.moveTo(0, y); ctx.lineTo(topo.w, y); }
    ctx.stroke();
  }

  function drawHub(now, sec) {
    const dt = Math.min(0.1, (now - topo.lastFrame) / 1000);
    const spinning = now < topo.hubSpinUntil;
    const blinking = now < topo.hubBlinkUntil;
    const jittering = now < topo.hubJitterUntil;
    topo.hubRot += dt * (spinning ? 7 : 0.35);

    let dx = 0, dy = 0;
    if (jittering) {
      dx = (Math.random() - 0.5) * 10;
      dy = (Math.random() - 0.5) * 10;
    }
    const x = topo.cx + dx, y = topo.cy + dy;

    // 光环（离线事件时变红）
    const pulse = 1 + 0.05 * Math.sin(sec * 2.2);
    const ringColor = jittering ? "255,107,107" : "195,233,75";
    for (let i = 0; i < 3; i++) {
      const rr = (34 + i * 15) * pulse;
      ctx.beginPath();
      ctx.arc(x, y, rr, 0, Math.PI * 2);
      ctx.strokeStyle = `rgba(${ringColor},${(jittering ? 0.4 : 0.16) - i * 0.04})`;
      ctx.lineWidth = jittering ? 2.4 : 1.4;
      ctx.stroke();
    }

    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(topo.hubRot);
    if (blinking) ctx.globalAlpha = 0.3 + 0.7 * Math.abs(Math.sin(now / 130));
    if (logoImg.complete && logoImg.naturalWidth > 0) {
      ctx.drawImage(logoImg, -27, -27, 54, 54);
    } else {
      ctx.fillStyle = jittering ? "#ff6b6b" : "#c3e94b";
      ctx.beginPath();
      ctx.arc(0, 0, 24 * pulse, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.restore();

    if (jittering) {
      ctx.save();
      ctx.globalAlpha = 0.35 + 0.3 * Math.abs(Math.sin(now / 90));
      ctx.fillStyle = "#ff6b6b";
      ctx.beginPath();
      ctx.arc(x, y, 30, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }

    ctx.fillStyle = jittering ? "#ff6b6b" : "#93a0b8";
    ctx.font = "700 10px Inter, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText("HUB", x, y + 42);
  }

  function drawNode(name, p, now, sec) {
    const meta = state.nodes.get(name);
    const online = meta ? meta.online : false;
    const working = online && now - (topo.activity.get(name) ?? -1e9) < 5000;
    const fl = topo.flash.get(name) ?? -1e9;
    const k = Math.max(0, 1 - (now - fl) / 5000);   // 5s 缓慢回落
    const breath = working ? 1 : 1 + 0.06 * Math.sin(sec * 2 + nameHash(name) % 7);
    const rr = 14 * (1 + 0.45 * k) * breath;
    const color = online ? nodeColor(name, meta) : "#55617a";

    ctx.save();
    ctx.translate(p.x, p.y);

    // 光晕：工作中稳定亮；空闲呼吸灯
    const haloA = working ? 0.28 + 0.2 * k : 0.1 + 0.08 * (0.5 + 0.5 * Math.sin(sec * 2 + nameHash(name) % 7)) + 0.25 * k;
    ctx.beginPath();
    ctx.arc(0, 0, rr + 8, 0, Math.PI * 2);
    ctx.fillStyle = hexAlpha(color, online ? haloA : 0.07);
    ctx.fill();

    // 工作中：旋转齿轮（8 齿）
    if (working) {
      ctx.save();
      ctx.rotate(now / 350);
      ctx.fillStyle = hexAlpha(color, 0.85);
      for (let i = 0; i < 8; i++) {
        ctx.save();
        ctx.rotate((i * Math.PI) / 4);
        ctx.fillRect(rr - 2, -2.6, 6, 5.2);
        ctx.restore();
      }
      ctx.restore();
    }

    ctx.beginPath();
    ctx.arc(0, 0, rr, 0, Math.PI * 2);
    ctx.fillStyle = color;
    ctx.fill();
    ctx.strokeStyle = online ? "rgba(255,255,255,0.45)" : "rgba(255,255,255,0.15)";
    ctx.lineWidth = 1.3;
    ctx.stroke();

    // 居中旋转的名称首字符：背景反色 + 75% 白，白色辉光
    ctx.save();
    ctx.rotate(working ? now / 500 : now / 2600);
    ctx.shadowColor = "rgba(255,255,255,0.9)";
    ctx.shadowBlur = 8;
    ctx.fillStyle = charColor(color);
    ctx.font = `700 ${Math.round(rr * 1.05)}px Inter, "PingFang SC", sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText([...name][0] || "?", 0, 1);
    ctx.restore();

    // 名称 + 工作状态（收/发计数）
    ctx.fillStyle = online ? "#eef3ff" : "#93a0b8";
    ctx.font = "600 11px Inter, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "alphabetic";
    ctx.fillText(trunc(name, 14), 0, rr + 16);
    if (meta) {
      ctx.fillStyle = working ? "#c3e94b" : "#93a0b8";
      ctx.font = "500 9.5px Inter, sans-serif";
      ctx.fillText(`${t("recv")} ${fmtNum(meta.msgs_in)} · ${t("sent")} ${fmtNum(meta.msgs_out)}`, 0, rr + 29);
    }
    ctx.restore();
  }

  function draw(now) {
    const t = (now - topo.birth) / 1000;
    // 每帧重置变换，避免任何中断累积污染后续帧。
    ctx.setTransform(topo.dpr || 1, 0, 0, topo.dpr || 1, 0, 0);
    ctx.clearRect(0, 0, topo.w, topo.h);
    drawGrid();

    // 辐条
    for (const [name, p] of topo.pos) {
      if (name === "hub") continue;
      const fl = topo.edgeFlash.get(name) ?? -1e9;
      const heat = Math.max(0, 1 - (now - fl) / 1100);
      const grad = ctx.createLinearGradient(topo.cx, topo.cy, p.x, p.y);
      grad.addColorStop(0, `rgba(195,233,75,${0.12 + heat * 0.5})`);
      grad.addColorStop(1, `rgba(87,182,255,${0.1 + heat * 0.45})`);
      ctx.strokeStyle = grad;
      ctx.lineWidth = 1.6 + heat * 2.2;
      ctx.beginPath();
      ctx.moveTo(topo.cx, topo.cy);
      ctx.lineTo(p.x, p.y);
      ctx.stroke();
    }

    drawHub(now, t);
    for (const [name, p] of topo.pos) {
      if (name !== "hub") drawNode(name, p, now, t);
    }

    // 消息粒子（折线拐弯 + 拖尾 + 全程加减速）
    const dtMs = Math.min(100, now - topo.lastFrame);
    for (let i = topo.particles.length - 1; i >= 0; i--) {
      const pt = topo.particles[i];
      pt.t += dtMs / pt.dur;
      if (pt.t >= 1) { topo.particles.splice(i, 1); continue; }
      const { x, y } = pointAt(pt, easeIO(pt.t));

      for (let k = 1; k <= 5; k++) {
        const tt = Math.max(0, pt.t - k * 0.02);
        const p2 = pointAt(pt, easeIO(tt));
        ctx.beginPath();
        ctx.arc(p2.x, p2.y, pt.r * (1 - k * 0.15), 0, Math.PI * 2);
        ctx.fillStyle = hexAlpha(pt.color, 0.16 - k * 0.026);
        ctx.fill();
      }

      const glow = ctx.createRadialGradient(x, y, 0, x, y, pt.r * 3.2);
      glow.addColorStop(0, hexAlpha(pt.color, 0.85));
      glow.addColorStop(1, hexAlpha(pt.color, 0));
      ctx.fillStyle = glow;
      ctx.beginPath();
      ctx.arc(x, y, pt.r * 3.2, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = pt.color;
      ctx.beginPath();
      ctx.arc(x, y, pt.r, 0, Math.PI * 2);
      ctx.fill();
    }

    topo.lastFrame = now;
    requestAnimationFrame(draw);
  }

  // ─────────────────────────── 节点点击弹窗（实时流最近 5 条） ───────────────────────────
  let popupNode = null;

  function renderPopup(name) {
    const box = $("node-popup");
    const msgs = state.feed
      .filter((m) => m.from === name || m.to === name || m.to === "all")
      .slice(0, 5);
    let html = `<div class="popup-head"><b>${esc(name)}</b>
      <span class="hint">${t("recentMsgs")}</span>
      <button class="modal-close" type="button" id="popup-close">✕</button></div>`;
    if (!msgs.length) {
      html += `<p class="popup-empty">${t("noMessages")}</p>`;
    } else {
      for (const m of msgs) {
        const kind = m.status === "broadcast" ? "broadcast" : (m.type || "chat");
        html += `<div class="popup-item">
          <div class="feed-head">
            <span class="arrow">${esc(m.from)}</span><span>→</span><span class="arrow">${esc(m.to)}</span>
            <span class="type-badge ${esc(kind)}">${esc(kind)}</span>
            <span class="feed-time">${fmtTime(m.ts_ms)}</span>
          </div>
          <p class="feed-text">${esc(trunc(m.text, 100))}</p>
        </div>`;
      }
    }
    box.innerHTML = html;
    $("popup-close").addEventListener("click", closePopup);
  }

  function closePopup() {
    popupNode = null;
    $("node-popup").hidden = true;
  }

  canvas.addEventListener("click", (e) => {
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    let hit = null;
    for (const [name, p] of topo.pos) {
      if (name === "hub") continue;
      if (Math.hypot(p.x - x, p.y - y) <= 24) { hit = name; break; }
    }
    if (!hit) { closePopup(); return; }
    popupNode = hit;
    renderPopup(hit);
    const box = $("node-popup");
    box.hidden = false;
    const p = topo.pos.get(hit);
    const bw = box.offsetWidth, bh = box.offsetHeight;
    let left = p.x + 26;
    let top = p.y - bh / 2;
    if (left + bw > topo.w - 8) left = p.x - bw - 26;
    left = Math.max(8, left);
    top = Math.max(8, Math.min(top, topo.h - bh - 8));
    box.style.left = `${left}px`;
    box.style.top = `${top}px`;
  });

  // ─────────────────────────── 工具函数 ───────────────────────────
  function esc(s) {
    return String(s ?? "").replace(/[&<>"']/g, (c) => ({
      "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
    })[c]);
  }

  function trunc(s, n) {
    s = String(s ?? "");
    return s.length > n ? s.slice(0, n) + "…" : s;
  }

  function fmtNum(n) {
    return Number(n || 0).toLocaleString(lang === "zh" ? "zh-CN" : "en-US");
  }

  function fmtTime(ms) {
    if (!ms) return "–";
    const d = new Date(Number(ms));
    const p = (x) => String(x).padStart(2, "0");
    return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  }

  function fmtDur(sec) {
    const h = Math.floor(sec / 3600);
    const m = Math.floor((sec % 3600) / 60);
    const s = Math.floor(sec % 60);
    if (h > 0) return `${h}h ${m}m`;
    if (m > 0) return `${m}m ${s}s`;
    return `${s}s`;
  }

  function hexAlpha(hex, a) {
    const h = hex.replace("#", "");
    const r = parseInt(h.slice(0, 2), 16);
    const g = parseInt(h.slice(2, 4), 16);
    const b = parseInt(h.slice(4, 6), 16);
    return `rgba(${r},${g},${b},${a})`;
  }

  // ─────────────────────────── 启动 ───────────────────────────
  window.addEventListener("resize", resize);
  $("load-more").addEventListener("click", () => {
    if (state.oldestSeq != null) fetchHistory(120, String(state.oldestSeq));
  });
  for (const id of ["f-from", "f-type"]) $(id).addEventListener("change", renderHistory);
  $("f-text").addEventListener("input", () => renderHistory());

  $("history-btn").addEventListener("click", openHistory);
  $("history-close").addEventListener("click", closeHistory);
  $("history-mask").addEventListener("click", (e) => {
    if (e.target === $("history-mask")) closeHistory();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") { closeHistory(); closePopup(); }
  });

  $("lang-btn").addEventListener("click", () => {
    lang = lang === "zh" ? "en" : "zh";
    localStorage.setItem("sf-lang", lang);
    applyLang();
    renderFromFilter();
  });

  $("color-btn").addEventListener("click", () => {
    state.colorMode = state.colorMode === "mono" ? "multi" : "mono";
    localStorage.setItem("sf-color", state.colorMode);
    $("color-btn").textContent = state.colorMode === "mono" ? t("colorMulti") : t("colorMono");
  });

  (async function boot() {
    resize();
    await refreshNodes();
    for (const n of state.nodes.values()) {
      if (n.mode === "tls" || n.mode === "plain") state.mode = n.mode;
    }
    await refreshStats();
    await fetchHistory(120);
    applyLang();
    connectSSE();
    requestAnimationFrame(draw);
    setInterval(() => { refreshStats(); refreshNodes(); }, 5000);
  })();
})();