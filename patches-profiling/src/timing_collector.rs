use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Mutex;

use patches_core::InstanceId;

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
        let mut map = self.inner.lock().unwrap();
        let e = map.entry((id, name)).or_insert_with(|| Entry {
            module_name: name,
            ..Entry::default()
        });
        e.process_calls += 1;
        e.process_total_ns += nanos;
    }

    /// Record one `periodic_update()` call of `nanos` wall-clock nanoseconds.
    pub fn record_periodic(&self, id: InstanceId, name: &'static str, nanos: u64) {
        let mut map = self.inner.lock().unwrap();
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
        let map = self.inner.lock().unwrap();
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
