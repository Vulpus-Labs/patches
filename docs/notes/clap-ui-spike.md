# CLAP UI spike: vizia vs webview

Input for a follow-up ADR. No decision made here — this is the
evaluation the ADR will cite.

- **Scope:** `patches-clap-vizia` (the existing plugin) and
  `patches-clap-webview` (the 0670-series spike).
- **Date:** 2026-04-24.
- **Author:** spike writeup for tickets E115 / 0674.

## Summary recommendation

**Keep both crates through one more iteration, then decide.** The
webview spike reaches feature parity with vizia at roughly one-third
the binary size on macOS and with a dramatically lighter dependency
graph, but the empirical questions that would force the decision —
cross-platform WebKitGTK behaviour, multi-instance memory footprint,
meter CPU under a real host — are not yet measured. The duplication
cost between the two crates is real but not yet painful enough to
force an early call.

The single biggest unresolved risk is Linux (WebKitGTK reliability
under wry, parented into a CLAP window). Until that is exercised in
at least one Linux DAW the webview route cannot be declared a
replacement, only an alternative.

## Axes

### Iteration speed

Subjective, macOS-only, one-developer:

- **vizia:** adding a widget means editing a Rust source file,
  rebuilding the plugin, and relaunching the host. Incremental rebuilds
  after a text change are ~3–5 s; relaunching the DAW to reload the
  plugin dominates the cycle. Layout is expressed in Rust DSL; errors
  surface as compile-time messages, which is good, but trial-and-error
  styling still needs full rebuilds.
- **webview:** HTML/CSS/JS lives under `assets/hello.html` and is
  `include_str!`'d into the binary. Changing only the HTML still
  requires a rebuild today (no asset hot-reload yet), but the rebuild
  is fast (no dependency graph invalidation beyond the webview crate).
  When iterating on layout or styling, the round-trip feels
  substantially shorter; stateful widgets that need Rust plumbing
  (e.g. meters, file dialogs) lose that advantage.

A useful quality-of-life improvement for the webview path would be a
dev-mode asset reloader — watch the HTML file and reinject on change
without rebuilding. Not done in this spike; flag for follow-up.

### Memory footprint

**Not yet measured.** Acceptance criterion carried forward to the
ADR. The interesting numbers are RSS for one instance and RSS for
four instances in the same host process, comparing vizia and webview
builds. Expectation:

- vizia: fixed overhead per instance (skia + baseview state).
- webview: potentially shared system WKWebView process on macOS; per-
  instance cost may be dominated by the HTML DOM rather than the
  renderer.

### CPU cost

**Not yet measured.** Three conditions per backend:

1. Plugin window closed.
2. Plugin window open, meters running.
3. Plugin window open, meters hidden (no canvas draws).

The meter path is the load-bearing question for the webview choice:
if `evaluate_script` at 10 Hz sits inside normal idle noise the
route is viable; if it consistently pushes the process above
quiescent CPU the route is in trouble.

(Ticket 0673 calls for 60 Hz measurement as a headroom check — run
that once with the real meter path and record numbers here.)

### Binary size (debug, macOS arm64)

| Backend  | Built dylib        |
|----------|--------------------|
| vizia    | **37.4 MB**        |
| webview  | **12.2 MB**        |

Debug numbers, so they overstate both. Release builds not yet
captured; ratio is expected to narrow but the direction should hold
— vizia bundles skia-safe plus baseview, webview relies on the
system's WKWebView (macOS) / WebView2 (Windows) / WebKitGTK (Linux).

### Cross-platform status

- **macOS:** Both backends work. Vizia is proven in daily use;
  webview reaches feature parity in the spike.
- **Windows:** Vizia: should work (baseview supports Win32), not
  recently exercised. Webview: wry supports WebView2 but the CLAP
  parented-child case has not been tested here.
- **Linux:** Vizia: should work (baseview supports X11). Webview:
  wry's WebKitGTK backend is the well-known weak point —
  parented-child reliability in CLAP hosts (Bitwig, Reaper) is
  unproven and is a real risk for the webview route. If it does not
  work, the webview crate is macOS+Windows only, which is a product
  decision rather than a technical one.

### Code volume

LOC per crate (Rust source only unless noted):

