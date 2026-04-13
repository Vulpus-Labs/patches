---
id: "0391"
title: VS Code Show Patch Graph command and webview panel
priority: high
created: 2026-04-13
---

## Summary

Add a `patches.showPatchGraph` command to the VS Code extension that opens a
side-panel webview and displays the SVG returned by the LSP's
`patches/renderSvg` request (ticket 0390). Update on document edits.

## Scope

- `patches-vscode/src/extension.ts`:
  - Register command `patches.showPatchGraph`.
  - Create `WebviewPanel` with `ViewColumn.Beside`, `retainContextWhenHidden: true`.
  - Track a single shared panel (reveal instead of recreating on re-invocation).
  - On command invocation and on active-editor change (for `.patches` files),
    send `client.sendRequest('patches/renderSvg', { textDocument: { uri } })`
    and post the SVG to the webview via `webview.postMessage`.
  - Debounce document-change updates (~300 ms).
- Webview HTML:
  - Minimal page that renders the posted SVG into a container.
  - Pan/zoom: start with inline zoom (wheel + drag), add a dependency only if
    the hand-rolled version gets messy.
  - Dark/light aware: use `vscode-theme` CSS variables for the background.
- `patches-vscode/package.json`:
  - Add command contribution with title "Patches: Show Patch Graph".
  - Add `editor/title` menu entry visible when `resourceExtname == .patches`.

## Acceptance criteria

- [ ] Command opens a side panel that displays the current patch as SVG.
- [ ] Panel updates (debounced) on edits to the active `.patches` file.
- [ ] Switching between `.patches` documents updates the panel to the new file.
- [ ] Panel survives tab switches without a full reload (webview retained).
- [ ] Graceful handling when the LSP returns an error (show the message in
      the panel, don't crash the extension).
- [ ] Existing LSP features (completion, hover, diagnostics) unaffected.

## Notes

- Avoid server-side rate limiting for now; debouncing client-side is enough.
- Keep webview JS minimal — no bundler changes unless strictly necessary.
