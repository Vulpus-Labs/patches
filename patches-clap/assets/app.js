(function () {
  "use strict";

  var api = (window.__patches = window.__patches || {});
  api.lastSnapshot = null;
  api.lastFrame = null;

  api.applyState = function (snapshot) {
    api.lastSnapshot = snapshot;
    if (api._syncTapLayout) api._syncTapLayout(snapshot && snapshot.taps);
    if (api._renderHalt) api._renderHalt(snapshot && snapshot.halt_message);
    if (api._renderDiagnostics) api._renderDiagnostics(snapshot && snapshot.diagnostics);
    if (api._renderStatusLog) api._renderStatusLog(snapshot && snapshot.status_log);
    if (api._renderFilePath) api._renderFilePath(snapshot && snapshot.file_path);
    if (api._renderModulePaths) api._renderModulePaths(snapshot && snapshot.module_paths);
  };

  api.applyTaps = function (frame) {
    api.lastFrame = frame;
    if (api._renderTaps) api._renderTaps(frame);
  };

  // dB thresholds — must match patches-player/src/tui.rs.
  var DB_AMBER_FLOOR = -18;
  var DB_RED_FLOOR = -6;
  var DB_FLOOR = -60;

  function ampToDb(amp) {
    if (amp <= 0) return DB_FLOOR;
    var db = 20 * Math.log10(amp);
    return db < DB_FLOOR ? DB_FLOOR : db;
  }

  function dbToRatio(db) {
    if (db < DB_FLOOR) db = DB_FLOOR;
    if (db > 0) db = 0;
    return (db - DB_FLOOR) / -DB_FLOOR;
  }

  function dbColour(db) {
    if (db >= DB_RED_FLOOR) return "#e04040";
    if (db >= DB_AMBER_FLOOR) return "#e0a040";
    return "#40c060";
  }

  // Canvas2D meter widget. `orientation` is "vertical" or "horizontal".
  // `update({ peak, rms })` redraws from a linear-amplitude pair.
  function MeterWidget(canvas, orientation) {
    this.canvas = canvas;
    this.orientation = orientation === "horizontal" ? "horizontal" : "vertical";
    this.peakDb = DB_FLOOR;
    this.rmsDb = DB_FLOOR;
  }

  MeterWidget.prototype.update = function (sample) {
    this.peakDb = ampToDb(sample && typeof sample.peak === "number" ? sample.peak : 0);
    this.rmsDb = ampToDb(sample && typeof sample.rms === "number" ? sample.rms : 0);
    this.draw();
  };

  MeterWidget.prototype.draw = function () {
    var c = this.canvas;
    if (!c) return;
    var ctx = c.getContext("2d");
    if (!ctx) return;
    var w = c.width;
    var h = c.height;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = "#1a1a1a";
    ctx.fillRect(0, 0, w, h);

    var rmsRatio = dbToRatio(this.rmsDb);
    var peakRatio = dbToRatio(this.peakDb);
    var rmsCol = dbColour(this.rmsDb);
    var peakCol = dbColour(this.peakDb);

    if (this.orientation === "horizontal") {
      var rmsW = Math.round(w * rmsRatio);
      ctx.fillStyle = rmsCol;
      ctx.fillRect(0, 0, rmsW, h);
      // Peak tick.
      if (this.peakDb > DB_FLOOR) {
        var px = Math.min(Math.round(w * peakRatio), w - 1);
        ctx.fillStyle = peakCol;
        ctx.fillRect(px, 0, 2, h);
      }
    } else {
      var rmsH = Math.round(h * rmsRatio);
      ctx.fillStyle = rmsCol;
      ctx.fillRect(0, h - rmsH, w, rmsH);
      if (this.peakDb > DB_FLOOR) {
        var py = Math.max(h - Math.round(h * peakRatio) - 1, 0);
        ctx.fillStyle = peakCol;
        ctx.fillRect(0, py, w, 2);
      }
    }
  };

  // Oscilloscope widget — line plot over the full buffer width, ±1.0
  // amplitude rails. `update(samples)` accepts any Float32Array-like;
  // the widget owns no buffer state.
  // `snap` (boolean) — if true, rotate the buffer so the latest rising
  // zero-cross sits at index 0. Toggleable client-side at no cost since
  // the server sends raw decimated samples.
  function ScopeWidget(canvas, opts) {
    this.canvas = canvas;
    this.samples = null;
    this.snap = !!(opts && opts.snap);
  }

  ScopeWidget.prototype.setSnap = function (snap) {
    this.snap = !!snap;
    this.draw();
  };

  ScopeWidget.prototype.update = function (samples) {
    this.samples = samples || null;
    this.draw();
  };

  // Find the latest rising zero-cross (prev<0, curr>=0). Returns
  // null if none.
  function findLatestZeroCross(s) {
    var n = s.length;
    var latest = null;
    for (var i = 1; i < n; i++) {
      if (s[i - 1] < 0 && s[i] >= 0) latest = i;
    }
    return latest;
  }

  ScopeWidget.prototype.draw = function () {
    var c = this.canvas;
    if (!c) return;
    var ctx = c.getContext("2d");
    if (!ctx) return;
    var w = c.width;
    var h = c.height;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = "#0f0f0f";
    ctx.fillRect(0, 0, w, h);

    // Rails: -1, 0, +1.
    ctx.strokeStyle = "#404040";
    ctx.lineWidth = 1;
    var rails = [-1, 0, 1];
    for (var r = 0; r < rails.length; r++) {
      var y = ((1 - rails[r]) / 2) * (h - 1);
      ctx.beginPath();
      ctx.moveTo(0, y + 0.5);
      ctx.lineTo(w, y + 0.5);
      ctx.stroke();
    }

    var s = this.samples;
    if (!s || s.length < 2) return;

    // Optional zero-cross alignment, client-side. Operates on a copy
    // so the cached `samples` array isn't mutated for next frame.
    var n = s.length;
    var view = s;
    if (this.snap) {
      var k = findLatestZeroCross(s);
      if (k !== null) {
        view = new Array(n);
        for (var i = 0; i < n; i++) view[i] = s[(i + k) % n];
      }
    }

    ctx.strokeStyle = "#40d0e0";
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (var j = 0; j < n; j++) {
      var v = view[j];
      if (v > 1) v = 1; else if (v < -1) v = -1;
      var x = (j / (n - 1)) * (w - 1);
      var yy = ((1 - v) / 2) * (h - 1);
      if (j === 0) ctx.moveTo(x, yy); else ctx.lineTo(x, yy);
    }
    ctx.stroke();
  };

  // Spectrum widget — log-X frequency, dB-Y magnitude with floor at
  // DB_FLOOR (-60). Bin centre frequency is `k * sampleRate / fftSize`.
  // Two display modes:
  //   "curve":   filled area under a smooth line per latest frame.
  //   "heatmap": rolling waterfall, latest column on the right.
  // Defaults match patches-observation::processor::SPECTRUM_FFT_SIZE
  // (1024) and a 48 kHz host rate.
  var SPECTRUM_DB_MAX = 6;

  // Magma-ish ramp: dark purple → orange → pale yellow.
  function heatColour(t) {
    if (t < 0) t = 0; else if (t > 1) t = 1;
    // Piecewise linear approximation of magma.
    var stops = [
      [0.00,   0,   0,   8],
      [0.20,  40,  10,  90],
      [0.40, 130,  35, 120],
      [0.60, 215,  70,  80],
      [0.80, 250, 150,  60],
      [1.00, 252, 250, 200],
    ];
    for (var i = 1; i < stops.length; i++) {
      if (t <= stops[i][0]) {
        var a = stops[i - 1], b = stops[i];
        var u = (t - a[0]) / (b[0] - a[0]);
        return [
          Math.round(a[1] + (b[1] - a[1]) * u),
          Math.round(a[2] + (b[2] - a[2]) * u),
          Math.round(a[3] + (b[3] - a[3]) * u),
        ];
      }
    }
    return [stops[stops.length - 1][1], stops[stops.length - 1][2], stops[stops.length - 1][3]];
  }

  function SpectrumWidget(canvas, opts) {
    this.canvas = canvas;
    opts = opts || {};
    this.sampleRate = opts.sampleRate || 48000;
    this.fftSize = opts.fftSize || 1024;
    this.mags = null;
    this.mode = opts.mode === "heatmap" ? "heatmap" : "curve";
    // Heatmap backing buffer: ImageData scrolled left by 1 column per
    // frame. Allocated lazily on first heatmap draw.
    this.heatImage = null;
  }

  SpectrumWidget.prototype.setMode = function (mode) {
    var next = mode === "heatmap" ? "heatmap" : "curve";
    if (next === this.mode) return;
    this.mode = next;
    // Clear heatmap history when leaving / re-entering, so a stale
    // waterfall doesn't bleed across mode toggles.
    this.heatImage = null;
    this.draw();
  };

  SpectrumWidget.prototype.update = function (mags) {
    this.mags = mags || null;
    this.draw();
  };

  // Shared frequency / dB scaffolding.
  SpectrumWidget.prototype._scales = function (w, h) {
    var binHz = this.sampleRate / this.fftSize;
    var fMin = binHz;
    var fMax = this.sampleRate / 2;
    if (fMax <= fMin) fMax = fMin * 10;
    var logMin = Math.log10(fMin);
    var logMax = Math.log10(fMax);
    return {
      binHz: binHz,
      fMin: fMin,
      fMax: fMax,
      xFor: function (freq) {
        return ((Math.log10(freq) - logMin) / (logMax - logMin)) * (w - 1);
      },
      yFor: function (db) {
        if (db < DB_FLOOR) db = DB_FLOOR;
        if (db > SPECTRUM_DB_MAX) db = SPECTRUM_DB_MAX;
        return ((SPECTRUM_DB_MAX - db) / (SPECTRUM_DB_MAX - DB_FLOOR)) * (h - 1);
      },
      logMin: logMin,
      logMax: logMax,
    };
  };

  SpectrumWidget.prototype._drawGrid = function (ctx, w, h, s) {
    ctx.strokeStyle = "#303030";
    ctx.lineWidth = 1;
    var decades = [100, 1000, 10000];
    for (var i = 0; i < decades.length; i++) {
      if (decades[i] > s.fMax) break;
      var gx = s.xFor(decades[i]);
      ctx.beginPath();
      ctx.moveTo(gx + 0.5, 0);
      ctx.lineTo(gx + 0.5, h);
      ctx.stroke();
    }
    var dbGrid = [-40, -20, 0];
    for (var j = 0; j < dbGrid.length; j++) {
      var gy = s.yFor(dbGrid[j]);
      ctx.beginPath();
      ctx.moveTo(0, gy + 0.5);
      ctx.lineTo(w, gy + 0.5);
      ctx.stroke();
    }
  };

  SpectrumWidget.prototype._drawCurve = function (ctx, w, h) {
    ctx.fillStyle = "#0f0f0f";
    ctx.fillRect(0, 0, w, h);
    var s = this._scales(w, h);
    this._drawGrid(ctx, w, h, s);

    var mags = this.mags;
    if (!mags || mags.length < 2) return;

    var n = mags.length;
    var nyquist = (n - 1) * s.binHz;
    var logMin = s.logMin, logMax = s.logMax;

    // For each display column, compute frequency, interpolate dB
    // between the two surrounding bins (linear in magnitude). This
    // removes the staircase that pure nearest-bin sampling produces
    // at low frequencies, and the duplicate-bin clumping at the top.
    // For columns that span multiple bins (high-freq region) we take
    // the max so peaks survive resampling.
    var colDb = new Float32Array(w);
    for (var x = 0; x < w; x++) {
      var t = x / (w - 1);
      var logF = logMin + t * (logMax - logMin);
      var freq = Math.pow(10, logF);
      if (freq > nyquist) { colDb[x] = DB_FLOOR; continue; }
      var binF = freq / s.binHz;
      // Width of this column in bins (look at neighbour columns).
      var nextLogF = logMin + ((x + 1) / (w - 1)) * (logMax - logMin);
      var nextBinF = Math.pow(10, nextLogF) / s.binHz;
      var span = nextBinF - binF;
      var m;
      if (span <= 1) {
        // Sub-bin column: linearly interpolate between bin samples.
        var lo = Math.max(1, Math.floor(binF));
        var hi = Math.min(n - 1, lo + 1);
        var frac = binF - lo;
        m = (1 - frac) * mags[lo] + frac * mags[hi];
      } else {
        // Multi-bin column: take the peak magnitude over its range
        // so spectral lines don't disappear in the resampling.
        var k0 = Math.max(1, Math.floor(binF));
        var k1 = Math.min(n - 1, Math.ceil(binF + span));
        m = 0;
        for (var k = k0; k <= k1; k++) {
          if (mags[k] > m) m = mags[k];
        }
      }
      colDb[x] = m <= 0 ? DB_FLOOR : 20 * Math.log10(m);
    }

    // Light box-blur over dB columns (3-tap, in-place safe via
    // temporary). Small kernel — preserves peak position to within
    // one pixel but takes the visual edge off bin-quantisation kinks.
    var smooth = new Float32Array(w);
    for (var i = 0; i < w; i++) {
      var a = colDb[Math.max(0, i - 1)];
      var b = colDb[i];
      var c = colDb[Math.min(w - 1, i + 1)];
      smooth[i] = (a + 2 * b + c) * 0.25;
    }

    var floorY = s.yFor(DB_FLOOR);

    // Build the smooth path with quadratic Béziers through the midpoints
    // of consecutive (column, dB) pairs. Standard "smooth-line"
    // technique: each segment uses the next sample as the control
    // point and the midpoint with the sample after as the end point,
    // which yields a C1-continuous curve without overshoot.
    function pathThrough(ctx, smooth, s, w) {
      ctx.moveTo(0, s.yFor(smooth[0]));
      for (var i = 1; i < w - 1; i++) {
        var x0 = i;
        var x1 = i + 1;
        var mx = (x0 + x1) * 0.5;
        var my = (s.yFor(smooth[i]) + s.yFor(smooth[i + 1])) * 0.5;
        ctx.quadraticCurveTo(x0, s.yFor(smooth[i]), mx, my);
      }
      ctx.lineTo(w - 1, s.yFor(smooth[w - 1]));
    }

    // Filled area.
    ctx.beginPath();
    ctx.moveTo(0, floorY);
    ctx.lineTo(0, s.yFor(smooth[0]));
    for (var i2 = 1; i2 < w - 1; i2++) {
      var mx2 = (i2 + i2 + 1) * 0.5;
      var my2 = (s.yFor(smooth[i2]) + s.yFor(smooth[i2 + 1])) * 0.5;
      ctx.quadraticCurveTo(i2, s.yFor(smooth[i2]), mx2, my2);
    }
    ctx.lineTo(w - 1, s.yFor(smooth[w - 1]));
    ctx.lineTo(w - 1, floorY);
    ctx.closePath();
    ctx.fillStyle = "rgba(64, 192, 224, 0.25)";
    ctx.fill();

    // Stroke on top.
    ctx.beginPath();
    pathThrough(ctx, smooth, s, w);
    ctx.strokeStyle = "#40c0e0";
    ctx.lineWidth = 1.5;
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    ctx.stroke();
  };

  SpectrumWidget.prototype._drawHeatmap = function (ctx, w, h) {
    var s = this._scales(w, h);
    var mags = this.mags;

    // Build / scroll the backing image. ImageData is RGBA8, row-major.
    if (!this.heatImage || this.heatImage.width !== w || this.heatImage.height !== h) {
      this.heatImage = ctx.createImageData(w, h);
      // Fill with floor colour so empty area looks consistent.
      var floor = heatColour(0);
      var data0 = this.heatImage.data;
      for (var p = 0; p < data0.length; p += 4) {
        data0[p]     = floor[0];
        data0[p + 1] = floor[1];
        data0[p + 2] = floor[2];
        data0[p + 3] = 255;
      }
    }

    if (mags && mags.length >= 2) {
      var data = this.heatImage.data;
      // Scroll left by one column. Row stride = 4 * w bytes.
      var rowBytes = 4 * w;
      for (var y = 0; y < h; y++) {
        var base = y * rowBytes;
        // memmove (overlapping copy is safe since src > dst).
        data.copyWithin(base, base + 4, base + rowBytes);
      }

      // Resolve magnitude → dB per pixel-row by mapping each row's
      // y back to a frequency, finding the nearest bin.
      var n = mags.length;
      var lastCol = w - 1;
      for (var y2 = 0; y2 < h; y2++) {
        // y back to dB: invert yFor. y = ((MAX-db)/(MAX-FLOOR))*(h-1)
        // Frequency: invert xFor. We map y to *frequency* via a vertical
        // log scale instead — convention for waterfalls is freq on Y axis.
        var t = y2 / (h - 1);
        var logF = s.logMax - t * (s.logMax - s.logMin);
        var freq = Math.pow(10, logF);
        var bin = Math.round(freq / s.binHz);
        if (bin < 1) bin = 1;
        if (bin >= n) bin = n - 1;
        var m2 = mags[bin];
        var db2 = m2 <= 0 ? DB_FLOOR : 20 * Math.log10(m2);
        if (db2 < DB_FLOOR) db2 = DB_FLOOR;
        if (db2 > SPECTRUM_DB_MAX) db2 = SPECTRUM_DB_MAX;
        var u2 = (db2 - DB_FLOOR) / (SPECTRUM_DB_MAX - DB_FLOOR);
        var rgb = heatColour(u2);
        var off = (y2 * w + lastCol) * 4;
        data[off]     = rgb[0];
        data[off + 1] = rgb[1];
        data[off + 2] = rgb[2];
        data[off + 3] = 255;
      }
    }

    ctx.putImageData(this.heatImage, 0, 0);
  };

  SpectrumWidget.prototype.draw = function () {
    var c = this.canvas;
    if (!c) return;
    var ctx = c.getContext("2d");
    if (!ctx) return;
    var w = c.width;
    var h = c.height;
    ctx.clearRect(0, 0, w, h);

    if (this.mode === "heatmap") {
      this._drawHeatmap(ctx, w, h);
    } else {
      this._drawCurve(ctx, w, h);
    }
  };

  api.SpectrumWidget = SpectrumWidget;
  api.ScopeWidget = ScopeWidget;
  api.MeterWidget = MeterWidget;

  // ── Tap layout ──────────────────────────────────────────────────
  // Per-slot bundle of widget instances kept alive across frames so
  // canvases don't flicker. Rebuilt only when the manifest changes.
  var slotWidgets = Object.create(null); // slot → { meter, scope, spectrum }
  var ledNodes = Object.create(null);    // (slot+":"+kind) → element
  var triggerFireTime = Object.create(null); // slot → last-fire ms (perf clock)
  var lastTapsKey = null;

  // Trigger flash decay (UI-side). Audio side latches a fired flag and
  // the consumer (Rust gui.rs) takes-and-clears once per tap push; JS
  // owns the visual decay so it's smooth at frame rate rather than
  // quantised to the ~30 Hz tap-push cadence.
  var TRIGGER_DECAY_MS = 140;

  // LED on-colour per kind, used as the lit-state RGB. Brightness is
  // modulated continuously by the scalar (0..1) so the dot fades with
  // the trigger / gate decay tail rather than snapping on/off.
  var LED_COLOURS = {
    gate_led: [64, 192, 96],     // #40c060
    trigger_led: [224, 160, 64], // #e0a040
  };
  // Perceptual gamma: low scalar values should read clearly *off* to
  // the eye even though the audio-side decay is exponential. Rapid
  // retriggers (e.g. 16th-note hats) leave a visible afterglow rather
  // than a constant-on impression.
  var LED_GAMMA = 2.4;

  function applyLed(node, kind, value) {
    if (!node) return;
    var rgb = LED_COLOURS[kind] || [200, 200, 200];
    var v = value;
    if (!(v > 0)) v = 0;
    if (v > 1) v = 1;
    var lit = Math.pow(v, LED_GAMMA);
    var r = Math.round(rgb[0] * lit);
    var g = Math.round(rgb[1] * lit);
    var b = Math.round(rgb[2] * lit);
    node.style.backgroundColor = "rgb(" + r + "," + g + "," + b + ")";
    node.style.borderColor = v > 0.4
      ? "rgb(" + rgb[0] + "," + rgb[1] + "," + rgb[2] + ")"
      : "";
  }

  function tapsSignature(taps) {
    if (!taps) return "";
    var parts = [];
    for (var i = 0; i < taps.length; i++) {
      var t = taps[i];
      parts.push(t.slot + ":" + t.name + ":" + (t.components || []).join(","));
    }
    return parts.join("|");
  }

  function rebuildTaps(taps) {
    var root = document.getElementById("taps");
    if (!root) return;
    root.innerHTML = "";
    slotWidgets = Object.create(null);
    ledNodes = Object.create(null);
    if (!taps || taps.length === 0) {
      var empty = document.createElement("div");
      empty.className = "empty";
      empty.textContent = "no taps declared in this patch";
      root.appendChild(empty);
      return;
    }
    var sorted = taps.slice().sort(function (a, b) { return a.slot - b.slot; });

    // LED strip: gate_led / trigger_led from any tap, collected at the
    // top of the panel. Backend support is NYI so dots render inert.
    var leds = [];
    for (var li = 0; li < sorted.length; li++) {
      var lt = sorted[li];
      var lcomps = lt.components || [];
      for (var lj = 0; lj < lcomps.length; lj++) {
        if (lcomps[lj] === "gate_led" || lcomps[lj] === "trigger_led") {
          leds.push({ slot: lt.slot, name: lt.name, kind: lcomps[lj] });
        }
      }
    }
    if (leds.length > 0) {
      var strip = document.createElement("div");
      strip.className = "led-strip";
      for (var lk = 0; lk < leds.length; lk++) {
        var led = leds[lk];
        var cell = document.createElement("div");
        cell.className = "led-cell";
        var dot = document.createElement("div");
        dot.className = "led led-" + led.kind;
        dot.title = led.name + " (" + led.kind + ")";
        var label = document.createElement("span");
        label.className = "led-label";
        label.textContent = led.name;
        cell.appendChild(dot);
        cell.appendChild(label);
        strip.appendChild(cell);
        ledNodes[led.slot + ":" + led.kind] = dot;
      }
      root.appendChild(strip);
    }

    // Each non-LED component gets its own row, except osc + spectrum
    // which share a row (both are 320×96 waveform-style views and the
    // shared row keeps them visually aligned for cross-reading).
    var ROW_GROUPS = [
      ["meter"],
      ["osc", "spectrum"],
    ];

    function buildWidgetBox(kind, t, bundle) {
      var box = document.createElement("div");
      box.className = "tap-widget kind-" + kind;
      var canvas = document.createElement("canvas");
      if (kind === "meter") {
        canvas.width = 240; canvas.height = 18;
        bundle.meter = new MeterWidget(canvas, "horizontal");
      } else if (kind === "osc") {
        canvas.width = 320; canvas.height = 96;
        bundle.scope = new ScopeWidget(canvas);
        var snapToggle = document.createElement("button");
        snapToggle.className = "btn btn-snap";
        snapToggle.type = "button";
        snapToggle.textContent = "snap";
        snapToggle.dataset.scopeSlot = String(t.slot);
        box.appendChild(snapToggle);
        var decSel = document.createElement("select");
        decSel.className = "tap-opt";
        decSel.dataset.scopeSlot = String(t.slot);
        decSel.dataset.scopeOpt = "decimation";
        [["1", "÷1"], ["4", "÷4"], ["8", "÷8"], ["16", "÷16"], ["32", "÷32"], ["64", "÷64"]]
          .forEach(function (pair) {
            var o = document.createElement("option");
            o.value = pair[0];
            o.textContent = pair[1];
            if (pair[0] === "16") o.selected = true;
            decSel.appendChild(o);
          });
        box.appendChild(decSel);
      } else if (kind === "spectrum") {
        canvas.width = 320; canvas.height = 96;
        bundle.spectrum = new SpectrumWidget(canvas);
        var toggle = document.createElement("button");
        toggle.className = "btn btn-mode";
        toggle.type = "button";
        toggle.textContent = "heatmap";
        toggle.dataset.spectrumSlot = String(t.slot);
        box.appendChild(toggle);
        var fftSel = document.createElement("select");
        fftSel.className = "tap-opt";
        fftSel.dataset.spectrumSlot = String(t.slot);
        fftSel.dataset.spectrumOpt = "fft_size";
        [["1024", "FFT 1024"], ["2048", "FFT 2048"], ["4096", "FFT 4096"]]
          .forEach(function (pair) {
            var o = document.createElement("option");
            o.value = pair[0];
            o.textContent = pair[1];
            if (pair[0] === "1024") o.selected = true;
            fftSel.appendChild(o);
          });
        box.appendChild(fftSel);
      } else {
        return null;
      }
      box.appendChild(canvas);
      return box;
    }

    for (var i = 0; i < sorted.length; i++) {
      var t = sorted[i];
      var compsRaw = t.components || [];
      var bundle = {};
      var rowsRendered = 0;
      for (var gi = 0; gi < ROW_GROUPS.length; gi++) {
        var group = ROW_GROUPS[gi];
        var present = group.filter(function (k) {
          return compsRaw.indexOf(k) !== -1;
        });
        if (present.length === 0) continue;
        var row = document.createElement("div");
        row.className = "tap-row";
        var label = document.createElement("div");
        label.className = "tap-name";
        // Tap name on the first row of the tap; subsequent rows get a
        // visually quieter continuation label so the grouping is clear
        // without repeating the full name.
        if (rowsRendered === 0) {
          label.textContent = t.name + " (" + compsRaw.join("+") + ")";
        } else {
          label.textContent = "↳ " + present.join("+");
          label.classList.add("tap-name-cont");
        }
        row.appendChild(label);
        var widgets = document.createElement("div");
        widgets.className = "tap-widgets";
        row.appendChild(widgets);
        for (var j = 0; j < present.length; j++) {
          var box = buildWidgetBox(present[j], t, bundle);
          if (box) widgets.appendChild(box);
        }
        root.appendChild(row);
        rowsRendered++;
      }
      if (rowsRendered > 0) slotWidgets[t.slot] = bundle;
    }
  }

  api._syncTapLayout = function (taps) {
    var sig = tapsSignature(taps);
    if (sig === lastTapsKey) return;
    lastTapsKey = sig;
    rebuildTaps(taps);
  };

  api._renderTaps = function (frame) {
    if (!frame || !frame.slots) return;
    for (var i = 0; i < frame.slots.length; i++) {
      var s = frame.slots[i];
      var b = slotWidgets[s.s];
      if (!b) continue;
      if (b.meter) b.meter.update({ peak: s.p, rms: s.r });
      if (b.scope && s.w) b.scope.update(s.w);
      if (b.spectrum && s.m) b.spectrum.update(s.m);
    }
    // LEDs: iterate frame slots independently of slotWidgets since
    // LED-only taps don't appear there.
    for (var k = 0; k < frame.slots.length; k++) {
      var sl = frame.slots[k];
      if (typeof sl.g === "number") {
        applyLed(ledNodes[sl.s + ":gate_led"], "gate_led", sl.g);
      }
      // Trigger: bool latch. Fire-edge stamps the timestamp; rAF loop
      // animates the decay. Don't paint here — the loop owns the dot's
      // colour every frame regardless.
      if (sl.t === true) {
        triggerFireTime[sl.s] = (typeof performance !== "undefined" && performance.now)
          ? performance.now()
          : Date.now();
      }
    }
  };
  // rAF loop driving trigger LED decays. Cheap: handful of nodes,
  // one style write each per frame, only while triggers exist.
  function tickTriggerLeds() {
    var now = (typeof performance !== "undefined" && performance.now)
      ? performance.now()
      : Date.now();
    for (var key in ledNodes) {
      if (!Object.prototype.hasOwnProperty.call(ledNodes, key)) continue;
      var sep = key.indexOf(":");
      if (sep < 0) continue;
      if (key.slice(sep + 1) !== "trigger_led") continue;
      var slot = parseInt(key.slice(0, sep), 10);
      var fired = triggerFireTime[slot];
      var v = 0;
      if (typeof fired === "number") {
        var age = now - fired;
        v = Math.exp(-age / TRIGGER_DECAY_MS);
        if (v < 0.001) {
          v = 0;
          delete triggerFireTime[slot];
        }
      }
      applyLed(ledNodes[key], "trigger_led", v);
    }
    requestAnimationFrame(tickTriggerLeds);
  }
  if (typeof requestAnimationFrame === "function") {
    requestAnimationFrame(tickTriggerLeds);
  }

  api.dbConstants = {
    DB_AMBER_FLOOR: DB_AMBER_FLOOR,
    DB_RED_FLOOR: DB_RED_FLOOR,
    DB_FLOOR: DB_FLOOR,
  };
  api._ampToDb = ampToDb;
  api._dbToRatio = dbToRatio;
  api._dbColour = dbColour;

  // ── Halt banner ─────────────────────────────────────────────────
  api._renderHalt = function (message) {
    var el = document.getElementById("halt-banner");
    if (!el) return;
    if (message) {
      el.textContent = message;
      el.hidden = false;
    } else {
      el.textContent = "";
      el.hidden = true;
    }
  };

  // ── Diagnostics list ────────────────────────────────────────────
  api._renderDiagnostics = function (diags) {
    var root = document.getElementById("diagnostics");
    if (!root) return;
    root.innerHTML = "";
    if (!diags || diags.length === 0) {
      var empty = document.createElement("div");
      empty.className = "empty";
      empty.textContent = "No diagnostics.";
      root.appendChild(empty);
      return;
    }
    for (var i = 0; i < diags.length; i++) {
      var d = diags[i];
      var row = document.createElement("div");
      row.className = "diag-row sev-" + (d.severity || "note");
      var msg = document.createElement("div");
      msg.className = "diag-message";
      msg.textContent = d.message || "";
      row.appendChild(msg);
      var metaParts = [];
      if (d.location) metaParts.push(d.location);
      if (d.label) metaParts.push(d.label);
      if (d.code) metaParts.push("[" + d.code + "]");
      if (metaParts.length > 0) {
        var meta = document.createElement("div");
        meta.className = "diag-meta";
        meta.textContent = metaParts.join("  ");
        row.appendChild(meta);
      }
      root.appendChild(row);
    }
  };

  // ── Event log ───────────────────────────────────────────────────
  function pad2(n) { return n < 10 ? "0" + n : "" + n; }
  function nowHms() {
    var d = new Date();
    return pad2(d.getUTCHours()) + ":" + pad2(d.getUTCMinutes()) + ":" + pad2(d.getUTCSeconds());
  }

  // Track timestamps per status-log line. The Rust side does not stamp
  // entries (yet); we stamp on first sight so the UI shows when the
  // entry *arrived*, matching the TUI's `format_hms` column.
  var logStamps = []; // parallel array of HH:MM:SS for status_log entries
  var lastStatusLog = [];

  api._renderStatusLog = function (lines) {
    var el = document.getElementById("event-log");
    if (!el) return;
    lines = lines || [];
    // Diff: if the new array is a suffix-extended version, keep stamps;
    // if the old array length > new (truncation/eviction), drop stamps
    // from the front to match.
    if (lines.length < lastStatusLog.length) {
      var drop = lastStatusLog.length - lines.length;
      logStamps = logStamps.slice(drop);
    }
    // Stamp any newly-appearing tail entries.
    for (var i = logStamps.length; i < lines.length; i++) {
      logStamps.push(nowHms());
    }
    lastStatusLog = lines.slice();

    // Auto-scroll only when already pinned to the bottom.
    var atBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 4;
    el.innerHTML = "";
    for (var j = 0; j < lines.length; j++) {
      var row = document.createElement("div");
      row.className = "log-line";
      var t = document.createElement("span");
      t.className = "log-time";
      t.textContent = logStamps[j] || "";
      row.appendChild(t);
      var m = document.createTextNode(lines[j]);
      row.appendChild(m);
      el.appendChild(row);
    }
    if (atBottom) {
      el.scrollTop = el.scrollHeight;
    }
  };

  function activateTab(name) {
    var tabs = document.querySelectorAll(".tab");
    var panes = document.querySelectorAll(".pane");
    for (var i = 0; i < tabs.length; i++) {
      tabs[i].classList.toggle("is-active", tabs[i].dataset.pane === name);
    }
    for (var j = 0; j < panes.length; j++) {
      panes[j].classList.toggle("is-active", panes[j].dataset.pane === name);
    }
  }

  function postIntent(name, extra) {
    var msg = { kind: name };
    if (extra) {
      for (var k in extra) {
        if (Object.prototype.hasOwnProperty.call(extra, k)) msg[k] = extra[k];
      }
    }
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify(msg));
    }
  }

  api.postIntent = postIntent;

  // ── File header ─────────────────────────────────────────────────
  api._renderFilePath = function (path) {
    var el = document.getElementById("file-path");
    if (!el) return;
    if (path) {
      el.textContent = path;
      el.classList.add("has-path");
    } else {
      el.textContent = "no patch loaded";
      el.classList.remove("has-path");
    }
  };

  // ── Module scan paths list ──────────────────────────────────────
  api._renderModulePaths = function (paths) {
    var root = document.getElementById("module-paths");
    if (!root) return;
    root.innerHTML = "";
    if (!paths || paths.length === 0) {
      var empty = document.createElement("div");
      empty.className = "empty";
      empty.textContent = "no scan paths configured";
      root.appendChild(empty);
      return;
    }
    for (var i = 0; i < paths.length; i++) {
      var row = document.createElement("div");
      row.className = "path-row";
      var text = document.createElement("span");
      text.className = "path-text";
      text.textContent = paths[i];
      row.appendChild(text);
      var rm = document.createElement("button");
      rm.className = "btn btn-remove";
      rm.dataset.removeIndex = String(i);
      rm.textContent = "Remove";
      row.appendChild(rm);
      root.appendChild(row);
    }
  };

  document.addEventListener("change", function (ev) {
    var t = ev.target;
    if (!t || !t.classList || !t.classList.contains("tap-opt")) return;
    var slot;
    var payload = { kind: "set_tap_opts" };
    if (t.dataset.spectrumSlot !== undefined) {
      slot = parseInt(t.dataset.spectrumSlot, 10);
      if (t.dataset.spectrumOpt === "fft_size") {
        payload.spectrum_fft_size = parseInt(t.value, 10);
      }
    } else if (t.dataset.scopeSlot !== undefined) {
      slot = parseInt(t.dataset.scopeSlot, 10);
      if (t.dataset.scopeOpt === "decimation") {
        payload.scope_decimation = parseInt(t.value, 10);
      } else if (t.dataset.scopeOpt === "window") {
        payload.scope_window_samples = parseInt(t.value, 10);
      }
    }
    if (typeof slot !== "number" || isNaN(slot)) return;
    payload.slot = slot;
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify(payload));
    }
  });

  document.addEventListener("click", function (ev) {
    var t = ev.target;
    if (!t || !t.classList) return;
    if (t.classList.contains("tab")) {
      activateTab(t.dataset.pane);
      return;
    }
    if (t.classList.contains("btn-snap") && t.dataset.scopeSlot !== undefined) {
      var sslot = parseInt(t.dataset.scopeSlot, 10);
      var sbundle = slotWidgets[sslot];
      if (sbundle && sbundle.scope) {
        sbundle.scope.setSnap(!sbundle.scope.snap);
        t.classList.toggle("is-active", sbundle.scope.snap);
      }
      return;
    }
    if (t.classList.contains("btn-mode") && t.dataset.spectrumSlot !== undefined) {
      var slot = parseInt(t.dataset.spectrumSlot, 10);
      var bundle = slotWidgets[slot];
      if (bundle && bundle.spectrum) {
        var next = bundle.spectrum.mode === "heatmap" ? "curve" : "heatmap";
        bundle.spectrum.setMode(next);
        // Button label shows the *other* mode (what clicking switches to).
        t.textContent = next === "heatmap" ? "curve" : "heatmap";
      }
      return;
    }
    if (t.classList.contains("btn-remove") && t.dataset.removeIndex !== undefined) {
      postIntent("remove_path", { index: parseInt(t.dataset.removeIndex, 10) });
      return;
    }
    if (t.classList.contains("btn") && t.dataset.intent) {
      postIntent(t.dataset.intent);
    }
  });
})();
