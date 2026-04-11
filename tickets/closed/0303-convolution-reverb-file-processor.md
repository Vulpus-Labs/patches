---
id: "0303"
title: "patches-modules: ConvolutionReverb FileProcessor impl"
priority: high
created: 2026-04-11
---

## Summary

Implement `FileProcessor` for `ConvolutionReverb`, replacing the `String`
path parameter with a `File` parameter. The `process_file` method reads the
audio file, resamples to the target sample rate, and returns pre-computed
FFT partition data as a flat `Vec<f32>`.

## Acceptance criteria

- [ ] `ConvolutionReverb` implements `FileProcessor`
- [ ] `process_file` for param `"ir_data"` (or chosen name): reads audio file via `patches_io`, resamples, partitions via `NonUniformConvolver`, serializes to `Vec<f32>` via `to_packed_vec`
- [ ] `process_file` respects `shape.high_quality` to select partition sizes or resampling quality
- [ ] The `"path"` string parameter is replaced with a `File` parameter (kind `ParameterKind::File { extensions: &["wav", "aiff"] }`)
- [ ] `update_validated_parameters` handles `ParameterValue::FloatBuffer`: takes the `Arc<[f32]>`, reconstructs `NonUniformConvolver` via `from_pre_fft`, and builds the processor infrastructure (thread, overlap buffer) asynchronously
- [ ] On the Install path (`update_parameters` on control thread), processor infrastructure is built synchronously as before, but from pre-FFT'd data
- [ ] Built-in IR variants (`room`, `hall`, `plate`) continue to work — `process_file` is only called for `ir: file` mode; synthetic IRs are generated in `update_parameters` as before
- [ ] Existing patches using `ir: file, path: "/absolute/path.aiff"` continue to work with updated syntax `ir: file, ir_data: file("/absolute/path.aiff")`
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy -p patches-modules` clean

## Notes

The parameter name for the file may change from `"path"` to something more
descriptive (e.g. `"ir_data"`) since it now carries processed data, not a
raw path. The DSL-facing name in the patch file would be the file parameter
name.

The async infrastructure for the Update path (building overlap buffer +
processor thread from pre-FFT'd data) is still needed — this happens on a
background thread, not the audio thread. But the work is cheaper since FFT
is already done.

Epic: E056
ADR: 0028
Depends: 0299, 0302
