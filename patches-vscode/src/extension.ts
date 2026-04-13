import * as path from "path";
import * as fs from "fs";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let graphPanel: vscode.WebviewPanel | undefined;
let refreshTimer: NodeJS.Timeout | undefined;
let lastRenderedUri: vscode.Uri | undefined;

const REFRESH_DEBOUNCE_MS = 300;

interface RenderSvgResult {
  svg: string;
  diagnostics: { message: string }[];
}

function resolveServerPath(context: vscode.ExtensionContext): string {
  const config = vscode.workspace.getConfiguration("patches.lsp");
  const userPath = config.get<string>("path", "");
  if (userPath) {
    return userPath;
  }

  const binaryName =
    process.platform === "win32" ? "patches-lsp.exe" : "patches-lsp";
  // Accept either `server/patches-lsp` (dir + binary) or `server` as a
  // single bundled binary file.
  const bundledInDir = path.join(context.extensionPath, "server", binaryName);
  if (fs.existsSync(bundledInDir) && fs.statSync(bundledInDir).isFile()) {
    return bundledInDir;
  }
  const bundledAsFile = path.join(context.extensionPath, "server");
  if (fs.existsSync(bundledAsFile) && fs.statSync(bundledAsFile).isFile()) {
    return bundledAsFile;
  }

  return "patches-lsp";
}

function isPatchesDoc(doc: vscode.TextDocument | undefined): boolean {
  return !!doc && doc.languageId === "patches";
}

async function requestSvg(uri: vscode.Uri): Promise<RenderSvgResult | undefined> {
  if (!client) {
    return undefined;
  }
  try {
    return await client.sendRequest<RenderSvgResult>("patches/renderSvg", {
      textDocument: { uri: uri.toString() },
    });
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    return { svg: "", diagnostics: [{ message }] };
  }
}

function scheduleRefresh(uri: vscode.Uri) {
  if (refreshTimer) {
    clearTimeout(refreshTimer);
  }
  refreshTimer = setTimeout(() => {
    refreshTimer = undefined;
    void refreshPanel(uri);
  }, REFRESH_DEBOUNCE_MS);
}

async function refreshPanel(uri: vscode.Uri) {
  if (!graphPanel) {
    return;
  }
  lastRenderedUri = uri;
  const result = await requestSvg(uri);
  if (!graphPanel) {
    return;
  }
  if (!result) {
    graphPanel.webview.postMessage({
      kind: "error",
      message: "Language server not running.",
    });
    return;
  }
  graphPanel.webview.postMessage({
    kind: "svg",
    svg: result.svg,
    diagnostics: result.diagnostics,
    fileName: path.basename(uri.fsPath),
  });
}

