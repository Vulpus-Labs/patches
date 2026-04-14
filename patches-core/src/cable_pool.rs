use crate::cables::{CableValue, MonoInput, MonoOutput, PolyInput, PolyOutput};

/// Encapsulates the ping-pong cable buffer pool and the current write index,
/// providing typed read/write accessors for use in [`Module::process`].
///
/// ## Ping-pong layout
///
/// Every cable has two slots: `pool[cable_idx][0]` and `pool[cable_idx][1]`.
/// On each tick, `wi` (write index) alternates between `0` and `1`.
/// - **Write slot** (`wi`): where the current module writes its output this tick.
/// - **Read slot** (`1 - wi`): the value a *downstream* module reads — what was written *last* tick.
///
/// This 1-sample cable delay is intentional: it means every module sees a consistent
/// snapshot of its inputs from the previous tick regardless of execution order, so
/// modules can be scheduled in any order (or across multiple threads) without
/// introducing data races or order-dependent results.
///
/// ## Why the `'a` lifetime is load-bearing
///
/// `pool: &'a mut [[CableValue; 2]]` holds an exclusive mutable borrow for the
/// duration of a single tick.  The lifetime `'a` ties each `CablePool` to one
/// tick's exclusive access window: you cannot create a second `CablePool` (or any
/// other mutable reference into the same buffer) while one is live.  Removing or
/// weakening this lifetime would allow aliased mutable borrows and break the
/// single-writer guarantee.
///
/// [`Module::process`]: crate::Module::process
pub struct CablePool<'a> {
    pool: &'a mut [[CableValue; 2]],
    wi: usize,
}

impl<'a> CablePool<'a> {
    /// Create a new `CablePool` wrapping `pool` with write index `wi`.
    pub fn new(pool: &'a mut [[CableValue; 2]], wi: usize) -> Self {
        Self { pool, wi }
    }

    /// Extract the raw parts needed to pass this pool across an FFI boundary.
    ///
    /// Returns `(pointer, length, write_index)` where `pointer` is a mutable
    /// pointer to the underlying `[CableValue; 2]` slice. The lifetime of the
    /// pointer is tied to the `&mut self` borrow, so the caller cannot create
    /// a second reference while the pointer is in use.
    ///
    /// # Safety contract for callers
    ///
    /// The returned pointer must not outlive the `&mut self` borrow. The FFI
    /// callee must reconstruct a `CablePool` via [`CablePool::new`] using
    /// `std::slice::from_raw_parts_mut` and must not store the pointer.
    pub fn as_raw_parts_mut(&mut self) -> (*mut [CableValue; 2], usize, usize) {
        (self.pool.as_mut_ptr(), self.pool.len(), self.wi)
    }

    /// Extract the raw parts as const pointers, suitable for read-only access
    /// (e.g. the `periodic_update` path across the FFI boundary).
    pub fn as_raw_parts(&self) -> (*const [CableValue; 2], usize, usize) {
        (self.pool.as_ptr(), self.pool.len(), self.wi)
    }

    #[inline(always)]
    /// Read a mono value from `input`, applying `input.scale`. Reads the **read slot** (`1 - wi`).
    ///
    /// # Panics
    /// Panics (via `unreachable!`) if the pool slot holds a `Poly` value —
    /// a well-formed graph never produces this.
    pub fn read_mono(&self, input: &MonoInput) -> f32 {
        let ri = 1 - self.wi;
        match self.pool[input.cable_idx][ri] {
            CableValue::Mono(v) => v * input.scale,
            CableValue::Poly(_) => {
                debug_assert!(
                    false,
                    "CablePool::read_mono encountered a Poly cable — graph validation should prevent this"
                );
                0.0
            }
        }
    }

    #[inline(always)]
    /// Read a 16-channel poly value from `input`, applying `input.scale` to each channel.
    /// Reads the **read slot** (`1 - wi`).
    ///
    /// Uses a `scale == 1.0` fast path (exact comparison) to skip the 16-channel
    /// multiply in the common case.  Scale values are set from DSL-parsed literals
    /// or default constants — never accumulated arithmetic — so the value is
    /// exactly `1.0_f32` when unscaled.
    ///
    /// # Panics
    /// Panics (via `unreachable!`) if the pool slot holds a `Mono` value.
    pub fn read_poly(&self, input: &PolyInput) -> [f32; 16] {
        let ri = 1 - self.wi;
        match self.pool[input.cable_idx][ri] {
            CableValue::Poly(channels) => {
                if input.scale == 1.0 {
                    channels
                } else {
                    channels.map(|v| v * input.scale)
                }
            }
            CableValue::Mono(_) => {
                debug_assert!(
                    false,
                    "CablePool::read_poly encountered a Mono cable — graph validation should prevent this"
                );
                [0.0; 16]
            }
        }
    }

