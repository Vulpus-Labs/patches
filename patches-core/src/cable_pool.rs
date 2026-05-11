use crate::cables::{CableValue, MonoInput, MonoOutput, PolyInput, PolyOutput, StereoInput, StereoOutput, StereoSample, SCRATCH_CAPACITY};

/// Encapsulates the two-region cable buffer pool and the current write
/// index, providing typed read/write accessors for use in
/// [`Module::process`].
///
/// ## Layout: scratch low, cycle high (ADR 0072 phase 5, ticket 0860)
///
/// - **Scratch region** `[0, SCRATCH_CAPACITY)` —
///   `scratch: &[CableValue]`. Sinks (`[0, SINK_SLOTS)`), backplane
///   (`[SINK_SLOTS, RESERVED_SLOTS)`), and dyn cables whose every
///   consumer is fused (`[RESERVED_SLOTS, SCRATCH_CAPACITY)`). One
///   slot per cable — no read ever needs last tick's value.
/// - **Cycle region** `[SCRATCH_CAPACITY, SCRATCH_CAPACITY +
///   CYCLE_CAPACITY)` — `cycle: &[[CableValue; 2]]`. Backs cables
///   whose producer port has at least one delayed (non-fused)
///   consumer. Each slot stores a ping-pong pair: writes land at
///   `[wi]`, delayed reads see `[1 - wi]`.
///
/// `cable_idx` indexes into a single virtual number space with a
/// single cutoff: `cable_idx < SCRATCH_CAPACITY` dispatches to
/// scratch, otherwise to cycle offset by `SCRATCH_CAPACITY`.
///
/// ## Why the `'a` lifetime is load-bearing
///
/// The two slice borrows hold exclusive mutable access for the
/// duration of a single tick. The lifetime `'a` ties each `CablePool`
/// to one tick's exclusive access window: you cannot create a second
/// `CablePool` (or any other mutable reference into either backing
/// store) while one is live. Removing or weakening this lifetime
/// would allow aliased mutable borrows and break the single-writer
/// guarantee.
///
/// [`Module::process`]: crate::Module::process
pub struct CablePool<'a> {
    scratch: &'a mut [CableValue],
    cycle: &'a mut [[CableValue; 2]],
    wi: usize,
}

impl<'a> CablePool<'a> {
    /// Create a new `CablePool` from separate scratch and cycle
    /// backing stores at write index `wi`.
    ///
    /// `scratch.len()` must cover every scratch-region `cable_idx` in
    /// use — at minimum `RESERVED_SLOTS` so the sink + backplane reads
    /// stay in bounds. In production the engine sizes this at
    /// `SCRATCH_CAPACITY`; tests use
    /// [`patches_core::test_support::reserved_scratch`] for the
    /// reserved-only minimum. `cycle.len()` must cover every cycle
    /// producer (typically `CYCLE_CAPACITY`).
    pub fn new(
        scratch: &'a mut [CableValue],
        cycle: &'a mut [[CableValue; 2]],
        wi: usize,
    ) -> Self {
        Self { scratch, cycle, wi }
    }

    /// Extract the raw parts needed to pass this pool across an FFI
    /// boundary. Returns separate scratch and cycle pointers (ABI v10).
    ///
    /// # Safety contract for callers
    ///
    /// The returned pointers must not outlive the `&mut self` borrow.
    /// The FFI callee must reconstruct a `CablePool` via
    /// [`CablePool::new`] using `std::slice::from_raw_parts_mut` and
    /// must not store the pointers.
    pub fn as_raw_parts_mut(&mut self) -> CablePoolRawPartsMut {
        CablePoolRawPartsMut {
            scratch_ptr: self.scratch.as_mut_ptr(),
            scratch_len: self.scratch.len(),
            cycle_ptr: self.cycle.as_mut_ptr(),
            cycle_len: self.cycle.len(),
            wi: self.wi,
        }
    }

    /// Const-pointer counterpart for the read-only `periodic_update`
    /// FFI path.
    pub fn as_raw_parts(&self) -> CablePoolRawParts {
        CablePoolRawParts {
            scratch_ptr: self.scratch.as_ptr(),
            scratch_len: self.scratch.len(),
            cycle_ptr: self.cycle.as_ptr(),
            cycle_len: self.cycle.len(),
            wi: self.wi,
        }
    }

