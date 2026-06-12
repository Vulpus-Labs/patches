use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Mutex;

use patches_core::{ExpectInvariant, InstanceId};

/// Per-instance timing record returned by [`TimingCollector::report`].
#[derive(Debug, Clone)]
pub struct TimingRecord {
    pub module_name: &'static str,
    pub instance_id: InstanceId,
    pub process_calls: u64,
    pub process_total_ns: u64,
    pub periodic_calls: u64,
    pub periodic_total_ns: u64,
}

impl TimingRecord {
    /// Combined ns across process and periodic.
    pub fn total_ns(&self) -> u64 {
        self.process_total_ns + self.periodic_total_ns
    }
}

#[derive(Default)]
struct Entry {
    module_name: &'static str,
    process_calls: u64,
    process_total_ns: u64,
    periodic_calls: u64,
    periodic_total_ns: u64,
}

/// Shared accumulator for timing data recorded by [`TimingShim`](crate::timing_shim::TimingShim).
///
/// Wrap in `Arc` and clone the `Arc` into each shim. `Mutex`-based; not suitable
/// for use on the audio thread.
#[derive(Default)]
pub struct TimingCollector {
    inner: Mutex<HashMap<(InstanceId, &'static str), Entry>>,
}

impl TimingCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one `process()` call of `nanos` wall-clock nanoseconds.
    pub fn record_process(&self, id: InstanceId, name: &'static str, nanos: u64) {
        let mut map = self.inner.lock().expect_invariant("timing collector mutex poisoned");
        let e = map.entry((id, name)).or_insert_with(|| Entry {
            module_name: name,
            ..Entry::default()
        });
        e.process_calls += 1;
        e.process_total_ns += nanos;
    }

    /// Record one `periodic_update()` call of `nanos` wall-clock nanoseconds.
    pub fn record_periodic(&self, id: InstanceId, name: &'static str, nanos: u64) {
        let mut map = self.inner.lock().expect_invariant("timing collector mutex poisoned");
        let e = map.entry((id, name)).or_insert_with(|| Entry {
            module_name: name,
            ..Entry::default()
        });
        e.periodic_calls += 1;
        e.periodic_total_ns += nanos;
    }

    /// Return one [`TimingRecord`] per `(InstanceId, name)` pair, sorted by
    /// combined total time descending.
    pub fn report(&self) -> Vec<TimingRecord> {
        let map = self.inner.lock().expect_invariant("timing collector mutex poisoned");
        let mut records: Vec<TimingRecord> = map
            .iter()
            .map(|(&(id, _), e)| TimingRecord {
                module_name: e.module_name,
                instance_id: id,
                process_calls: e.process_calls,
                process_total_ns: e.process_total_ns,
                periodic_calls: e.periodic_calls,
                periodic_total_ns: e.periodic_total_ns,
            })
            .collect();
        records.sort_by_key(|r| Reverse(r.total_ns()));
        records
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{AUTOCONV_PREFIX, AUTOSUM_PREFIX};

    /// Ticket 0857 + ADR 0074: synthesised auto-Sum (`__autosum_*`) and
    /// auto-conv (`__autoconv_*`) modules must not surface in the
    /// profiler under their synthetic instance id. The collector is
    /// keyed by `(InstanceId, &'static str)` where the string is the
    /// module's descriptor *type* name (e.g. `"Sum"`, `"MonoToPoly"`),
    /// set by [`crate::timing_shim::TimingShim::new`] from
    /// `Module::descriptor().module_name`. Per-instance rows therefore
    /// carry the type name and a numeric `InstanceId`; the QName-based
    /// `__autosum_*` / `__autoconv_*` id never reaches this layer.
    /// This test pins the invariant — should that ever change, the
    /// report would start leaking synthesised names and the profiler
    /// view would diverge from the user's authored patch.
    #[test]
    fn synthesised_names_do_not_reach_collector() {
        let collector = TimingCollector::new();
        let id = InstanceId::next();
        // The shim records the *type* name. A synthesised node is an
        // instance of `Sum`/`PolySum`/`StereoSum` (autosum) or
        // `MonoToPoly`/`PolyToMono` (autoconv); its descriptor returns
        // the type name, which is what gets recorded here.
        collector.record_process(id, "Sum", 100);
        collector.record_periodic(id, "MonoToPoly", 50);

        for r in collector.report() {
            assert!(
                !r.module_name.starts_with(AUTOSUM_PREFIX)
                    && !r.module_name.starts_with(AUTOCONV_PREFIX),
                "synthesised name leaked into TimingRecord: {}",
                r.module_name
            );
        }
    }
}
