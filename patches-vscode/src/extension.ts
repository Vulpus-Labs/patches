import * as path from "path";
import * as fs from "fs";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

function resolveServerPath(context: vscode.ExtensionContext): string {
  // User override takes priority
  const config = vscode.workspace.getConfiguration("patches.lsp");
  const userPath = config.get<string>("path", "");
  if (userPath) {
    return userPath;
  }

  // Try bundled binary
  const binaryName =
    process.platform === "win32" ? "patches-lsp.exe" : "patches-lsp";
  const bundledPath = path.join(
    context.extensionPath,
    "server",
    binaryName
  );
  if (fs.existsSync(bundledPath)) {
    return bundledPath;
  }

  // Fall back to PATH lookup
  return "patches-lsp";
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
    clientOptions
  );

  client.start().catch((err) => {
    const msg = `Could not start patches-lsp: ${err.message}. ` +
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
      msg + platformHint +
        ` You can also set "patches.lsp.path" in settings to a custom binary path.`
    );
    client = undefined;
  });
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
