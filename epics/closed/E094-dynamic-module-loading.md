---
id: "E094"
title: Dynamic module loading and reload across player, CLAP, LSP
created: 2026-04-19
depends_on: ["E088"]
tickets: ["0562", "0563", "0564", "0565", "0566", "0567", "0568"]
---

## Goal

Make external FFI plugin bundles first-class across every Registry
consumer. After this epic:

- Every loaded builder and instance holds an `Arc<libloading::Library>`
  so dylibs unload only when no longer referenced.
- Bundles declare per-module versions; Registry keeps the highest
  seen.
- A shared `PluginScanner` + `ScanReport` drives discovery for all
  consumers.
- `patches-player` takes `--module-path` on the CLI.
- `patches-clap` persists module paths in plugin state, scans on
  activate, and exposes a GUI "Rescan" action implemented as a
  hard-stop reload.
- `patches-lsp` reads `patches.modulePaths` from workspace
  configuration and exposes a custom `patches/rescanModules` command.
- VSCode extension surfaces the setting and a `patches.rescanModules`
  command bound to the LSP request.

Implements ADR 0044. Depends on E088 (multi-module bundle ABI v2).

## Tickets

| ID   | Title                                                       | Priority | Depends on |
| ---- | ----------------------------------------------------------- | -------- | ---------- |
| 0562 | Arc<Library> lifetime on DylibModuleBuilder + DylibModule  | high     | E088       |
| 0563 | Per-module version symbol + version-aware Registry insert   | high     | 0562       |
| 0564 | PluginScanner + ScanReport shared type                      | high     | 0562, 0563 |
| 0565 | patches-player: --module-path CLI, scan pre-compile         | medium   | 0564       |
| 0566 | patches-clap: persisted paths, activate-scan, rescan button | high     | 0564       |
| 0567 | patches-lsp: workspace config paths + rescan custom command | high     | 0564       |
| 0568 | patches-vscode: settings schema + rescan command wiring     | medium   | 0567       |

## Definition of done

- `Arc<Library>` held by every builder and instance; library unload
  proven via refcount test.
- Registry replacement policy: higher module version wins; equal or
  lower is skipped and reported.
- `PluginScanner::scan(&Registry)` returns a populated `ScanReport`.
- Player CLI accepts one or more `--module-path DIR` flags, scans once
  before compiling the patch.
- CLAP plugin persists `module_paths` as part of state; on `activate`
  it scans; GUI rescan button triggers full hard-stop reload (stop
  audio → drop plan → rescan → recompile → resume).
- LSP reads `patches.modulePaths` on init and on `workspace/configuration`
  change; `patches/rescanModules` custom request rebuilds the registry
  and refreshes diagnostics.
- VSCode extension contributes `patches.modulePaths` (array of strings)
  setting and `patches.rescanModules` command; command invokes the LSP
  custom request.
- `cargo build`, `cargo test`, `cargo clippy` clean; no `unwrap`/`expect`
  added to library code.