    /// Read raw `CableValue` at `cable_idx`. Dispatches on region.
    /// `fused` selects the ping-pong half for cycle slots; ignored for
    /// scratch slots (which are inherently same-tick).
    #[inline(always)]
    fn read_raw(&self, cable_idx: usize, fused: bool) -> CableValue {
        if cable_idx < SCRATCH_CAPACITY {
            debug_assert!(
                fused,
                "scratch slot read with fused=false (cable_idx={cable_idx}); \
                 scratch implies all consumers fused (ADR 0072 phase 5)"
            );
            self.scratch[cable_idx]
        } else {
            let cycle_idx = cable_idx - SCRATCH_CAPACITY;
            let si = if fused { self.wi } else { 1 - self.wi };
            self.cycle[cycle_idx][si]
        }
    }

    /// Write raw `CableValue` at `cable_idx`. Dispatches on region.
    #[inline(always)]
    fn write_raw(&mut self, cable_idx: usize, value: CableValue) {
        if cable_idx < SCRATCH_CAPACITY {
            self.scratch[cable_idx] = value;
        } else {
            self.cycle[cable_idx - SCRATCH_CAPACITY][self.wi] = value;
        }
    }

    #[inline(always)]
    /// Read a mono value from `input`, applying `input.scale`.
    ///
    /// Dispatches on `input.cable_idx` vs [`SCRATCH_CAPACITY`] (region)
    /// and, for cycle slots, on `input.fused` (ping-pong half).
    /// Lanes outside lane 0 are unspecified for a Mono cable per ADR 0068.
    pub fn read_mono(&self, input: &MonoInput) -> f32 {
        let v = self.read_raw(input.cable_idx, input.fused).as_mono();
        let y = v * input.scale + input.offset;
        match input.clip {
            Some((lo, hi)) => y.clamp(lo, hi),
            None => y,
        }
    }

    #[inline(always)]
    /// Read a 16-channel poly value from `input`, applying `input.scale` to each channel.
    ///
    /// See [`read_mono`](Self::read_mono) for region/slot dispatch.
    ///
    /// Uses a `scale == 1.0` fast path (exact comparison) to skip the 16-channel
    /// multiply in the common case.  Scale values are set from DSL-parsed literals
    /// or default constants — never accumulated arithmetic — so the value is
    /// exactly `1.0_f32` when unscaled.
    pub fn read_poly(&self, input: &PolyInput) -> [f32; 16] {
        let channels = self.read_raw(input.cable_idx, input.fused).as_poly();
        if input.scale == 1.0 && input.offset == 0.0 && input.clip.is_none() {
            channels
        } else {
            let scale = input.scale;
            let offset = input.offset;
            match input.clip {
                Some((lo, hi)) => channels.map(|v: f32| (v * scale + offset).clamp(lo, hi)),
                None => channels.map(|v: f32| v * scale + offset),
            }
        }
    }

    #[inline(always)]
    /// Write a mono `value` to `output`. Dispatches on region.
    pub fn write_mono(&mut self, output: &MonoOutput, value: f32) {
        self.write_raw(output.cable_idx, CableValue::mono(value));
    }

    #[inline(always)]
    /// Write a 16-channel poly `value` to `output`. Dispatches on region.
    pub fn write_poly(&mut self, output: &PolyOutput, value: [f32; 16]) {
        self.write_raw(output.cable_idx, CableValue::poly(value));
    }

    #[inline(always)]
    /// Read a stereo `(L, R)` pair from `input`, applying `input.scale`.
    ///
    /// See [`read_mono`](Self::read_mono) for region/slot dispatch.
    ///
    /// When `input.broadcast_from_mono` is set the underlying slot is
    /// produced by a mono source feeding a stereo input (ADR 0059 §2);
    /// lane 0 is replicated as `(s, s)`. Otherwise lanes 0 and 1 are
    /// returned as `(L, R)`.
    pub fn read_stereo(&self, input: &StereoInput) -> StereoSample {
        let apply = |v: f32| {
            let y = v * input.scale + input.offset;
            match input.clip {
                Some((lo, hi)) => y.clamp(lo, hi),
                None => y,
            }
        };
        let slot = self.read_raw(input.cable_idx, input.fused);
        if input.broadcast_from_mono {
            let s = apply(slot.as_mono());
            (s, s)
        } else {
            let (l, r) = slot.as_stereo();
            (apply(l), apply(r))
        }
    }

    #[inline(always)]
    /// Write a stereo `(left, right)` pair to `output`. Dispatches on
    /// region; lanes 2..16 are zeroed.
    pub fn write_stereo(&mut self, output: &StereoOutput, left: f32, right: f32) {
        let mut frame = [0.0_f32; 16];
        frame[0] = left;
        frame[1] = right;
        self.write_raw(output.cable_idx, CableValue::poly(frame));
    }
}

