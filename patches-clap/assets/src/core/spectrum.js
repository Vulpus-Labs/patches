import { DB_FLOOR } from "./db.js";

export const SPECTRUM_DB_MAX = 6;

// Magma-ish ramp: dark purple → orange → pale yellow.
const HEAT_STOPS = [
  [0.00,   0,   0,   8],
  [0.20,  40,  10,  90],
  [0.40, 130,  35, 120],
  [0.60, 215,  70,  80],
  [0.80, 250, 150,  60],
  [1.00, 252, 250, 200],
];

export function heatColour(t) {
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

// Frequency / dB scaffolding for spectrum widgets.
export function makeScales(sampleRate, fftSize, w, h) {
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