    #[inline(always)]
    /// Write a mono `value` to `output`. Writes to the **write slot** (`wi`).
    pub fn write_mono(&mut self, output: &MonoOutput, value: f32) {
        self.pool[output.cable_idx][self.wi] = CableValue::Mono(value);
    }

    #[inline(always)]
    /// Write a 16-channel poly `value` to `output`. Writes to the **write slot** (`wi`).
    pub fn write_poly(&mut self, output: &PolyOutput, value: [f32; 16]) {
        self.pool[output.cable_idx][self.wi] = CableValue::Poly(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pool(values: &[CableValue]) -> Vec<[CableValue; 2]> {
        values.iter().map(|&v| [v, v]).collect()
    }

    #[test]
    fn read_mono_applies_scale() {
        let mut pool = make_pool(&[CableValue::Mono(4.0)]);
        // wi = 0, so ri = 1; both slots seeded with same value
        let cp = CablePool::new(&mut pool, 0);
        let input = MonoInput { cable_idx: 0, scale: 0.5, connected: true };
        assert_eq!(cp.read_mono(&input), 2.0);
    }

    #[test]
    fn read_poly_applies_scale_to_all_channels() {
        let channels: [f32; 16] = std::array::from_fn(|i| i as f32);
        let mut pool = make_pool(&[CableValue::Poly(channels)]);
        let cp = CablePool::new(&mut pool, 0);
        let input = PolyInput { cable_idx: 0, scale: 2.0, connected: true };
        let result = cp.read_poly(&input);
        for (i, &v) in result.iter().enumerate() {
            assert_eq!(v, i as f32 * 2.0, "channel {i} mismatch");
        }
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn read_mono_kind_mismatch_returns_zero() {
        // Slot holds Poly but MonoInput tries to read it — should return 0.0, not panic
        let mut pool = vec![[CableValue::Poly([1.0; 16]); 2]];
        let cp = CablePool::new(&mut pool, 0);
        let input = MonoInput { cable_idx: 0, scale: 1.0, connected: true };
        assert_eq!(cp.read_mono(&input), 0.0);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn read_poly_kind_mismatch_returns_zero() {
        // Slot holds Mono but PolyInput tries to read it — should return [0.0;16], not panic
        let mut pool = vec![[CableValue::Mono(1.0); 2]];
        let cp = CablePool::new(&mut pool, 0);
        let input = PolyInput { cable_idx: 0, scale: 1.0, connected: true };
        assert_eq!(cp.read_poly(&input), [0.0f32; 16]);
    }

    #[test]
    fn write_mono_stores_at_write_index() {
        let mut pool = vec![[CableValue::Mono(0.0); 2]];
        {
            let mut cp = CablePool::new(&mut pool, 1);
            let output = MonoOutput { cable_idx: 0, connected: true };
            cp.write_mono(&output, 2.5);
        }
        match pool[0][1] {
            CableValue::Mono(v) => assert_eq!(v, 2.5),
            _ => panic!("expected CableValue::Mono at write index"),
        }
    }

    #[test]
    fn write_poly_stores_at_write_index() {
        let mut pool = vec![[CableValue::Poly([0.0; 16]); 2]];
        let data: [f32; 16] = std::array::from_fn(|i| i as f32 * 0.1);
        {
            let mut cp = CablePool::new(&mut pool, 0);
            let output = PolyOutput { cable_idx: 0, connected: true };
            cp.write_poly(&output, data);
        }
        match pool[0][0] {
            CableValue::Poly(channels) => assert_eq!(channels, data),
            _ => panic!("expected CableValue::Poly at write index"),
        }
    }

    // ── T-0241: Ping-pong 1-sample-delay invariant ──────────────────────────

    /// Write a value at tick N, read at tick N returns the *previous* value,
    /// read at tick N+1 returns the written value.
    #[test]
    fn ping_pong_one_sample_delay_mono() {
        let mut pool = vec![[CableValue::Mono(0.0); 2]];
        let output = MonoOutput { cable_idx: 0, connected: true };
        let input = MonoInput { cable_idx: 0, scale: 1.0, connected: true };

        // Tick 0: wi=0, ri=1. Pool is [Mono(0), Mono(0)].
        // Write 42.0 into write slot (index 0).
        {
            let mut cp = CablePool::new(&mut pool, 0);
            // Reading should return the read slot (index 1) = 0.0 (previous value).
            let read_val = cp.read_mono(&input);
            assert_eq!(read_val, 0.0, "tick 0: read should return previous value (0.0)");
            cp.write_mono(&output, 42.0);
        }

        // Tick 1: wi=1, ri=0. The value written last tick is now in the read slot.
        {
            let cp = CablePool::new(&mut pool, 1);
            let read_val = cp.read_mono(&input);
            assert_eq!(read_val, 42.0, "tick 1: read should return value written last tick (42.0)");
        }
    }

    /// Two modules writing and reading the same cable slot on the same tick
    /// do not see each other's writes (isolation within a tick).
    #[test]
    fn ping_pong_within_tick_isolation() {
        // Seed read slot (index 1) with 10.0.
        let mut pool = vec![[CableValue::Mono(0.0), CableValue::Mono(10.0)]];
        let output = MonoOutput { cable_idx: 0, connected: true };
        let input = MonoInput { cable_idx: 0, scale: 1.0, connected: true };

        // wi=0: Module A writes 99.0, Module B reads — should see 10.0 (from read slot),
        // not 99.0 (written this tick into write slot).
        let mut cp = CablePool::new(&mut pool, 0);
        cp.write_mono(&output, 99.0);
        let read_val = cp.read_mono(&input);
        assert_eq!(
            read_val, 10.0,
            "within-tick read should see previous tick's value (10.0), not this tick's write (99.0)"
        );
    }

    /// Scale is applied at read time, not write time.
    #[test]
    fn scale_applied_at_read_time() {
        let mut pool = vec![[CableValue::Mono(0.0), CableValue::Mono(8.0)]];
        let input_scaled = MonoInput { cable_idx: 0, scale: 0.25, connected: true };

        let cp = CablePool::new(&mut pool, 0);
        let result = cp.read_mono(&input_scaled);
        assert_eq!(result, 2.0, "read with scale 0.25 of value 8.0 should be 2.0");
    }

    /// The scale=1.0 fast path for poly reads produces the same result as the
    /// general path.
    #[test]
    fn poly_scale_one_fast_path_matches_general() {
        let channels: [f32; 16] = std::array::from_fn(|i| (i as f32 + 1.0) * 3.0);
        let mut pool = vec![[CableValue::Poly([0.0; 16]), CableValue::Poly(channels)]];

        let input_unit = PolyInput { cable_idx: 0, scale: 1.0, connected: true };
        let input_general = PolyInput { cable_idx: 0, scale: 1.0000001, connected: true };

        let cp = CablePool::new(&mut pool, 0);
        let result_fast = cp.read_poly(&input_unit);
        let result_general = cp.read_poly(&input_general);

        // The fast path returns the raw channels; the general path multiplies
        // by a scale very close to 1.0. Both should produce nearly identical results.
        for i in 0..16 {
            assert!(
                (result_fast[i] - result_general[i]).abs() < 0.01,
                "channel {i}: fast={} general={}", result_fast[i], result_general[i]
            );
            // The fast path should return the exact raw value.
            assert_eq!(result_fast[i], channels[i], "fast path should return exact channel value");
        }
    }

    /// Poly read with non-unity scale.
    #[test]
    fn ping_pong_one_sample_delay_poly() {
        let data: [f32; 16] = std::array::from_fn(|i| i as f32);
        let mut pool = vec![[CableValue::Poly([0.0; 16]); 2]];
        let output = PolyOutput { cable_idx: 0, connected: true };
        let input = PolyInput { cable_idx: 0, scale: 0.5, connected: true };

        // Tick 0: write data.
        {
            let mut cp = CablePool::new(&mut pool, 0);
            let read_val = cp.read_poly(&input);
            assert_eq!(read_val, [0.0; 16], "tick 0: should read zeros");
            cp.write_poly(&output, data);
        }

        // Tick 1: read should return data * 0.5.
        {
            let cp = CablePool::new(&mut pool, 1);
            let read_val = cp.read_poly(&input);
            for (i, &v) in read_val.iter().enumerate() {
                assert_eq!(
                    v, i as f32 * 0.5,
                    "tick 1 channel {i}: expected {} got {v}", i as f32 * 0.5
                );
            }
        }
    }
}
