(() => {
  const $ = (id) => document.getElementById(id);

  const els = {
    linkState: $("linkState"),
    modeChip: $("modeChip"),
    kpis: $("kpis"),
    btnStart: $("btnStart"),
    btnStop: $("btnStop"),
    target: $("target"),
    method: $("method"),
    concurrency: $("concurrency"),
    duration: $("duration"),
    rps: $("rps"),
    timeout: $("timeout"),
    warmup: $("warmup"),
    body: $("body"),
    chart: $("latencyChart"),
    p99Label: $("p99Label"),
    totalLabel: $("totalLabel"),
    statusBars: $("statusBars"),
    runId: $("runId"),
    log: $("log"),
  };

  const kpiDefs = [
    { key: "rps", label: "RPS", fmt: (v) => v.toFixed(1), cls: "accent" },
    { key: "ok", label: "OK", fmt: (v) => fmtInt(v), cls: "ok" },
    { key: "err", label: "Errors", fmt: (v) => fmtInt(v), cls: "bad" },
    { key: "p50Ms", label: "p50", fmt: (v) => `${v.toFixed(0)}ms` },
    { key: "p95Ms", label: "p95", fmt: (v) => `${v.toFixed(0)}ms` },
    { key: "inFlight", label: "In flight", fmt: (v) => fmtInt(v) },
  ];

  let mode = "live"; // live | demo
  let ws = null;
  let demoTimer = null;
  let demoState = null;
  let points = [];

  function fmtInt(n) {
    return Math.round(n || 0).toLocaleString();
  }

  function log(line) {
    const ts = new Date().toLocaleTimeString();
    els.log.textContent = `[${ts}] ${line}\n` + els.log.textContent.slice(0, 2400);
  }

  function buildKpis() {
    els.kpis.innerHTML = kpiDefs
      .map(
        (d) =>
          `<div class="kpi"><div class="label">${d.label}</div><div class="value ${d.cls || ""}" data-k="${d.key}">—</div></div>`
      )
      .join("");
  }

  function buildBars() {
    const rows = [
      ["2xx", "s2"],
      ["3xx", "s3"],
      ["4xx", "s4"],
      ["5xx", "s5"],
      ["other", "so"],
    ];
    els.statusBars.innerHTML = rows
      .map(
        ([name, cls]) =>
          `<div class="bar-row"><span>${name}</span><div class="bar-track"><div class="bar-fill ${cls}" data-bar="${name}"></div></div><span data-bar-n="${name}">0</span></div>`
      )
      .join("");
  }

  function applySnapshot(s) {
    for (const d of kpiDefs) {
      const node = els.kpis.querySelector(`[data-k="${d.key}"]`);
      if (node) node.textContent = d.fmt(Number(s[d.key] || 0));
    }
    els.p99Label.textContent = `p99 ${Number(s.p99Ms || 0).toFixed(0)}ms`;
    els.totalLabel.textContent = `${fmtInt(s.total)} req`;
    els.runId.textContent = s.runId ? s.runId.slice(0, 8) : "—";

    const total = Math.max(1, Number(s.total) || 1);
    const mix = {
      "2xx": s.status2xx || 0,
      "3xx": s.status3xx || 0,
      "4xx": s.status4xx || 0,
      "5xx": s.status5xx || 0,
      other: s.statusOther || 0,
    };
    for (const [name, val] of Object.entries(mix)) {
      const fill = els.statusBars.querySelector(`[data-bar="${name}"]`);
      const n = els.statusBars.querySelector(`[data-bar-n="${name}"]`);
      if (fill) fill.style.width = `${(val / total) * 100}%`;
      if (n) n.textContent = fmtInt(val);
    }

    const recent = s.recentLatenciesMs || [];
    if (recent.length) {
      points = recent.slice(-120);
    } else if (s.running) {
      points.push(Number(s.p50Ms) || 0);
      if (points.length > 120) points.shift();
    }
    drawChart();

    els.btnStart.disabled = !!s.running;
    els.btnStop.disabled = !s.running;
    els.linkState.className = "chip " + (s.running ? "busy" : mode === "demo" ? "demo" : "live");
    els.linkState.textContent = s.running ? "forging" : mode === "demo" ? "demo" : "ready";
  }

  function drawChart() {
    const canvas = els.chart;
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth || 600;
    const h = 160;
    canvas.width = w * dpr;
    canvas.height = h * dpr;
    const ctx = canvas.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    ctx.strokeStyle = "rgba(18,24,32,0.08)";
    ctx.lineWidth = 1;
    for (let i = 1; i < 4; i++) {
      const y = (h / 4) * i;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(w, y);
      ctx.stroke();
    }

    if (points.length < 2) return;
    const max = Math.max(...points, 1);
    ctx.beginPath();
    ctx.strokeStyle = "#e4572e";
    ctx.lineWidth = 2;
    points.forEach((v, i) => {
      const x = (i / (points.length - 1)) * (w - 8) + 4;
      const y = h - 8 - (v / max) * (h - 20);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.stroke();

    const grad = ctx.createLinearGradient(0, 0, 0, h);
    grad.addColorStop(0, "rgba(228,87,46,0.22)");
    grad.addColorStop(1, "rgba(228,87,46,0)");
    ctx.lineTo(w - 4, h - 4);
    ctx.lineTo(4, h - 4);
    ctx.closePath();
    ctx.fillStyle = grad;
    ctx.fill();
  }

  function configFromForm() {
    const rpsVal = Number(els.rps.value) || 0;
    return {
      target: els.target.value.trim(),
      method: els.method.value,
      concurrency: Number(els.concurrency.value) || 32,
      durationSecs: Number(els.duration.value) || 15,
      rps: rpsVal > 0 ? rpsVal : null,
      timeoutMs: Number(els.timeout.value) || 5000,
      headers: {},
      body: els.body.value.trim() || null,
      warmupSecs: Number(els.warmup.value) || 0,
    };
  }

  async function startLive() {
    const cfg = configFromForm();
    log(`ignite → ${cfg.method} ${cfg.target}`);
    const res = await fetch("/api/run", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(cfg),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      log(`failed: ${err.error || res.status}`);
      return;
    }
    log("forge accepted");
  }

  async function stopLive() {
    await fetch("/api/stop", { method: "POST" });
    log("cool-down signaled");
  }

  function startDemo() {
    if (demoTimer) return;
    const cfg = configFromForm();
    const runId = crypto.randomUUID();
    const started = performance.now();
    demoState = {
      runId,
      target: cfg.target,
      running: true,
      ok: 0,
      err: 0,
      total: 0,
      bytes: 0,
      inFlight: 0,
      status2xx: 0,
      status3xx: 0,
      status4xx: 0,
      status5xx: 0,
      statusOther: 0,
      recentLatenciesMs: [],
      durationMs: (cfg.durationSecs || 20) * 1000,
      concurrency: cfg.concurrency,
    };
    log(`demo forge → ${cfg.method} ${cfg.target}`);
    demoTimer = setInterval(() => {
      const elapsed = performance.now() - started;
      if (elapsed >= demoState.durationMs) {
        demoState.running = false;
        demoState.inFlight = 0;
        clearInterval(demoTimer);
        demoTimer = null;
        log("demo forge complete");
      } else {
        const burst = Math.max(1, Math.round(cfg.concurrency / 6));
        for (let i = 0; i < burst; i++) {
          const lat = 18 + Math.random() * 90 + (Math.random() < 0.08 ? Math.random() * 220 : 0);
          demoState.recentLatenciesMs.push(lat);
          if (demoState.recentLatenciesMs.length > 120) demoState.recentLatenciesMs.shift();
          demoState.total += 1;
          if (Math.random() < 0.985) {
            demoState.ok += 1;
            demoState.status2xx += 1;
            demoState.bytes += 400 + Math.floor(Math.random() * 800);
          } else if (Math.random() < 0.6) {
            demoState.err += 1;
            demoState.status5xx += 1;
          } else {
            demoState.err += 1;
            demoState.status4xx += 1;
          }
        }
        demoState.inFlight = Math.floor(Math.random() * cfg.concurrency * 0.4);
      }
      const sorted = [...demoState.recentLatenciesMs].sort((a, b) => a - b);
      const pct = (p) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))] || 0;
      const snap = {
        ...demoState,
        elapsedSecs: elapsed / 1000,
        rps: demoState.total / Math.max(0.001, elapsed / 1000),
        p50Ms: pct(0.5),
        p90Ms: pct(0.9),
        p95Ms: pct(0.95),
        p99Ms: pct(0.99),
        meanMs: sorted.reduce((a, b) => a + b, 0) / Math.max(1, sorted.length),
        ts: new Date().toISOString(),
      };
      applySnapshot(snap);
    }, 120);
  }

  function stopDemo() {
    if (demoTimer) {
      clearInterval(demoTimer);
      demoTimer = null;
    }
    if (demoState) {
      demoState.running = false;
      applySnapshot({
        ...demoState,
        elapsedSecs: 0,
        rps: 0,
        p50Ms: 0,
        p90Ms: 0,
        p95Ms: 0,
        p99Ms: 0,
        meanMs: 0,
        inFlight: 0,
        ts: new Date().toISOString(),
      });
    }
    log("demo cooled");
  }

  els.btnStart.addEventListener("click", () => {
    if (mode === "demo") startDemo();
    else startLive().catch((e) => log(String(e)));
  });
  els.btnStop.addEventListener("click", () => {
    if (mode === "demo") stopDemo();
    else stopLive().catch((e) => log(String(e)));
  });

  function connectWs() {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    const url = `${proto}://${location.host}/ws`;
    try {
      ws = new WebSocket(url);
    } catch {
      enterDemo("ws construct failed");
      return;
    }
    ws.onopen = () => {
      mode = "live";
      els.modeChip.textContent = "live";
      els.modeChip.className = "chip live";
      els.linkState.textContent = "ready";
      els.linkState.className = "chip live";
      log("websocket linked");
    };
    ws.onmessage = (ev) => {
      try {
        applySnapshot(JSON.parse(ev.data));
      } catch {
        /* ignore */
      }
    };
    ws.onerror = () => {};
    ws.onclose = () => {
      if (mode === "live") {
        // Pages / static host — fall back to demo
        enterDemo("backend offline — demo mode");
      }
    };
  }

  function enterDemo(reason) {
    mode = "demo";
    els.modeChip.textContent = "demo";
    els.modeChip.className = "chip demo";
    els.linkState.textContent = "demo";
    els.linkState.className = "chip demo";
    log(reason);
    // gentle idle waveform so the page never feels dead
    if (!demoTimer && !demoState?.running) {
      points = Array.from({ length: 40 }, (_, i) => 30 + Math.sin(i / 3) * 12 + Math.random() * 8);
      drawChart();
      applySnapshot({
        runId: "",
        target: "",
        running: false,
        elapsedSecs: 0,
        ok: 0,
        err: 0,
        total: 0,
        rps: 0,
        bytes: 0,
        inFlight: 0,
        p50Ms: 0,
        p90Ms: 0,
        p95Ms: 0,
        p99Ms: 0,
        meanMs: 0,
        status2xx: 0,
        status3xx: 0,
        status4xx: 0,
        status5xx: 0,
        statusOther: 0,
        recentLatenciesMs: points,
        ts: new Date().toISOString(),
      });
    }
  }

  async function boot() {
    buildKpis();
    buildBars();
    // GitHub Pages has no API — detect quickly
    const isPages = location.hostname.endsWith("github.io");
    if (isPages) {
      enterDemo("GitHub Pages demo — click Ignite for a simulated forge");
      return;
    }
    try {
      const res = await fetch("/api/health", { cache: "no-store" });
      if (!res.ok) throw new Error("no health");
      connectWs();
    } catch {
      enterDemo("no local server — demo mode");
    }
    window.addEventListener("resize", drawChart);
  }

  boot();
})();