/// Mutable raw-parts view of a [`CablePool`] for FFI plugins. ABI v11.
#[repr(C)]
pub struct CablePoolRawPartsMut {
    pub scratch_ptr: *mut CableValue,
    pub scratch_len: usize,
    pub cycle_ptr: *mut [CableValue; 2],
    pub cycle_len: usize,
    pub wi: usize,
}

/// Const raw-parts view of a [`CablePool`] for FFI plugins'
/// `periodic_update` path. ABI v11.
#[repr(C)]
pub struct CablePoolRawParts {
    pub scratch_ptr: *const CableValue,
    pub scratch_len: usize,
    pub cycle_ptr: *const [CableValue; 2],
    pub cycle_len: usize,
    pub wi: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cables::CYCLE_CAPACITY;

    /// Build a uniform pair pool of `n` cycle slots, all initialised to `value`.
    fn cycle_pool(value: CableValue, n: usize) -> Vec<[CableValue; 2]> {
        vec![[value, value]; n]
    }

    fn empty_scratch() -> Vec<CableValue> {
        Vec::new()
    }

    /// Cycle producer at logical cycle slot `i`.
    const fn cycle_idx(i: usize) -> usize {
        SCRATCH_CAPACITY + i
    }

    #[test]
    fn read_mono_applies_scale() {
        let mut cycle = cycle_pool(CableValue::mono(4.0), 1);
        let mut scratch = empty_scratch();
        let cp = CablePool::new(&mut scratch, &mut cycle, 0);
        let input = MonoInput::scalar(cycle_idx(0), 0.5);
        // wi=0, fused=false → reads [1] which seeds 4.0
        assert_eq!(cp.read_mono(&input), 2.0);
    }

    #[test]
    fn read_poly_applies_scale_to_all_channels() {
        let channels: [f32; 16] = std::array::from_fn(|i| i as f32);
        let mut cycle = cycle_pool(CableValue::poly(channels), 1);
        let mut scratch = empty_scratch();
        let cp = CablePool::new(&mut scratch, &mut cycle, 0);
        let input = PolyInput::scalar(cycle_idx(0), 2.0);
        let result = cp.read_poly(&input);
        for (i, &v) in result.iter().enumerate() {
            assert_eq!(v, i as f32 * 2.0, "channel {i} mismatch");
        }
    }

    #[test]
    fn write_mono_stores_at_write_index() {
        let mut cycle = vec![[CableValue::mono(0.0); 2]];
        let mut scratch = empty_scratch();
        {
            let mut cp = CablePool::new(&mut scratch, &mut cycle, 1);
            let output = MonoOutput { cable_idx: cycle_idx(0), connected: true };
            cp.write_mono(&output, 2.5);
        }
        assert_eq!(cycle[0][1].as_mono(), 2.5);
    }

    #[test]
    fn write_poly_stores_at_write_index() {
        let mut cycle = vec![[CableValue::poly([0.0; 16]); 2]];
        let mut scratch = empty_scratch();
        let data: [f32; 16] = std::array::from_fn(|i| i as f32 * 0.1);
        {
            let mut cp = CablePool::new(&mut scratch, &mut cycle, 0);
            let output = PolyOutput { cable_idx: cycle_idx(0), connected: true };
            cp.write_poly(&output, data);
        }
        assert_eq!(cycle[0][0].as_poly(), data);
    }

    // ── T-0241: Ping-pong 1-sample-delay invariant (cycle region) ───────────

    #[test]
    fn ping_pong_one_sample_delay_mono() {
        let mut cycle = vec![[CableValue::mono(0.0); 2]];
        let mut scratch = empty_scratch();
        let output = MonoOutput { cable_idx: cycle_idx(0), connected: true };
        let input = MonoInput::scalar(cycle_idx(0), 1.0);

        // Tick 0: wi=0, fused=false → reads slot [1] (previous value).
        {
            let mut cp = CablePool::new(&mut scratch, &mut cycle, 0);
            assert_eq!(cp.read_mono(&input), 0.0, "tick 0: read previous");
            cp.write_mono(&output, 42.0);
        }
        // Tick 1: wi=1, fused=false → reads slot [0] (just-written 42.0).
        {
            let cp = CablePool::new(&mut scratch, &mut cycle, 1);
            assert_eq!(cp.read_mono(&input), 42.0, "tick 1: read prior write");
        }
    }

