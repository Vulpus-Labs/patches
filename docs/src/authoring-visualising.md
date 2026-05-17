# Visualising patches

The text DSL is the source of truth, but for an unfamiliar patch a
diagram is faster to read than the file. Patches can render any
`.patches` file as an SVG signal-flow graph: modules as boxes,
ports on the sides, cables as routed arrows between them.

Rendering is a feature of the language server (`patches-lsp`),
exposed through:

- The VS Code extension command **Show Patch Graph**.
- The LSP `patches/renderSvg` custom request, for any LSP client.
- The `patches_svg` library crate, for programmatic use.
- The `patches-svg-cli` workspace crate, which produces a `patches-svg`
  binary (`publish = false`, build with `cargo build -p patches-svg-cli`).

## From VS Code

Install the [VS Code extension](install-vscode.md). Open a `.patches`
file. Then either:

- Right-click in the editor → **Show Patch Graph**.
- Command Palette (Cmd+Shift+P / Ctrl+Shift+P) → **Patches: Show
  Patch Graph**.

A side panel opens with the SVG, scrollable and zoomable. The panel
re-renders when you save the file (the LSP refreshes on each
successful parse), so editing the patch and watching the graph
update side-by-side is a usable authoring loop.

If the patch has a parse error, the panel shows the diagnostic
instead of the (stale) graph. Fix the error and save; the graph
returns.

## Reading the diagram

A rendered patch is a sequence of columns laid out left-to-right by
topological depth: sources (oscillators, MIDI sources, sequencers)
on the left, sinks (`AudioOut`) on the right, everything else
ordered by the longest signal path that reaches it.

Inside each module box:

- The **module type** is the label across the top (`PolyOsc`,
  `Adsr`, `StereoMixer`).
- The **instance name** is below the type, smaller (`osc`, `env`,
  `mix`).
- **Input ports** are dots on the left edge, labelled with their
  port names.
- **Output ports** are dots on the right edge, likewise labelled.

Cables are arrows from output dots to input dots. Scaled cables
(`-[0.5]->`) carry the scale as a small label near the destination.

Mono cables are drawn as single lines; poly cables and stereo
cables are styled slightly thicker, with a small annotation marking
the kind. (Exact visual coding may evolve — when in doubt, the
SVG's CSS classes name each kind.)

The layout is computed by the `rust-sugiyama` crate (a Sugiyama-
family layered-graph layout). Crossings are minimised but not
eliminated; expect occasional overlaps in dense patches.

## Themes

Two built-in themes:

- **Dark** (default) — light strokes and labels on a dark
  background. Suited to the VS Code dark theme.
- **Light** — dark strokes on a light background. Suited to the
  VS Code light theme and to embedding in print.

The VS Code extension currently renders with the default (dark)
theme. To render a `light` SVG programmatically:

```rust
use patches_svg::{render_svg, SvgOptions, Theme};

let svg = render_svg(
    &flat_patch,
    &flat_patch.module_descriptors,
    &SvgOptions { theme: Theme::Light, ..Default::default() },
);
```

`SvgOptions` has a handful of additional knobs (margins, font size,
port-label visibility); see the crate's source for the current set.

## Programmatic rendering

If you want diagrams in a build pipeline (auto-generated docs,
README badges, a static-site catalogue), depend on `patches_svg`
directly:

```toml
[dependencies]
patches-dsl = { path = "../patches-dsl" }
patches-svg = { path = "../patches-svg" }
```

The flow is: parse the source with `patches_dsl`, flatten it
(template expansion, stereo desugaring), then call
`patches_svg::render_svg` with the resulting `FlatPatch` and the
module-descriptor table. The output is a `String` of SVG markup
ready to write to disk or embed.

Two consumers in-tree:

- `patches-lsp` calls `render_svg` from its `patches/renderSvg`
  endpoint.
- `patches-vscode` requests SVGs from the LSP and renders them in
  a webview panel.

## When the diagram helps

Reading SVGs is the fastest way to:

- **Onboard onto an unfamiliar patch.** Layout shows the
  architecture at a glance: subtractive, FM, sequencer-driven, etc.
- **Spot fan-out you forgot about.** A module driving five
  destinations is obvious in the diagram; in text it's spread
  across a dozen lines.
- **See the feedback structure.** Cycles in the graph are drawn as
  back-edges and are unmissable.

It's less useful while authoring, when the text is faster to edit
than the diagram is to scan. Authors typically read the text and
use the diagram as a sanity check after structural changes.
