// Canvas2D meter widget. `orientation` is "vertical" or "horizontal".
// `update({ peak, rms })` redraws from a linear-amplitude pair.
function meterWidget(canvas, orientation) {
  const horizontal = orientation === "horizontal";
  let peakDb = DB_FLOOR;
  let rmsDb = DB_FLOOR;

  function draw() {
    const ctx = canvas.getContext("2d");
    const w = canvas.width;
    const h = canvas.height;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = "#1a1a1a";
    ctx.fillRect(0, 0, w, h);

    const rmsRatio = dbToRatio(rmsDb);
    const peakRatio = dbToRatio(peakDb);
    ctx.fillStyle = dbColour(rmsDb);
    if (horizontal) {
      ctx.fillRect(0, 0, Math.round(w * rmsRatio), h);
      if (peakDb > DB_FLOOR) {
        ctx.fillStyle = dbColour(peakDb);
        ctx.fillRect(Math.min(Math.round(w * peakRatio), w - 1), 0, 2, h);
      }
    } else {
      const rmsH = Math.round(h * rmsRatio);
      ctx.fillRect(0, h - rmsH, w, rmsH);
      if (peakDb > DB_FLOOR) {
        ctx.fillStyle = dbColour(peakDb);
        ctx.fillRect(0, Math.max(h - Math.round(h * peakRatio) - 1, 0), w, 2);
      }
    }
  }

  function update(sample) {
    peakDb = ampToDb(typeof sample?.peak === "number" ? sample.peak : 0);
    rmsDb = ampToDb(typeof sample?.rms === "number" ? sample.rms : 0);
    draw();
  }

  return { update, draw };
}

// Oscilloscope widget — line plot over the full buffer width, ±1.0
// amplitude rails. `update(samples)` accepts any Float32Array-like;
// the widget owns no buffer state.
// `snap` (boolean) — if true, rotate the buffer so the latest rising
// zero-cross sits at index 0. Toggleable client-side at no cost since
// the server sends raw decimated samples.
// First rising zero-cross in s[1..end). Schmitt-armed: requires the
// signal to have dipped below -eps before the upward zero-cross to
// suppress retrigger on noise / harmonic ripple. eps = 5% of the
// window peak amplitude. Returns null if none. Ticket 0754.
function findFirstRisingCross(s, end) {
  let peak = 0;
  for (let i = 0; i < end; i++) {
    const a = s[i] < 0 ? -s[i] : s[i];
    if (a > peak) peak = a;
  }
  const eps = peak * 0.05;
  let armed = false;
  for (let j = 1; j < end; j++) {
    if (s[j] < -eps) armed = true;
    else if (armed && s[j - 1] < 0 && s[j] >= 0) return j;
  }
  return null;
}

function scopeWidget(canvas, opts) {
  let samples = null;
  let snap = !!opts?.snap;

  function draw() {
    const ctx = canvas.getContext("2d");
    const w = canvas.width;
    const h = canvas.height;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = "#0f0f0f";
    ctx.fillRect(0, 0, w, h);

    ctx.strokeStyle = "#404040";
    ctx.lineWidth = 1;
    for (const rail of [-1, 0, 1]) {
      const y = ((1 - rail) / 2) * (h - 1);
      ctx.beginPath();
      ctx.moveTo(0, y + 0.5);
      ctx.lineTo(w, y + 0.5);
      ctx.stroke();
    }

    if (!samples || samples.length < 2) return;

    // Always display a fixed-length tail of the buffer. The first
    // quarter is reserved as a trigger search region; the remaining
    // three-quarters are drawn across the full canvas width. Snap
    // mode shifts the start to a rising zero-cross within the search
    // region; unsnapped mode just uses the natural offset. Same
    // sample-per-pixel scale either way.
    const n = samples.length;
    const searchEnd = n >> 2;
    const displayLen = n - searchEnd;
    let start = searchEnd;
    if (snap && n >= 4) {
      const k0 = findFirstRisingCross(samples, searchEnd);
      if (k0 !== null) start = k0;
    }

    ctx.strokeStyle = "#40d0e0";
    ctx.lineWidth = 1;
    ctx.beginPath();
    const stride = (w - 1) / (displayLen - 1);
    for (let j = 0; j < displayLen; j++) {
      let v = samples[start + j];
      if (v > 1) v = 1; else if (v < -1) v = -1;
      const x = j * stride;
      const yy = ((1 - v) / 2) * (h - 1);
      if (j === 0) ctx.moveTo(x, yy); else ctx.lineTo(x, yy);
    }
    ctx.stroke();
  }

  function update(s) {
    samples = s ?? null;
    draw();
  }

  function setSnap(next) {
    snap = !!next;
    draw();
  }

  const getSnap = () => snap;

  return { update, draw, setSnap, getSnap };
}

