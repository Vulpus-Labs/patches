---
id: "0182"
title: Add lib target and TimingCollector to patches-profiling
priority: medium
created: 2026-03-24
epic: E033
---

## Summary

Add a `[lib]` target to `patches-profiling/Cargo.toml` and implement
`TimingCollector` in `src/timing_collector.rs`. This provides the shared
accumulator that `TimingShim` (T-0183) will write into and `profile.rs`
(T-0184) will read from. No changes to other crates.

## Acceptance criteria

- [ ] `patches-profiling/Cargo.toml` has a `[lib]` section pointing at `src/lib.rs`
- [ ] `src/lib.rs` declares `pub mod timing_collector`
- [ ] `TimingCollector` is an `Arc`-shareable type backed by a `Mutex`
- [ ] `TimingCollector::record_process(id: InstanceId, name: &'static str, nanos: u64)` accumulates call count and total ns
- [ ] `TimingCollector::record_periodic(id: InstanceId, name: &'static str, nanos: u64)` accumulates call count and total ns separately
- [ ] `TimingCollector::report() -> Vec<TimingRecord>` returns one entry per `(InstanceId, name)` pair, sorted by total time descending
- [ ] `TimingRecord` exposes: `module_name`, `instance_id`, `process_calls`, `process_total_ns`, `periodic_calls`, `periodic_total_ns`
- [ ] `cargo clippy -p patches-profiling` clean

## Notes

`InstanceId` is in `patches-core`. The `Mutex` + `HashMap` approach is fine
here — `patches-profiling` has no audio-thread constraints. Key type is
`(InstanceId, &'static str)` so the reporter can group by module type or sort
per-instance.