| Crate                    | LOC  | Notes                              |
|--------------------------|------|------------------------------------|
| patches-clap-vizia       | 2307 | includes 311-line `gui_vizia.rs` + 169-line `diagnostic_widget.rs` |
| patches-clap-webview     | 2005 | Rust only                          |
| patches-clap-webview HTML| 292  | `assets/hello.html`                |
| patches-plugin-common    | 336  | shared across both                 |

Plugin crate sizes are within ~15% of each other once assets are
counted. The dominant cost in each is `plugin.rs` (~990 lines) plus
`extensions.rs` (~640 lines), which are nearly identical between
the two.

### Duplication vs `patches-plugin-common`

**Shared (lives in `patches-plugin-common`):**

- `GuiState`, `GuiSnapshot`, `DiagnosticView`, `Intent`
- `MeterTap` (added for 0673)
- Status log plumbing.

**Duplicated between the two plugin crates:**

- `plugin.rs` — the full CLAP vtable. `diff` reports ~45 differing
  lines; the rest is byte-for-byte duplicated. Differences are:
  meter accumulator (webview only), GUI handle type, scale handling.
- `extensions.rs` — ~15 differing lines out of 640. Essentially the
  same file.
- `factory.rs`, `entry.rs`, `descriptor.rs`, `lib.rs`, `error.rs` —
  small files, mostly identical modulo crate names.

This is the real maintenance tax of keeping both crates: roughly
1600 lines of CLAP boilerplate that must be kept in sync by hand.
Any non-trivial change to the plugin surface (a new CLAP extension,
a new GuiState field, a bug in event handling) has to land twice.

**Should move into `patches-plugin-common` before committing to
keep-both:**

- The CLAP vtable body itself, parameterised by a trait that
  abstracts GUI create/destroy/update. This is a meaningful refactor
  — ~1600 lines → one trait with two implementations. The current
  "one implementation is not enough to design an abstraction around"
  note in `patches-plugin-common` is out of date now that two
  implementations exist.
- The `extensions.rs` module in its entirety; nothing there is
  toolkit-specific.

Without that extraction, keep-both is expensive. With it, keep-both
is cheap enough to defer the cross-platform decision until Linux
data is in hand.

### LLM-assisted iteration

Subjective. Across the 0670-series webview tickets:

- The webview flow (HTML + JS + Rust IPC) matched LLM assistance
  well. HTML/CSS is well-represented in training data; the bridge
  surface (`window.__patches.applyState`, `window.ipc.postMessage`)
  is small enough to keep in context and reason about.
- The vizia flow is Rust-heavy, depends on a less-mainstream library
  (vizia), and its API is still shifting. LLMs produce plausible but
  frequently wrong vizia code. Verifying against the live API was
  routine work.
- For meters and data-driven views specifically, the webview route
  won decisively: canvas 2D is boring, well-understood, and small
  diffs. The vizia equivalent would have needed custom `View::draw`
  implementations on top of Skia (see `reference_vizia_draw_api` in
  memory).

This asymmetry will persist for anything where the visual artefact
is the thing being iterated on, and will matter less for the
skeleton plumbing, which is written once.

## Open questions for the ADR

1. **Linux webview reliability.** Must be tested in at least one
   Linux CLAP host before keep-both can be justified. If it fails,
   keep-both means "vizia everywhere + webview on Mac/Windows" or
   "drop webview".
2. **Multi-instance memory.** Does a host with four plugin instances
   pay 4× or closer to 1× on the webview side (shared system
   WebView)?
3. **Meter CPU cost under real host conditions.** 60 Hz headroom
   number plus the 10 Hz production number from 0673.
4. **Asset hot-reload for the webview dev loop.** Worth the tooling
   or not?
5. **Refactor gate.** If we commit to keep-both, do the
   vtable-into-common refactor first, then iterate? Or accept the
   duplication tax for another cycle and reassess?
6. **Widget library fit.** Beyond the master meter, the full GUI
   needs: scopes, patch browser, parameter automation affordances,
   keyboard shortcuts. Which backend is cheaper to scale up through
   those?
7. **DPI / scaling.** Vizia handles this through baseview; webview
   relies on CSS. Does the webview path survive a retina/non-retina
   mix or a fractional-scale Windows display without extra work?

## Artefacts produced by the spike

- `patches-clap-webview` crate (0670–0673).
- `patches-plugin-common` extracted from the original vizia crate
  (0668).
- `MeterTap` (0673).
- This writeup.