// Spectrum widget — log-X frequency, dB-Y magnitude with floor at
// DB_FLOOR (-60). Bin centre frequency is `k * sampleRate / fftSize`.
// Two display modes:
//   "curve":   filled area under a smooth line per latest frame.
//   "heatmap": rolling waterfall, latest column on the right.
// Defaults match patches-observation::processor::SPECTRUM_FFT_SIZE
// (1024) and a 48 kHz host rate.
const SPECTRUM_DB_MAX = 6;

// Magma-ish ramp: dark purple → orange → pale yellow.
const HEAT_STOPS = [
  [0.00,   0,   0,   8],
  [0.20,  40,  10,  90],
  [0.40, 130,  35, 120],
  [0.60, 215,  70,  80],
  [0.80, 250, 150,  60],
  [1.00, 252, 250, 200],
];
function heatColour(t) {
  if (t < 0) t = 0; else if (t > 1) t = 1;
  for (let i = 1; i < HEAT_STOPS.length; i++) {
    if (t <= HEAT_STOPS[i][0]) {
      const a = HEAT_STOPS[i - 1];
      const b = HEAT_STOPS[i];
      const u = (t - a[0]) / (b[0] - a[0]);
      return [
        Math.round(a[1] + (b[1] - a[1]) * u),
        Math.round(a[2] + (b[2] - a[2]) * u),
        Math.round(a[3] + (b[3] - a[3]) * u),
      ];
    }
  }
  const last = HEAT_STOPS[HEAT_STOPS.length - 1];
  return [last[1], last[2], last[3]];
}

