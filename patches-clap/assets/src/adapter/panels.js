import { nowHms, computeLogStamps } from "../core/log.js";
import { formatDiagnostic } from "../core/diagnostics.js";

export function renderHalt(message) {
  const el = document.getElementById("halt-banner");
  if (!el) return;
  if (message) {
    el.textContent = message;
    el.hidden = false;
  } else {
    el.textContent = "";
    el.hidden = true;
  }
}

export function renderDiagnostics(diags) {
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
    const view = formatDiagnostic(d);
    const row = document.createElement("div");
    row.className = `diag-row ${view.sevClass}`;
    const msg = document.createElement("div");
    msg.className = "diag-message";
    msg.textContent = view.message;
    row.appendChild(msg);
    if (view.metaText !== null) {
      const meta = document.createElement("div");
      meta.className = "diag-meta";
      meta.textContent = view.metaText;
      row.appendChild(meta);
    }
    root.appendChild(row);
  }
}

let logStamps = [];
let prevStatusLen = 0;

export function renderStatusLog(lines) {
  const el = document.getElementById("event-log");
  if (!el) return;
  lines = lines ?? [];
  logStamps = computeLogStamps(logStamps, prevStatusLen, lines.length, nowHms());
  prevStatusLen = lines.length;

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
}

export function renderFilePath(path) {
  const el = document.getElementById("file-path");
  if (!el) return;
  if (path) {
    el.textContent = path;
    el.classList.add("has-path");
  } else {
    el.textContent = "no patch loaded";
    el.classList.remove("has-path");
  }
}

export function renderModulePaths(paths) {
  const root = document.getElementById("module-paths");
  if (!root) return;
  root.innerHTML = "";
  if (!paths || paths.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "no bundle directories configured";
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
    // Path string is the controller's canonical key for the action;
    // sending it verbatim avoids index drift when the list mutates
    // between click + dispatch.
    rm.dataset.removePath = paths[i];
    rm.textContent = "Remove";
    row.appendChild(rm);
    root.appendChild(row);
  }
}

export function renderModuleNames(names) {
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
}

export function activateTab(name) {
  for (const tab of document.querySelectorAll(".tab")) {
    tab.classList.toggle("is-active", tab.dataset.pane === name);
  }
  for (const pane of document.querySelectorAll(".pane")) {
    pane.classList.toggle("is-active", pane.dataset.pane === name);
  }
}
