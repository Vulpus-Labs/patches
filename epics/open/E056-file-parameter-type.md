# E056 — File parameter type with planner-side processing

## Goal

Introduce a `file("path")` parameter type in the DSL so that modules can
declare file dependencies, have those files loaded and pre-processed on the
control thread during plan building, and receive the processed data as
`FloatBuffer(Arc<[f32]>)` parameter values. Errors propagate through the
normal build error path.

After this epic:

- The DSL supports `file("path")` syntax; paths resolve relative to the
  patch file.
- `ParameterKind::File` and `ParameterValue::File` / `FloatBuffer` exist
  in patches-core.
- A `FileProcessor` trait lets modules define how to process file contents.
- The planner calls `process_file` for all `File` parameters before
  creating the execution plan.
- ConvolutionReverb and StereoConvReverb use `file()` parameters with
  pre-computed spectral data, replacing the `IrLoader` async machinery.
- File load errors are reported through the CLAP GUI and player console.

## Background

ADR 0028 documents the design. Key decisions:

- **`process_file` is a static method** called by the planner on the
  control thread — no async loading, no module instance needed.
- **`Arc<[f32]>` parameter value** — O(1) clone, cheap to move through the
  plan ring buffer.
- **Pre-computed FFT partitions** — ConvolutionReverb's `process_file`
  returns frequency-domain data, skipping the expensive FFT at module
  init time.
- **Path resolution in the interpreter** — relative paths resolve against
  the patch file's parent directory before reaching the planner.

## Tickets

| ID     | Title                                                                | Dependencies   |
| ------ | -------------------------------------------------------------------- | -------------- |
| 0297   | patches-core: File parameter kind and FloatBuffer value              | —              |
| 0298   | patches-dsl: parse file() syntax                                     | —              |
| 0299   | patches-core: FileProcessor trait and registry support               | 0297           |
| 0300   | patches-interpreter: map file() to ParameterValue::File              | 0297, 0298     |
| 0301   | patches-engine: planner resolves File params via FileProcessor       | 0299           |
| 0302   | patches-dsp: NonUniformConvolver from pre-FFT'd data                 | —              |
| 0303   | patches-modules: ConvolutionReverb FileProcessor impl                | 0299, 0302     |
| 0304   | patches-modules: remove IrLoader async machinery                     | 0303           |
| 0305   | patches-modules: StereoConvReverb FileProcessor impl                 | 0303           |
| 0306   | Integration tests: file parameter round-trip                         | 0301, 0303     |
| 0307   | patches-ffi: File and FloatBuffer ABI support                        | 0297, 0299     |

Epic: E056
ADR: 0028