function webviewHtml(): string {
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<style>
  html, body {
    margin: 0;
    padding: 0;
    height: 100%;
    overflow: hidden;
    background: var(--vscode-editor-background);
    color: var(--vscode-editor-foreground);
    font-family: var(--vscode-font-family);
  }
  #toolbar {
    padding: 4px 8px;
    font-size: 12px;
    border-bottom: 1px solid var(--vscode-panel-border, transparent);
    display: flex;
    gap: 8px;
    align-items: center;
  }
  #status {
    color: var(--vscode-descriptionForeground);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  #viewport {
    position: absolute;
    top: 30px;
    left: 0;
    right: 0;
    bottom: 0;
    overflow: hidden;
    cursor: grab;
  }
  #viewport.dragging { cursor: grabbing; }
  #canvas {
    transform-origin: 0 0;
    position: absolute;
    top: 0;
    left: 0;
  }
  #canvas svg { display: block; }
  .error {
    padding: 12px;
    color: var(--vscode-errorForeground, #c94a4a);
    white-space: pre-wrap;
  }
  button {
    background: var(--vscode-button-secondaryBackground);
    color: var(--vscode-button-secondaryForeground);
    border: 1px solid var(--vscode-button-border, transparent);
    padding: 2px 8px;
    cursor: pointer;
    font-size: 12px;
  }
  button:hover { background: var(--vscode-button-secondaryHoverBackground); }
</style>
</head>
<body>
<div id="toolbar">
  <span id="status">No patch loaded</span>
  <button id="reset">Reset view</button>
</div>
<div id="viewport">
  <div id="canvas"></div>
</div>
<script>
(function () {
  const viewport = document.getElementById('viewport');
  const canvas = document.getElementById('canvas');
  const status = document.getElementById('status');
  const resetBtn = document.getElementById('reset');

  let scale = 1;
  let tx = 0;
  let ty = 0;
  let dragging = false;
  let lastX = 0;
  let lastY = 0;

  function applyTransform() {
    canvas.style.transform = 'translate(' + tx + 'px, ' + ty + 'px) scale(' + scale + ')';
  }

  function resetView() {
    scale = 1;
    tx = 0;
    ty = 0;
    applyTransform();
  }

  viewport.addEventListener('mousedown', (e) => {
    dragging = true;
    lastX = e.clientX;
    lastY = e.clientY;
    viewport.classList.add('dragging');
  });
  window.addEventListener('mouseup', () => {
    dragging = false;
    viewport.classList.remove('dragging');
  });
  window.addEventListener('mousemove', (e) => {
    if (!dragging) return;
    tx += e.clientX - lastX;
    ty += e.clientY - lastY;
    lastX = e.clientX;
    lastY = e.clientY;
    applyTransform();
  });
  viewport.addEventListener('wheel', (e) => {
    e.preventDefault();
    const rect = viewport.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
    const newScale = Math.max(0.1, Math.min(8, scale * factor));
    // Keep point under cursor fixed.
    tx = mx - (mx - tx) * (newScale / scale);
    ty = my - (my - ty) * (newScale / scale);
    scale = newScale;
    applyTransform();
  }, { passive: false });

  resetBtn.addEventListener('click', resetView);

  window.addEventListener('message', (event) => {
    const msg = event.data;
    if (msg.kind === 'svg') {
      canvas.innerHTML = msg.svg;
      const diagLine = (msg.diagnostics && msg.diagnostics.length > 0)
        ? ' — ' + msg.diagnostics.map((d) => d.message).join('; ')
        : '';
      status.textContent = (msg.fileName || '') + diagLine;
    } else if (msg.kind === 'error') {
      canvas.innerHTML = '<div class="error">' + escapeHtml(msg.message) + '</div>';
      status.textContent = 'Error';
    }
  });

  function escapeHtml(s) {
    return s.replace(/[&<>"']/g, (c) => ({
      '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
    }[c]));
  }
})();
</script>
</body>
</html>`;
}

function ensurePanel(context: vscode.ExtensionContext): vscode.WebviewPanel {
  if (graphPanel) {
    graphPanel.reveal(vscode.ViewColumn.Beside, true);
    return graphPanel;
  }
  const panel = vscode.window.createWebviewPanel(
    "patchesGraph",
    "Patch Graph",
    { viewColumn: vscode.ViewColumn.Beside, preserveFocus: true },
    {
      enableScripts: true,
      retainContextWhenHidden: true,
    },
  );
  panel.webview.html = webviewHtml();
  panel.onDidDispose(
    () => {
      graphPanel = undefined;
      lastRenderedUri = undefined;
      if (refreshTimer) {
        clearTimeout(refreshTimer);
        refreshTimer = undefined;
      }
    },
    undefined,
    context.subscriptions,
  );
  graphPanel = panel;
  return panel;
}

function registerCommands(context: vscode.ExtensionContext) {
  context.subscriptions.push(
    vscode.commands.registerCommand("patches.showPatchGraph", async () => {
      const editor = vscode.window.activeTextEditor;
      if (!isPatchesDoc(editor?.document)) {
        void vscode.window.showInformationMessage(
          "Open a .patches file to show the patch graph.",
        );
        return;
      }
      ensurePanel(context);
      await refreshPanel(editor!.document.uri);
    }),
  );

  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      if (!graphPanel || !isPatchesDoc(editor?.document)) {
        return;
      }
      scheduleRefresh(editor!.document.uri);
    }),
  );

  context.subscriptions.push(
    vscode.workspace.onDidChangeTextDocument((event) => {
      if (!graphPanel || !isPatchesDoc(event.document)) {
        return;
      }
      const activeUri = vscode.window.activeTextEditor?.document.uri;
      if (!activeUri || activeUri.toString() !== event.document.uri.toString()) {
        return;
      }
      scheduleRefresh(event.document.uri);
    }),
  );

  context.subscriptions.push(
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (!graphPanel || !isPatchesDoc(doc)) {
        return;
      }
      scheduleRefresh(doc.uri);
    }),
  );
}

export function activate(context: vscode.ExtensionContext) {
  const serverPath = resolveServerPath(context);

  const serverOptions: ServerOptions = {
    command: serverPath,
    args: [],
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "patches" }],
  };

  client = new LanguageClient(
    "patches-lsp",
    "Patches Language Server",
    serverOptions,
    clientOptions,
  );

  client.start().catch((err) => {
    const msg =
      `Could not start patches-lsp: ${err.message}. ` +
      `Syntax highlighting is still active.`;
    let platformHint = "";
    if (process.platform === "darwin") {
      platformHint =
        ` On macOS you may need to remove the quarantine attribute: ` +
        `run "xattr -d com.apple.quarantine ${serverPath}" in a terminal.`;
    } else if (process.platform === "win32") {
      platformHint =
        ` On Windows, right-click the binary in Properties and check "Unblock".`;
    }
    vscode.window.showWarningMessage(
      msg +
        platformHint +
        ` You can also set "patches.lsp.path" in settings to a custom binary path.`,
    );
    client = undefined;
  });

  registerCommands(context);
}

export function deactivate(): Thenable<void> | undefined {
  if (refreshTimer) {
    clearTimeout(refreshTimer);
    refreshTimer = undefined;
  }
  if (graphPanel) {
    graphPanel.dispose();
    graphPanel = undefined;
  }
  if (!client) {
    return undefined;
  }
  return client.stop();
}
