// dB thresholds — must match patches-player/src/tui.rs.
const DB_AMBER_FLOOR = -18;
const DB_RED_FLOOR = -6;
const DB_FLOOR = -60;

function ampToDb(amp) {
  if (amp <= 0) return DB_FLOOR;
  const db = 20 * Math.log10(amp);
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

api.dbConstants = { DB_AMBER_FLOOR, DB_RED_FLOOR, DB_FLOOR };
api._ampToDb = ampToDb;
api._dbToRatio = dbToRatio;
api._dbColour = dbColour;
