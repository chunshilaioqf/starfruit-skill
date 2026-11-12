/* starfruit panel —— 只读可视化：拓扑 + 消息流动画（canvas 粒子）+ 实时流 + 历史 */
/* 零依赖、零构建。数据来源：/api/* 轮询 + /events SSE。 */

(() => {
  "use strict";

  // ─────────────────────────── 全局状态 ───────────────────────────
  const state = {
    nodes: new Map(),          // name -> node meta
    feed: [],                  // 最新消息（上限 60）
    history: [],               // 当前已加载历史（升序）
    historyTotal: 0,
    oldestSeq: null,           // 分页游标
    stats: {},
    mode: "plain",
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
    const s = await fetchJSON("/api/stats");
    state.stats = s;
    $("stat-online").textContent = `${s.online ?? 0} / ${s.registered ?? 0}`;
    $("stat-msgs").textContent = fmtNum(s.total_messages ?? 0);
    $("stat-uptime").textContent = fmtDur(s.uptime_sec ?? 0);
    $("stat-mode").textContent = state.mode === "tls" ? "TLS 加密" : "plain 明文";
    $("foot-mode").textContent = `mode=${state.mode}`;
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
    $("history-hint").textContent = `已加载 ${state.history.length} / ${state.historyTotal} 条`;
    renderHistory();
  }

  // ─────────────────────────── 实时事件（SSE） ───────────────────────────
  function connectSSE() {
    const es = new EventSource("/events");
    es.onopen = () => {
      $("live-dot").className = "live-dot on";
      $("live-state").textContent = "实时连接";
    };
    es.onerror = () => {
      $("live-dot").className = "live-dot off";
      $("live-state").textContent = "重连中…";
    };
    es.onmessage = (e) => {
      let ev;
      try { ev = JSON.parse(e.data); } catch { return; }
      if (ev.ev === "message") onMessage(ev.data);
      else if (ev.ev === "node_join" || ev.ev === "node_leave") {
        refreshNodes();
        refreshStats();
        if (ev.ev === "node_join" && ev.data && ev.data.name) flashNode(ev.data.name);
      }
    };
  }

  function onMessage(m) {
    // 实时消息流
    state.feed.unshift(m);
    if (state.feed.length > 60) state.feed.pop();
    renderFeed();

    // 历史
    if (m.seq && !state.history.some((x) => x.seq === m.seq)) {
      state.history.push(m);
      state.historyTotal = Math.max(state.historyTotal, m.seq);
      renderHistory();
    }

    // 拓扑动画：沿辐条流动（经 hub 中转两段）
    const fromHub = m.from === "manager" || m.from === state.hubName;
    const toAll = m.to === "all";
    const src = m.from;
    const dst = m.to;
    if (toAll) {
      for (const name of state.nodes.keys()) {
        if (name !== src) spawnFlow(name, src, 0);   // → hub
        if (name !== src) spawnFlow(src, name, 220); // hub → 全员
      }
    } else {
      spawnFlow(src, "hub", 0);
      spawnFlow("hub", dst, 200);
    }

    refreshStats();
    if (m.from) flashNode(m.from);
    if (dst && dst !== "all") flashNode(dst);
  }

  // ─────────────────────────── 渲染：节点列表 ───────────────────────────
  function renderNodes() {
    const box = $("node-list");
    box.innerHTML = "";
    const names = [...state.nodes.keys()].sort();
    $("node-count").textContent = `${names.length} 个`;
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
          <span>发 <b>${fmtNum(n.msgs_out)}</b></span>
          <span>收 <b>${fmtNum(n.msgs_in)}</b></span>
          <span>积压 <b>${fmtNum(n.backlog)}</b></span>
          <span>mode <b>${esc(n.mode)}</b></span>
        </div>`;
      box.appendChild(card);
    }
  }

  function renderFromFilter() {
    const sel = $("f-from");
    const cur = sel.value;
    sel.innerHTML = `<option value="">全部</option>`;
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

  // ─────────────────────────── 渲染：历史表 ───────────────────────────
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
    }).slice(-300).reverse(); // 最新在上，最多 300 行

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
      `已加载 ${state.history.length} / ${state.historyTotal} 条 · 当前筛选 ${rows.length} 条`;
  }

  // ─────────────────────────── 拓扑画布 + 消息流动画 ───────────────────────────
  const canvas = $("topo");
  const ctx = canvas.getContext("2d");
  const topo = {
    w: 0, h: 0, cx: 0, cy: 0, r: 0,
    pos: new Map(),        // name -> {x, y}
    flash: new Map(),      // name -> ts
    edgeFlash: new Map(),  // name -> ts
    particles: [],         // {x0,y0,x1,y1,t,speed,color,r}
    birth: performance.now(),
  };

  function resize() {
    const wrap = canvas.parentElement;
    const dpr = window.devicePixelRatio || 1;
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

  function spawnFlow(a, b, delayMs) {
    const go = () => {
      const pa = topo.pos.get(a) || topo.pos.get("hub");
      const pb = topo.pos.get(b) || topo.pos.get("hub");
      if (!pa || !pb) return;
      topo.particles.push({
        x0: pa.x, y0: pa.y, x1: pb.x, y1: pb.y,
        t: 0, speed: 1.35,
        color: b === "hub" ? "#c3e94b" : "#57b6ff",
        r: 3.4,
      });
      topo.edgeFlash.set(b === "hub" ? a : b, performance.now());
      if (a !== "hub") topo.edgeFlash.set(a, performance.now());
    };
    if (delayMs > 0) setTimeout(go, delayMs); else go();
  }

  function draw(now) {
    const t = (now - topo.birth) / 1000;
    ctx.clearRect(0, 0, topo.w, topo.h);

    // 辐条
    for (const [name, p] of topo.pos) {
      if (name === "hub") continue;
      const fl = topo.edgeFlash.get(name) || 0;
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

    // hub 光环
    const pulse = 1 + 0.05 * Math.sin(t * 2.2);
    for (let i = 0; i < 3; i++) {
      const rr = (30 + i * 16) * pulse;
      ctx.beginPath();
      ctx.arc(topo.cx, topo.cy, rr, 0, Math.PI * 2);
      ctx.strokeStyle = `rgba(195,233,75,${0.16 - i * 0.045})`;
      ctx.lineWidth = 1.4;
      ctx.stroke();
    }
    const hubGrad = ctx.createRadialGradient(topo.cx, topo.cy, 4, topo.cx, topo.cy, 34);
    hubGrad.addColorStop(0, "#d7f05a");
    hubGrad.addColorStop(1, "#6fbf3a");
    ctx.fillStyle = hubGrad;
    ctx.beginPath();
    ctx.arc(topo.cx, topo.cy, 24 * pulse, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = "#0b1020";
    ctx.font = "700 11px Inter, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText("HUB", topo.cx, topo.cy);

    // 节点
    for (const [name, p] of topo.pos) {
      if (name === "hub") continue;
      const meta = state.nodes.get(name);
      const online = meta ? meta.online : false;
      const internal = meta ? meta.internal : false;
      const fl = topo.flash.get(name) || 0;
      const hot = Math.max(0, 1 - (now - fl) / 900);
      const rr = 13 + hot * 4;

      ctx.beginPath();
      ctx.arc(p.x, p.y, rr + 7, 0, Math.PI * 2);
      ctx.fillStyle = internal
        ? `rgba(180,139,255,${0.1 + hot * 0.3})`
        : online
          ? `rgba(111,191,58,${0.1 + hot * 0.3})`
          : "rgba(147,160,184,0.08)";
      ctx.fill();

      ctx.beginPath();
      ctx.arc(p.x, p.y, rr, 0, Math.PI * 2);
      ctx.fillStyle = internal ? "#b48bff" : online ? "#8fd94d" : "#55617a";
      ctx.fill();
      ctx.strokeStyle = online ? "rgba(215,240,90,0.7)" : "rgba(255,255,255,0.15)";
      ctx.lineWidth = 1.3;
      ctx.stroke();

      ctx.fillStyle = online ? "#eef3ff" : "#93a0b8";
      ctx.font = "600 11px Inter, sans-serif";
      ctx.textAlign = "center";
      ctx.fillText(trunc(name, 14), p.x, p.y + rr + 15);
    }

    // 消息粒子（带拖尾）
    for (let i = topo.particles.length - 1; i >= 0; i--) {
      const pt = topo.particles[i];
      pt.t += 0.016 * pt.speed;
      if (pt.t >= 1) { topo.particles.splice(i, 1); continue; }
      const ease = pt.t < 0.5
        ? 2 * pt.t * pt.t
        : 1 - Math.pow(-2 * pt.t + 2, 2) / 2;
      const x = pt.x0 + (pt.x1 - pt.x0) * ease;
      const y = pt.y0 + (pt.y1 - pt.y0) * ease;

      // 拖尾
      for (let k = 1; k <= 5; k++) {
        const tt = Math.max(0, pt.t - k * 0.022);
        const e2 = tt < 0.5 ? 2 * tt * tt : 1 - Math.pow(-2 * tt + 2, 2) / 2;
        const tx = pt.x0 + (pt.x1 - pt.x0) * e2;
        const ty = pt.y0 + (pt.y1 - pt.y0) * e2;
        ctx.beginPath();
        ctx.arc(tx, ty, pt.r * (1 - k * 0.15), 0, Math.PI * 2);
        ctx.fillStyle = hexAlpha(pt.color, 0.16 - k * 0.026);
        ctx.fill();
      }

      // 主体 + 辉光
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

    requestAnimationFrame(draw);
  }

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
    return Number(n || 0).toLocaleString("zh-CN");
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

  (async function boot() {
    resize();
    // 模式：从 stats 侧的节点信息推断（首个非 local 节点），否则默认 plain。
    await refreshNodes();
    for (const n of state.nodes.values()) {
      if (n.mode === "tls" || n.mode === "plain") state.mode = n.mode;
    }
    await refreshStats();
    await fetchHistory(120);
    renderFeed();
    connectSSE();
    requestAnimationFrame(draw);
    setInterval(() => { refreshStats(); refreshNodes(); }, 5000);
  })();
})();
