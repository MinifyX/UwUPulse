/*
 * UwUPulse overlay. Served at /overlay/{id}?token=…[&preview=1][&demo=1].
 * No build step, no external resources. Class names: see overlay.css.
 */
(function () {
  'use strict';

  // ── Config ──────────────────────────────────────────────

  var PRESETS = ['classic', 'minimal', 'ring', 'graph', 'ecg'];
  var SYSTEM_FONT = "system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif";
  var HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
  var LIVE_MS = 10000;
  var HISTORY_MS = 600000;

  // Same defaults and limits as apps/desktop/src/lib/widget.ts.
  function defaults() {
    return {
      preset: 'classic',
      color: '#ff4d6d',
      textColor: '#ffffff',
      background: 'transparent',
      font: SYSTEM_FONT,
      fontSize: 72,
      showIcon: true,
      showLabel: true,
      animateBeat: true,
      showGraph: false,
      graphSeconds: 60,
      zones: {
        enabled: false,
        items: [
          { min: 0, color: '#4dabf7' },
          { min: 100, color: '#51cf66' },
          { min: 130, color: '#fcc419' },
          { min: 160, color: '#ff6b6b' },
        ],
      },
      idleText: '',
      hideWhenIdle: false,
      customCss: '',
    };
  }

  function num(v, d, min, max) {
    return typeof v === 'number' && isFinite(v) ? Math.min(max, Math.max(min, Math.round(v))) : d;
  }
  function str(v, d, max) {
    return typeof v === 'string' ? v.slice(0, max) : d;
  }
  function bool(v, d) {
    return typeof v === 'boolean' ? v : d;
  }
  function color(v, d, allowTransparent) {
    if (typeof v !== 'string') return d;
    var c = v.trim();
    return HEX.test(c) || (allowTransparent && c === 'transparent') ? c : d;
  }

  function normalize(input) {
    var raw = input && typeof input === 'object' ? input : {};
    var d = defaults();
    var zr = raw.zones && typeof raw.zones === 'object' ? raw.zones : {};
    var items = Array.isArray(zr.items)
      ? zr.items
          .filter(function (z) {
            return z && typeof z === 'object';
          })
          .slice(0, 8)
          .map(function (z) {
            return { min: num(z.min, 0, 0, 250), color: color(z.color, d.color) };
          })
      : d.zones.items;
    items.sort(function (a, b) {
      return a.min - b.min;
    });
    var font = str(raw.font, d.font, 200).trim();
    return {
      preset: PRESETS.indexOf(raw.preset) >= 0 ? raw.preset : d.preset,
      color: color(raw.color, d.color),
      textColor: color(raw.textColor, d.textColor),
      background: color(raw.background, d.background, true),
      font: font || d.font,
      fontSize: num(raw.fontSize, d.fontSize, 12, 400),
      showIcon: bool(raw.showIcon, d.showIcon),
      showLabel: bool(raw.showLabel, d.showLabel),
      animateBeat: bool(raw.animateBeat, d.animateBeat),
      showGraph: bool(raw.showGraph, d.showGraph),
      graphSeconds: num(raw.graphSeconds, d.graphSeconds, 10, 600),
      zones: { enabled: bool(zr.enabled, d.zones.enabled), items: items },
      idleText: str(raw.idleText, d.idleText, 64),
      hideWhenIdle: bool(raw.hideWhenIdle, d.hideWhenIdle),
      customCss: str(raw.customCss, d.customCss, 32 * 1024),
    };
  }

  // ── Page parameters and elements ────────────────────────

  var parts = location.pathname.split('/').filter(Boolean);
  var widgetId = parts.length >= 2 ? decodeURIComponent(parts[parts.length - 1]) : '';
  var query = new URLSearchParams(location.search);
  var token = query.get('token') || '';
  var preview = query.get('preview') === '1';
  var demo = query.get('demo') === '1';

  var root = document.querySelector('.uwp-root');
  var widget = root.querySelector('.uwp-widget');
  var bpmEl = root.querySelector('.uwp-bpm');
  var idleEl = root.querySelector('.uwp-idle');
  var ringFill = root.querySelector('.uwp-ring-fill');
  var graph = root.querySelector('.uwp-graph');
  var ecg = root.querySelector('.uwp-ecg');
  var customStyle = document.getElementById('uwp-custom');

  var cfg = normalize(null);
  var previewConfigured = false; // in preview, the editor's config wins over the saved one

  // ── State ───────────────────────────────────────────────

  var target = null; // last bpm
  var shown = null; // displayed (smoothed) bpm
  var shownText = '';
  var live = false;
  var lastSampleAt = 0; // performance.now() of the last value
  var lastRealAt = -Infinity; // the last value from the server (for demo)
  var samples = []; // {t: performance.now(), bpm}
  var beats = []; // beat times in seconds (performance.now() / 1000)
  var phase = 0;
  var pulse = 0;
  var colorCache = { graph: '', ecg: '' };

  function setAttr(name, value) {
    var v = String(value);
    if (root.getAttribute(name) !== v) root.setAttribute(name, v);
  }

  function apply(next) {
    cfg = next;
    var s = root.style;
    s.setProperty('--uwp-text', cfg.textColor);
    s.setProperty('--uwp-bg', cfg.background);
    s.setProperty('--uwp-font', cfg.font);
    s.setProperty('--uwp-size', cfg.fontSize + 'px');
    setAttr('data-preset', cfg.preset);
    setAttr('data-icon', cfg.showIcon);
    setAttr('data-label', cfg.showLabel);
    setAttr('data-graph', cfg.showGraph);
    setAttr('data-hide-idle', cfg.hideWhenIdle);
    setAttr('data-idle-text', cfg.idleText.trim() !== '');
    idleEl.textContent = cfg.idleText;
    // Text only: the browser never parses this as HTML.
    if (customStyle.textContent !== cfg.customCss) customStyle.textContent = cfg.customCss;
    if (!cfg.animateBeat) {
      pulse = 0;
      s.setProperty('--uwp-pulse', '0');
    }
    updateValue();
    fit();
    resizeCanvases();
    kick();
  }

  // Zone colour, ring fill and beat length follow the value.
  function updateValue() {
    var s = root.style;
    var accent = cfg.color;
    if (cfg.zones.enabled && target != null && cfg.zones.items.length) {
      var idx = 0;
      for (var i = 0; i < cfg.zones.items.length; i++) {
        if (target >= cfg.zones.items[i].min) idx = i;
      }
      accent = cfg.zones.items[idx].color;
      setAttr('data-zone', idx);
    } else if (root.hasAttribute('data-zone')) {
      root.removeAttribute('data-zone');
    }
    if (s.getPropertyValue('--uwp-color') !== accent) {
      s.setProperty('--uwp-color', accent);
      colorCache.graph = colorCache.ecg = '';
      drawGraph();
    }
    if (target != null) s.setProperty('--uwp-beat', (60 / target).toFixed(3) + 's');
    var pct = target == null ? 0 : Math.min(100, Math.max(2, ((target - 40) / 160) * 100));
    ringFill.style.strokeDasharray = pct.toFixed(1) + ' 100';
  }

  function setLive(value) {
    live = value;
    setAttr('data-live', value);
    if (value) kick();
  }

  function addSample(bpm, real) {
    if (typeof bpm !== 'number' || !isFinite(bpm) || bpm <= 0) return;
    var now = performance.now();
    if (real) lastRealAt = now;
    target = Math.round(bpm);
    if (shown == null) shown = target;
    lastSampleAt = now;
    samples.push({ t: now, bpm: target });
    while (samples.length && samples[0].t < now - HISTORY_MS) samples.shift();
    updateValue();
    setLive(true);
    drawGraph();
  }

  // ── Network ─────────────────────────────────────────────

  function api(path) {
    return path + (path.indexOf('?') < 0 ? '?' : '&') + 'token=' + encodeURIComponent(token);
  }

  function loadWidget() {
    if (!widgetId) return;
    fetch(api('/api/v1/widgets/' + encodeURIComponent(widgetId)), { cache: 'no-store' })
      .then(function (r) {
        if (!r.ok) throw new Error('HTTP ' + r.status);
        return r.json();
      })
      .then(function (json) {
        if (!(preview && previewConfigured)) apply(normalize(json));
      })
      .catch(function () {
        // Keep what is shown; the next reconnect tries again.
      });
  }

  var socket = null;
  var attempt = 0;

  function connect() {
    var url =
      (location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + api('/api/v1/ws');
    var ws;
    try {
      ws = new WebSocket(url);
    } catch (e) {
      return retry();
    }
    socket = ws;
    ws.onopen = function () {
      attempt = 0;
      loadWidget();
    };
    ws.onmessage = function (event) {
      var msg;
      try {
        msg = JSON.parse(event.data);
      } catch (e) {
        return;
      }
      if (!msg || typeof msg !== 'object') return;
      if (msg.t === 'hello') {
        var cur = msg.current;
        if (cur && cur.live && typeof cur.bpm === 'number') addSample(cur.bpm, true);
      } else if (msg.t === 'hr') {
        addSample(msg.bpm, true);
      } else if (msg.t === 'live') {
        if (msg.live === false && !demoActive()) setLive(false);
      } else if (msg.t === 'widget') {
        if (msg.id === widgetId && !preview) apply(normalize(msg.config));
      }
    };
    ws.onclose = function () {
      if (socket === ws) {
        socket = null;
        retry();
      }
    };
    ws.onerror = function () {
      ws.close();
    };
  }

  function retry() {
    var base = Math.min(30000, 1000 * Math.pow(2, attempt));
    attempt++;
    setTimeout(connect, base * (0.75 + Math.random() * 0.5));
  }

  // ── Demo values ─────────────────────────────────────────

  function demoActive() {
    return demo && performance.now() - lastRealAt > LIVE_MS;
  }

  if (demo) {
    var t0 = performance.now();
    var demoTick = function () {
      if (!demoActive()) return;
      var t = (performance.now() - t0) / 1000;
      var bpm = 84 + 18 * Math.sin(t / 11) + 7 * Math.sin(t / 2.7) + (Math.random() - 0.5) * 3;
      addSample(bpm, false);
    };
    demoTick();
    setInterval(demoTick, 1000);
  }

  // Local live timeout, in case the socket is gone.
  setInterval(function () {
    if (live && performance.now() - lastSampleAt > LIVE_MS) setLive(false);
    if (cfg.showGraph || cfg.preset === 'graph') drawGraph();
  }, 1000);

  // ── Preview ─────────────────────────────────────────────

  function fit() {
    if (!preview) return;
    root.style.setProperty('--uwp-fit', '1');
    var w = widget.offsetWidth;
    var h = widget.offsetHeight;
    if (!w || !h) return;
    var f = Math.min(1, (window.innerWidth - 16) / w, (window.innerHeight - 16) / h);
    root.style.setProperty('--uwp-fit', String(Math.max(0.05, f)));
  }

  if (preview) {
    window.addEventListener('message', function (event) {
      if (event.source !== window.parent) return;
      var data = event.data;
      if (!data || data.type !== 'uwupulse-config') return;
      previewConfigured = true;
      apply(normalize(data.config));
    });
    window.addEventListener('resize', fit);
    if (window.parent !== window) window.parent.postMessage({ type: 'uwupulse-ready' }, '*');
  }

  // ── Canvases ────────────────────────────────────────────

  function sizeCanvas(c) {
    var dpr = window.devicePixelRatio || 1;
    var w = Math.round(c.clientWidth * dpr);
    var h = Math.round(c.clientHeight * dpr);
    if (c.width !== w || c.height !== h) {
      c.width = w;
      c.height = h;
    }
    return dpr;
  }

  function resizeCanvases() {
    sizeCanvas(graph);
    sizeCanvas(ecg);
    drawGraph();
  }

  if (window.ResizeObserver) {
    var ro = new ResizeObserver(function () {
      resizeCanvases();
      fit();
    });
    ro.observe(graph);
    ro.observe(ecg);
    ro.observe(widget);
  }

  function strokeColor(c, key) {
    if (!colorCache[key]) colorCache[key] = getComputedStyle(c).color || cfg.color;
    return colorCache[key];
  }

  function withAlpha(rgb, alpha) {
    var m = /^rgba?\(([^)]+)\)/.exec(rgb);
    if (!m) return rgb;
    var p = m[1].split(',');
    return 'rgba(' + p[0] + ',' + p[1] + ',' + p[2] + ',' + alpha + ')';
  }

  // Sparkline of the last graphSeconds.
  function drawGraph() {
    if (!graph.clientWidth) return;
    var dpr = sizeCanvas(graph);
    var ctx = graph.getContext('2d');
    var w = graph.width;
    var h = graph.height;
    ctx.clearRect(0, 0, w, h);
    var now = performance.now();
    var span = cfg.graphSeconds * 1000;
    var from = now - span;
    var pts = [];
    var lo = Infinity;
    var hi = -Infinity;
    for (var i = 0; i < samples.length; i++) {
      var s = samples[i];
      if (s.t < from - 2000) continue;
      pts.push(s);
      if (s.bpm < lo) lo = s.bpm;
      if (s.bpm > hi) hi = s.bpm;
    }
    if (pts.length < 2) return;
    var mid = (lo + hi) / 2;
    var range = Math.max(20, hi - lo + 8);
    lo = mid - range / 2;
    var pad = 3 * dpr;
    var x = function (t) {
      return ((t - from) / span) * w;
    };
    var y = function (b) {
      return pad + (1 - (b - lo) / range) * (h - 2 * pad);
    };
    var col = strokeColor(graph, 'graph');
    ctx.beginPath();
    ctx.moveTo(x(pts[0].t), y(pts[0].bpm));
    for (var j = 1; j < pts.length; j++) ctx.lineTo(x(pts[j].t), y(pts[j].bpm));
    ctx.lineJoin = 'round';
    ctx.lineCap = 'round';
    ctx.lineWidth = 2.5 * dpr;
    ctx.strokeStyle = col;
    ctx.stroke();
    ctx.lineTo(x(pts[pts.length - 1].t), h);
    ctx.lineTo(x(pts[0].t), h);
    ctx.closePath();
    var fill = ctx.createLinearGradient(0, 0, 0, h);
    fill.addColorStop(0, withAlpha(col, 0.35));
    fill.addColorStop(1, withAlpha(col, 0));
    ctx.fillStyle = fill;
    ctx.fill();
  }

  // ECG: a PQRST complex per beat, as a sum of Gaussians (offset s, amplitude).
  var WAVES = [
    [-0.16, 0.12, 0.025], // P
    [-0.035, -0.12, 0.01], // Q
    [0, 1, 0.012], // R
    [0.035, -0.28, 0.012], // S
    [0.24, 0.28, 0.045], // T
  ];
  var ECG_WINDOW = 3; // seconds visible
  var ECG_DELAY = 0.5; // the trace lags so the P wave of a beat can be drawn

  function ecgAt(t) {
    var v = 0;
    for (var i = beats.length - 1; i >= 0; i--) {
      var d = t - beats[i];
      if (d > 0.6) break;
      if (d < -0.3) continue;
      for (var k = 0; k < WAVES.length; k++) {
        var z = (d - WAVES[k][0]) / WAVES[k][2];
        v += WAVES[k][1] * Math.exp(-0.5 * z * z);
      }
    }
    return v;
  }

  function drawEcg(nowSec) {
    if (!ecg.clientWidth) return;
    var dpr = sizeCanvas(ecg);
    var ctx = ecg.getContext('2d');
    var w = ecg.width;
    var h = ecg.height;
    ctx.clearRect(0, 0, w, h);
    var end = nowSec - ECG_DELAY;
    var start = end - ECG_WINDOW;
    var base = h * 0.66;
    var amp = h * 0.56;
    var step = 2 * dpr;
    var col = strokeColor(ecg, 'ecg');
    ctx.beginPath();
    for (var px = 0; px <= w; px += step) {
      var yy = base - ecgAt(start + (px / w) * ECG_WINDOW) * amp;
      if (px === 0) ctx.moveTo(px, yy);
      else ctx.lineTo(px, yy);
    }
    var grad = ctx.createLinearGradient(0, 0, w, 0);
    grad.addColorStop(0, withAlpha(col, 0));
    grad.addColorStop(0.3, col);
    grad.addColorStop(1, col);
    ctx.strokeStyle = grad;
    ctx.lineWidth = 2.5 * dpr;
    ctx.lineJoin = 'round';
    ctx.lineCap = 'round';
    ctx.stroke();
    // Bright head at the newest point.
    ctx.fillStyle = col;
    ctx.beginPath();
    ctx.arc(w - dpr * 2, base - ecgAt(end) * amp, 3 * dpr, 0, Math.PI * 2);
    ctx.fill();
  }

  // ── Animation loop ──────────────────────────────────────

  // Lub-dub: a strong and a weaker bump at the start of each beat.
  function beatCurve(p) {
    var a = (p - 0.06) / 0.05;
    var b = (p - 0.26) / 0.06;
    return Math.min(1, Math.exp(-a * a) + 0.55 * Math.exp(-b * b));
  }

  var raf = 0;
  var lastFrame = 0;

  function kick() {
    if (!raf) {
      lastFrame = performance.now();
      raf = requestAnimationFrame(frame);
    }
  }

  function frame(now) {
    raf = 0;
    var dt = Math.min(0.1, Math.max(0, (now - lastFrame) / 1000));
    lastFrame = now;
    var nowSec = now / 1000;
    var busy = false;

    // Smooth number.
    if (target != null) {
      if (shown == null) shown = target;
      shown += (target - shown) * (1 - Math.exp(-dt * 5));
      if (Math.abs(target - shown) < 0.02) shown = target;
      else busy = true;
    }
    var text = shown == null ? '--' : String(Math.round(shown));
    if (text !== shownText) {
      bpmEl.textContent = text;
      shownText = text;
    }

    // Beats.
    if (live && target != null) {
      phase += dt * (target / 60);
      if (phase >= 1) {
        phase -= Math.floor(phase);
        beats.push(nowSec);
        if (beats.length > 32) beats.shift();
      }
      busy = true;
    }
    var nextPulse = live && cfg.animateBeat ? beatCurve(phase) : pulse * Math.exp(-dt * 10);
    if (nextPulse < 0.002) nextPulse = 0;
    if (nextPulse !== pulse) {
      pulse = nextPulse;
      root.style.setProperty('--uwp-pulse', pulse.toFixed(3));
      if (pulse) busy = true;
    }

    // ECG keeps scrolling until the last beat has left the trace.
    if (cfg.preset === 'ecg') {
      drawEcg(nowSec);
      if (beats.length && nowSec - beats[beats.length - 1] < ECG_WINDOW + ECG_DELAY + 1)
        busy = true;
    }

    if (busy) raf = requestAnimationFrame(frame);
  }

  // ── Start ───────────────────────────────────────────────

  document.addEventListener('visibilitychange', function () {
    if (!document.hidden) kick();
  });

  apply(cfg);
  loadWidget();
  connect();
})();
