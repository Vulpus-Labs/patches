// dB thresholds — must match patches-player/src/tui.rs.
export const DB_AMBER_FLOOR = -18;
export const DB_RED_FLOOR = -6;
export const DB_FLOOR = -60;

export function ampToDb(amp) {
  if (amp <= 0) return DB_FLOOR;
  const db = 20 * Math.log10(amp);
  return db < DB_FLOOR ? DB_FLOOR : db;
}

export function dbToRatio(db) {
  if (db < DB_FLOOR) db = DB_FLOOR;
  if (db > 0) db = 0;
  return (db - DB_FLOOR) / -DB_FLOOR;
}

export function dbColour(db) {
  if (db >= DB_RED_FLOOR) return "#e04040";
  if (db >= DB_AMBER_FLOOR) return "#e0a040";
  return "#40c060";
}
