import { DB_FLOOR, ampToDb, dbToRatio, dbColour } from "../core/db.js";
import { findFirstRisingCross } from "../core/scope.js";
import { SPECTRUM_DB_MAX, heatColour, makeScales } from "../core/spectrum.js";

// Canvas2D meter widget. `orientation` is "vertical" or "horizontal".
// `update({ peak, rms })` redraws from a linear-amplitude pair.
export function meterWidget(canvas, orientation) {
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

export function scopeWidget(canvas, opts) {
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

  return { update, draw, setSnap, getSnap: () => snap };
}

export function spectrumWidget(canvas, opts = {}) {
  const sampleRate = opts.sampleRate ?? 48000;
  const fftSize = opts.fftSize ?? 1024;
  let mags = null;
  let mode = opts.mode === "heatmap" ? "heatmap" : "curve";
  let heatImage = null;

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
    const s = makeScales(sampleRate, fftSize, w, h);
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
    const s = makeScales(sampleRate, fftSize, w, h);

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

  return { update, draw, setMode, getMode: () => mode };
}