    #[test]
    fn ping_pong_within_tick_isolation() {
        let mut cycle = vec![[CableValue::mono(0.0), CableValue::mono(10.0)]];
        let mut scratch = empty_scratch();
        let output = MonoOutput { cable_idx: cycle_idx(0), connected: true };
        let input = MonoInput::scalar(cycle_idx(0), 1.0);

        // wi=0, fused=false → reads slot [1] = 10.0; ignores this-tick write to [0].
        let mut cp = CablePool::new(&mut scratch, &mut cycle, 0);
        cp.write_mono(&output, 99.0);
        assert_eq!(cp.read_mono(&input), 10.0);
    }

    #[test]
    fn scale_applied_at_read_time() {
        let mut cycle = vec![[CableValue::mono(0.0), CableValue::mono(8.0)]];
        let mut scratch = empty_scratch();
        let input_scaled = MonoInput::scalar(cycle_idx(0), 0.25);

        let cp = CablePool::new(&mut scratch, &mut cycle, 0);
        assert_eq!(cp.read_mono(&input_scaled), 2.0);
    }

    #[test]
    fn poly_scale_one_fast_path_matches_general() {
        let channels: [f32; 16] = std::array::from_fn(|i| (i as f32 + 1.0) * 3.0);
        let mut cycle = vec![[CableValue::poly([0.0; 16]), CableValue::poly(channels)]];
        let mut scratch = empty_scratch();

        let input_unit = PolyInput::scalar(cycle_idx(0), 1.0);
        let input_general = PolyInput::scalar(cycle_idx(0), 1.0000001);

        let cp = CablePool::new(&mut scratch, &mut cycle, 0);
        let result_fast = cp.read_poly(&input_unit);
        let result_general = cp.read_poly(&input_general);

        for i in 0..16 {
            assert!(
                (result_fast[i] - result_general[i]).abs() < 0.01,
                "channel {i}: fast={} general={}", result_fast[i], result_general[i]
            );
            assert_eq!(result_fast[i], channels[i]);
        }
    }

    #[test]
    fn ping_pong_one_sample_delay_poly() {
        let data: [f32; 16] = std::array::from_fn(|i| i as f32);
        let mut cycle = vec![[CableValue::poly([0.0; 16]); 2]];
        let mut scratch = empty_scratch();
        let output = PolyOutput { cable_idx: cycle_idx(0), connected: true };
        let input = PolyInput::scalar(cycle_idx(0), 0.5);

        {
            let mut cp = CablePool::new(&mut scratch, &mut cycle, 0);
            assert_eq!(cp.read_poly(&input), [0.0; 16]);
            cp.write_poly(&output, data);
        }
        {
            let cp = CablePool::new(&mut scratch, &mut cycle, 1);
            let result = cp.read_poly(&input);
            for (i, &v) in result.iter().enumerate() {
                assert_eq!(v, i as f32 * 0.5);
            }
        }
    }

    // ── Scratch region tests (ticket 0850, ADR 0072 phase 5 invert) ─────────

    /// A scratch slot read with `fused=true` returns the same-tick
    /// write — there is no delay.
    #[test]
    fn scratch_read_returns_same_tick_write() {
        let mut cycle = cycle_pool(CableValue::mono(0.0), CYCLE_CAPACITY);
        let scratch_slot = crate::cables::RESERVED_SLOTS;
        let mut scratch = vec![CableValue::mono(0.0); scratch_slot + 1];

        let output = MonoOutput { cable_idx: scratch_slot, connected: true };
        let mut input = MonoInput::scalar(scratch_slot, 1.0);
        input.fused = true;

        let mut cp = CablePool::new(&mut scratch, &mut cycle, 0);
        cp.write_mono(&output, 7.5);
        assert_eq!(cp.read_mono(&input), 7.5);
    }

    /// Scratch slot writes don't disturb the cycle pool, and vice versa.
    #[test]
    fn scratch_and_cycle_isolated() {
        let mut cycle = cycle_pool(CableValue::mono(0.0), CYCLE_CAPACITY);
        let scratch_slot = crate::cables::RESERVED_SLOTS;
        let mut scratch = vec![CableValue::mono(0.0); scratch_slot + 1];
        let cycle_slot = cycle_idx(5);

        {
            let mut cp = CablePool::new(&mut scratch, &mut cycle, 0);
            cp.write_mono(&MonoOutput { cable_idx: scratch_slot, connected: true }, 11.0);
            cp.write_mono(&MonoOutput { cable_idx: cycle_slot, connected: true }, 22.0);
        }
        assert_eq!(scratch[scratch_slot].as_mono(), 11.0);
        assert_eq!(cycle[5][0].as_mono(), 22.0);
        assert_eq!(cycle[5][1].as_mono(), 0.0);
    }
}
