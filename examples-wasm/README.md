# WASM module examples

> **Experimental / shelved.** These examples were built as part of an
> implementation spike and are not part of the active build. They may not compile
> against the current `patches-core` API. See
> [ADR 0027](../adr/0027-wasm-module-loading.md) for design rationale and spike
> findings.

Example `.wasm` modules loadable by the patches player.

## wavefolder-rs (Rust)

Sine-based wavefolder with configurable and CV-modulatable drive.

| Port        | Direction | Kind | Description                           |
|-------------|-----------|------|---------------------------------------|
| `in`        | input     | mono | Audio input                           |
| `drive_cv`  | input     | mono | Bipolar CV added to drive (±20 range) |
| `out`       | output    | mono | Folded audio output                   |

| Parameter | Type  | Range     | Default |
|-----------|-------|-----------|---------|
| `drive`   | float | 1.0–20.0  | 1.0     |

Build:

```bash
cargo build -p example-wavefolder-wasm --target wasm32-unknown-unknown --release
# Output: target/wasm32-unknown-unknown/release/example_wavefolder_wasm.wasm
```

## mid-side-as (AssemblyScript)

Stereo mid/side encoder/decoder. Feed stereo into `in_left`/`in_right` to get
`mid_out`/`side_out`; feed processed mid/side back into `mid_in`/`side_in` to
get reconstructed stereo on `out_left`/`out_right`.

| Port        | Direction | Kind | Description               |
|-------------|-----------|------|---------------------------|
| `in_left`   | input     | mono | Stereo left input         |
| `in_right`  | input     | mono | Stereo right input        |
| `mid_in`    | input     | mono | Mid return (for decoding) |
| `side_in`   | input     | mono | Side return (for decoding)|
| `mid_out`   | output    | mono | Encoded mid signal        |
| `side_out`  | output    | mono | Encoded side signal       |
| `out_left`  | output    | mono | Decoded stereo left       |
| `out_right` | output    | mono | Decoded stereo right      |

No parameters.

Build:

```bash
cd examples-wasm/mid-side-as
npm install
npm run build
# Output: build/mid_side.wasm
```