function spectrumWidget(canvas, opts = {}) {
  const sampleRate = opts.sampleRate ?? 48000;
  const fftSize = opts.fftSize ?? 1024;
  let mags = null;
  let mode = opts.mode === "heatmap" ? "heatmap" : "curve";
  // Heatmap backing buffer: ImageData scrolled left by 1 column per
  // frame. Allocated lazily on first heatmap draw.
  let heatImage = null;

  function scales(w, h) {
    const binHz = sampleRate / fftSize;
    const fMin = binHz;
    let fMax = sampleRate / 2;
    if (fMax <= fMin) fMax = fMin * 10;
    const logMin = Math.log10(fMin);
    const logMax = Math.log10(fMax);
    return {
      binHz, fMin, fMax, logMin, logMax,
      xFor: (freq) => ((Math.log10(freq) - logMin) / (logMax - logMin)) * (w - 1),
      yFor: (db) => {
        if (db < DB_FLOOR) db = DB_FLOOR;
        if (db > SPECTRUM_DB_MAX) db = SPECTRUM_DB_MAX;
        return ((SPECTRUM_DB_MAX - db) / (SPECTRUM_DB_MAX - DB_FLOOR)) * (h - 1);
      },
    };
  }

  function drawGrid(ctx, w, h, s) {
    ctx.strokeStyle = "#303030";
    ctx.lineWidth = 1;
    for (const decade of [100, 1000, 10000]) {
      if (decade > s.fMax) break;
      const gx = s.xFor(decade);
      ctx.beginPath();
      ctx.moveTo(gx + 0.5, 0);
      ctx.lineTo(gx + 0.5, h);
      ctx.stroke();
    }
    for (const db of [-40, -20, 0]) {
      const gy = s.yFor(db);
      ctx.beginPath();
      ctx.moveTo(0, gy + 0.5);
      ctx.lineTo(w, gy + 0.5);
      ctx.stroke();
    }
  }

  function drawCurve(ctx, w, h) {
    ctx.fillStyle = "#0f0f0f";
    ctx.fillRect(0, 0, w, h);
    const s = scales(w, h);
    drawGrid(ctx, w, h, s);

    if (!mags || mags.length < 2) return;

    const n = mags.length;
    const nyquist = (n - 1) * s.binHz;
    const { logMin, logMax } = s;

    const colDb = new Float32Array(w);
    for (let x = 0; x < w; x++) {
      const t = x / (w - 1);
      const logF = logMin + t * (logMax - logMin);
      const freq = Math.pow(10, logF);
      if (freq > nyquist) { colDb[x] = DB_FLOOR; continue; }
      const binF = freq / s.binHz;
      const nextLogF = logMin + ((x + 1) / (w - 1)) * (logMax - logMin);
      const nextBinF = Math.pow(10, nextLogF) / s.binHz;
      const span = nextBinF - binF;
      let m;
      if (span <= 1) {
        const lo = Math.max(1, Math.floor(binF));
        const hi = Math.min(n - 1, lo + 1);
        const frac = binF - lo;
        m = (1 - frac) * mags[lo] + frac * mags[hi];
      } else {
        const k0 = Math.max(1, Math.floor(binF));
        const k1 = Math.min(n - 1, Math.ceil(binF + span));
        m = 0;
        for (let k = k0; k <= k1; k++) {
          if (mags[k] > m) m = mags[k];
        }
      }
      colDb[x] = m <= 0 ? DB_FLOOR : 20 * Math.log10(m);
    }

    const smooth = new Float32Array(w);
    for (let i = 0; i < w; i++) {
      const a = colDb[Math.max(0, i - 1)];
      const b = colDb[i];
      const c = colDb[Math.min(w - 1, i + 1)];
      smooth[i] = (a + 2 * b + c) * 0.25;
    }

    const floorY = s.yFor(DB_FLOOR);

    const pathThrough = () => {
      ctx.moveTo(0, s.yFor(smooth[0]));
      for (let i = 1; i < w - 1; i++) {
        const mx = (i + i + 1) * 0.5;
        const my = (s.yFor(smooth[i]) + s.yFor(smooth[i + 1])) * 0.5;
        ctx.quadraticCurveTo(i, s.yFor(smooth[i]), mx, my);
      }
      ctx.lineTo(w - 1, s.yFor(smooth[w - 1]));
    };

    ctx.beginPath();
    ctx.moveTo(0, floorY);
    pathThrough();
    ctx.lineTo(w - 1, floorY);
    ctx.closePath();
    ctx.fillStyle = "rgba(64, 192, 224, 0.25)";
    ctx.fill();

    ctx.beginPath();
    pathThrough();
    ctx.strokeStyle = "#40c0e0";
    ctx.lineWidth = 1.5;
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    ctx.stroke();
  }

  function drawHeatmap(ctx, w, h) {
    const s = scales(w, h);

    if (!heatImage || heatImage.width !== w || heatImage.height !== h) {
      heatImage = ctx.createImageData(w, h);
      const floor = heatColour(0);
      const data0 = heatImage.data;
      for (let p = 0; p < data0.length; p += 4) {
        data0[p]     = floor[0];
        data0[p + 1] = floor[1];
        data0[p + 2] = floor[2];
        data0[p + 3] = 255;
      }
    }

    if (mags && mags.length >= 2) {
      const data = heatImage.data;
      const rowBytes = 4 * w;
      for (let y = 0; y < h; y++) {
        const base = y * rowBytes;
        data.copyWithin(base, base + 4, base + rowBytes);
      }

      const n = mags.length;
      const lastCol = w - 1;
      for (let y2 = 0; y2 < h; y2++) {
        const t = y2 / (h - 1);
        const logF = s.logMax - t * (s.logMax - s.logMin);
        const freq = Math.pow(10, logF);
        let bin = Math.round(freq / s.binHz);
        if (bin < 1) bin = 1;
        if (bin >= n) bin = n - 1;
        const m2 = mags[bin];
        let db2 = m2 <= 0 ? DB_FLOOR : 20 * Math.log10(m2);
        if (db2 < DB_FLOOR) db2 = DB_FLOOR;
        if (db2 > SPECTRUM_DB_MAX) db2 = SPECTRUM_DB_MAX;
        const u2 = (db2 - DB_FLOOR) / (SPECTRUM_DB_MAX - DB_FLOOR);
        const rgb = heatColour(u2);
        const off = (y2 * w + lastCol) * 4;
        data[off]     = rgb[0];
        data[off + 1] = rgb[1];
        data[off + 2] = rgb[2];
        data[off + 3] = 255;
      }
    }

    ctx.putImageData(heatImage, 0, 0);
  }

  function draw() {
    const ctx = canvas.getContext("2d");
    const w = canvas.width;
    const h = canvas.height;
    ctx.clearRect(0, 0, w, h);
    if (mode === "heatmap") drawHeatmap(ctx, w, h);
    else drawCurve(ctx, w, h);
  }

  function update(m) {
    mags = m ?? null;
    draw();
  }

  function setMode(next) {
    next = next === "heatmap" ? "heatmap" : "curve";
    if (next === mode) return;
    mode = next;
    heatImage = null;
    draw();
  }

  const getMode = () => mode;

  return { update, draw, setMode, getMode };
}

api.spectrumWidget = spectrumWidget;
api.scopeWidget = scopeWidget;
api.meterWidget = meterWidget;
