// ── Halt banner ─────────────────────────────────────────────────
api._renderHalt = (message) => {
  const el = document.getElementById("halt-banner");
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
api._renderDiagnostics = (diags) => {
  const root = document.getElementById("diagnostics");
  if (!root) return;
  root.innerHTML = "";
  if (!diags || diags.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "No diagnostics.";
    root.appendChild(empty);
    return;
  }
  for (const d of diags) {
    const row = document.createElement("div");
    row.className = `diag-row sev-${d.severity ?? "note"}`;
    const msg = document.createElement("div");
    msg.className = "diag-message";
    msg.textContent = d.message ?? "";
    row.appendChild(msg);
    const metaParts = [];
    if (d.location) metaParts.push(d.location);
    if (d.label) metaParts.push(d.label);
    if (d.code) metaParts.push(`[${d.code}]`);
    if (metaParts.length > 0) {
      const meta = document.createElement("div");
      meta.className = "diag-meta";
      meta.textContent = metaParts.join("  ");
      row.appendChild(meta);
    }
    root.appendChild(row);
  }
};

// ── Event log ───────────────────────────────────────────────────
const pad2 = (n) => (n < 10 ? `0${n}` : `${n}`);
function nowHms() {
  const d = new Date();
  return `${pad2(d.getUTCHours())}:${pad2(d.getUTCMinutes())}:${pad2(d.getUTCSeconds())}`;
}

// Track timestamps per status-log line. The Rust side does not stamp
// entries (yet); we stamp on first sight so the UI shows when the
// entry *arrived*, matching the TUI's `format_hms` column.
let logStamps = [];
let lastStatusLog = [];

api._renderStatusLog = (lines) => {
  const el = document.getElementById("event-log");
  if (!el) return;
  lines = lines ?? [];
  if (lines.length < lastStatusLog.length) {
    logStamps = logStamps.slice(lastStatusLog.length - lines.length);
  }
  for (let i = logStamps.length; i < lines.length; i++) {
    logStamps.push(nowHms());
  }
  lastStatusLog = lines.slice();

  const atBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 4;
  el.innerHTML = "";
  for (let j = 0; j < lines.length; j++) {
    const row = document.createElement("div");
    row.className = "log-line";
    const t = document.createElement("span");
    t.className = "log-time";
    t.textContent = logStamps[j] ?? "";
    row.appendChild(t);
    row.appendChild(document.createTextNode(lines[j]));
    el.appendChild(row);
  }
  if (atBottom) el.scrollTop = el.scrollHeight;
};

// ── File header ─────────────────────────────────────────────────
api._renderFilePath = (path) => {
  const el = document.getElementById("file-path");
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
api._renderModulePaths = (paths) => {
  const root = document.getElementById("module-paths");
  if (!root) return;
  root.innerHTML = "";
  if (!paths || paths.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "no scan paths configured";
    root.appendChild(empty);
    return;
  }
  for (let i = 0; i < paths.length; i++) {
    const row = document.createElement("div");
    row.className = "path-row";
    const text = document.createElement("span");
    text.className = "path-text";
    text.textContent = paths[i];
    row.appendChild(text);
    const rm = document.createElement("button");
    rm.className = "btn btn-remove";
    rm.dataset.removeIndex = String(i);
    rm.textContent = "Remove";
    row.appendChild(rm);
    root.appendChild(row);
  }
};

// ── Loaded modules list ─────────────────────────────────────────
api._renderModuleNames = (names) => {
  const root = document.getElementById("module-names");
  if (!root) return;
  root.innerHTML = "";
  if (!names || names.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "no modules loaded";
    root.appendChild(empty);
    return;
  }
  for (const name of names) {
    const row = document.createElement("div");
    row.className = "module-row";
    row.textContent = name;
    root.appendChild(row);
  }
};

function activateTab(name) {
  for (const tab of document.querySelectorAll(".tab")) {
    tab.classList.toggle("is-active", tab.dataset.pane === name);
  }
  for (const pane of document.querySelectorAll(".pane")) {
    pane.classList.toggle("is-active", pane.dataset.pane === name);
  }
}
