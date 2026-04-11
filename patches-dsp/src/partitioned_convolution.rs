//! Uniform partitioned convolution using overlap-save.
//!
//! The impulse response (IR) is split into fixed-size partitions, each
//! pre-transformed via a 2N-point real FFT. Input arrives in N-sample blocks;
//! a frequency-domain delay line (FDL) accumulates the contribution of each
//! partition for correct linear convolution without circular artifacts.
//!
//! All buffers are pre-allocated at construction time — `process_block` performs
//! zero heap allocations.

use crate::fft::RealPackedFft;

/// Complex multiply-accumulate in CMSIS packed format.
///
/// For each frequency bin, computes `acc[k] += a[k] * b[k]` where multiplication
/// is complex. DC and Nyquist bins (indices 0 and 1) are real-only.
///
/// Uses chunked iteration (4 floats = 2 complex numbers per step) to give
/// LLVM's auto-vectorizer a clean loop body to work with.
///
/// # Panics
///
/// Panics if `acc`, `a`, and `b` do not all have the same length, or if
/// the length is less than 4 or not a multiple of 2.
pub fn complex_multiply_accumulate_packed(acc: &mut [f32], a: &[f32], b: &[f32]) {
    let n = acc.len();
    assert_eq!(n, a.len());
    assert_eq!(n, b.len());
    assert!(n >= 4 && n.is_multiple_of(2));

    // DC (real-only)
    acc[0] += a[0] * b[0];
    // Nyquist (real-only)
    acc[1] += a[1] * b[1];

    // Interior complex bins in chunks of 2 complex numbers (4 floats).
    // Processing pairs lets LLVM emit wider vector ops on ARM NEON / x86 SSE.
    let interior_acc = &mut acc[2..];
    let interior_a = &a[2..];
    let interior_b = &b[2..];

    let chunks_acc = interior_acc.chunks_exact_mut(4);
    let chunks_a = interior_a.chunks_exact(4);
    let chunks_b = interior_b.chunks_exact(4);

    let remainder_start = 2 + chunks_acc.len() * 4;

    for ((c_acc, c_a), c_b) in chunks_acc.zip(chunks_a).zip(chunks_b) {
        let ar0 = c_a[0];
        let ai0 = c_a[1];
        let ar1 = c_a[2];
        let ai1 = c_a[3];
        let br0 = c_b[0];
        let bi0 = c_b[1];
        let br1 = c_b[2];
        let bi1 = c_b[3];
        c_acc[0] += ar0 * br0 - ai0 * bi0;
        c_acc[1] += ar0 * bi0 + ai0 * br0;
        c_acc[2] += ar1 * br1 - ai1 * bi1;
        c_acc[3] += ar1 * bi1 + ai1 * br1;
    }

    // Handle a trailing odd complex number if the interior bin count is odd.
    if remainder_start < n {
        let ar = a[remainder_start];
        let ai = a[remainder_start + 1];
        let br = b[remainder_start];
        let bi = b[remainder_start + 1];
        acc[remainder_start] += ar * br - ai * bi;
        acc[remainder_start + 1] += ar * bi + ai * br;
    }
}

/// Complex multiply in CMSIS packed format (non-accumulating).
///
/// Computes `out[k] = a[k] * b[k]` for each frequency bin.
pub fn complex_multiply_packed(out: &mut [f32], a: &[f32], b: &[f32]) {
    let n = out.len();
    assert_eq!(n, a.len());
    assert_eq!(n, b.len());
    assert!(n >= 4 && n.is_multiple_of(2));

    out[0] = a[0] * b[0];
    out[1] = a[1] * b[1];

    let interior_out = &mut out[2..];
    let interior_a = &a[2..];
    let interior_b = &b[2..];

    let chunks_out = interior_out.chunks_exact_mut(4);
    let chunks_a = interior_a.chunks_exact(4);
    let chunks_b = interior_b.chunks_exact(4);

    let remainder_start = 2 + chunks_out.len() * 4;

    for ((c_out, c_a), c_b) in chunks_out.zip(chunks_a).zip(chunks_b) {
        let ar0 = c_a[0];
        let ai0 = c_a[1];
        let ar1 = c_a[2];
        let ai1 = c_a[3];
        let br0 = c_b[0];
        let bi0 = c_b[1];
        let br1 = c_b[2];
        let bi1 = c_b[3];
        c_out[0] = ar0 * br0 - ai0 * bi0;
        c_out[1] = ar0 * bi0 + ai0 * br0;
        c_out[2] = ar1 * br1 - ai1 * bi1;
        c_out[3] = ar1 * bi1 + ai1 * br1;
    }

    if remainder_start < n {
        let ar = a[remainder_start];
        let ai = a[remainder_start + 1];
        let br = b[remainder_start];
        let bi = b[remainder_start + 1];
        out[remainder_start] = ar * br - ai * bi;
        out[remainder_start + 1] = ar * bi + ai * br;
    }
}

/// Pre-FFT'd impulse response partitions.
pub struct IrPartitions {
    /// Each partition is a 2N-length packed spectrum.
    partitions: Vec<Box<[f32]>>,
    block_size: usize,
    fft_size: usize,
    num_partitions: usize,
}

impl IrPartitions {
    /// Partition an impulse response and pre-compute the FFT of each partition.
    ///
    /// `block_size` must be a power of 2 and >= 2 (so that `2 * block_size >= 4`
    /// for `RealPackedFft`). The IR is zero-padded to a multiple of `block_size`.
    ///
    /// # Panics
    ///
    /// Panics if `block_size` is not a power of 2 or is less than 2, or if `ir`
    /// is empty.
    pub fn from_ir(ir: &[f32], block_size: usize) -> Self {
        assert!(block_size >= 2 && block_size.is_power_of_two());
        assert!(!ir.is_empty());

        let fft_size = 2 * block_size;
        let fft = RealPackedFft::new(fft_size);
        let num_partitions = ir.len().div_ceil(block_size);

        let mut partitions = Vec::with_capacity(num_partitions);
        for i in 0..num_partitions {
            let start = i * block_size;
            let end = (start + block_size).min(ir.len());
            let mut buf = vec![0.0f32; fft_size];
            buf[..end - start].copy_from_slice(&ir[start..end]);
            // Remaining samples are already zero (zero-pad to 2N).
            fft.forward(&mut buf);
            partitions.push(buf.into_boxed_slice());
        }

        Self {
            partitions,
            block_size,
            fft_size,
            num_partitions,
        }
    }

    /// Create `IrPartitions` from pre-computed frequency-domain partition data.
    ///
    /// `partitions` must contain frequency-domain spectra of length `2 * block_size`
    /// in CMSIS packed format (as produced by [`RealPackedFft::forward`]).
    ///
    /// This skips the forward FFT step, allowing the caller to pre-compute
    /// spectral data (e.g. during file processing on the control thread).
    pub fn from_packed(partitions: Vec<Box<[f32]>>, block_size: usize) -> Self {
        let fft_size = 2 * block_size;
        let num_partitions = partitions.len();
        Self { partitions, block_size, fft_size, num_partitions }
    }

    /// Number of IR partitions.
    pub fn num_partitions(&self) -> usize {
        self.num_partitions
    }

    /// Block size (N).
    pub fn block_size(&self) -> usize {
        self.block_size
    }

    /// FFT size (2N).
    pub fn fft_size(&self) -> usize {
        self.fft_size
    }
}

/// Uniform partitioned convolver using overlap-save.
///
/// Call `process_block` once per N-sample input block. All internal buffers are
/// pre-allocated; the hot path performs zero heap allocations.
pub struct PartitionedConvolver {
    ir: IrPartitions,
    fft: RealPackedFft,
    /// Circular buffer of frequency-domain input spectra.
    fdl: Vec<Box<[f32]>>,
    /// Current write position in the FDL (advances by 1 per block).
    fdl_pos: usize,
    /// Time-domain input history: `[prev_N | current_N]`, length 2N.
    input_history: Box<[f32]>,
    /// Frequency-domain accumulator, length 2N.
    accumulator: Box<[f32]>,
    /// Time-domain scratch for IFFT output, length 2N.
    output_buf: Box<[f32]>,
}

impl PartitionedConvolver {
    /// Create a new convolver for the given pre-partitioned IR.
    pub fn new(ir: IrPartitions) -> Self {
        let fft_size = ir.fft_size();
        let fft = RealPackedFft::new(fft_size);
        let num_partitions = ir.num_partitions();

        let fdl: Vec<Box<[f32]>> = (0..num_partitions)
            .map(|_| vec![0.0f32; fft_size].into_boxed_slice())
            .collect();

        Self {
            ir,
            fft,
            fdl,
            fdl_pos: 0,
            input_history: vec![0.0f32; fft_size].into_boxed_slice(),
            accumulator: vec![0.0f32; fft_size].into_boxed_slice(),
            output_buf: vec![0.0f32; fft_size].into_boxed_slice(),
        }
    }

    /// Process one block of N input samples, writing N output samples.
    ///
    /// # Panics
    ///
    /// Panics if `input.len()` or `output.len()` does not equal `block_size`.
    pub fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        let n = self.ir.block_size();
        let fft_size = self.ir.fft_size();
        assert_eq!(input.len(), n);
        assert_eq!(output.len(), n);

        // 1. Update input history: shift left by N, write new block into right half.
        self.input_history.copy_within(n..fft_size, 0);
        self.input_history[n..].copy_from_slice(input);

        // 2. FFT the 2N input history into the current FDL slot.
        let fdl_slot = &mut self.fdl[self.fdl_pos];
        fdl_slot.copy_from_slice(&self.input_history);
        self.fft.forward(fdl_slot);

        // 3. Accumulate: for each partition i, multiply FDL[(pos - i) mod B] * H[i].
        //    First partition uses non-accumulating multiply (avoids zeroing pass);
        //    remaining partitions accumulate into the result.
        let num_p = self.ir.num_partitions();
        {
            let fdl_idx = self.fdl_pos % num_p;
            complex_multiply_packed(
                &mut self.accumulator,
                &self.fdl[fdl_idx],
                &self.ir.partitions[0],
            );
        }
        for i in 1..num_p {
            let fdl_idx = (self.fdl_pos + num_p - i) % num_p;
            complex_multiply_accumulate_packed(
                &mut self.accumulator,
                &self.fdl[fdl_idx],
                &self.ir.partitions[i],
            );
        }

        // 4. IFFT the accumulator.
        self.output_buf.copy_from_slice(&self.accumulator);
        self.fft.inverse(&mut self.output_buf);

        // 5. Take the last N samples (overlap-save).
        output.copy_from_slice(&self.output_buf[n..]);

        // 6. Advance FDL position.
        self.fdl_pos = (self.fdl_pos + 1) % num_p;
    }

    /// Clear all internal state (FDL, input history). The next block will
    /// process as if the convolver were freshly constructed.
    pub fn reset(&mut self) {
        for slot in &mut self.fdl {
            slot.fill(0.0);
        }
        self.fdl_pos = 0;
        self.input_history.fill(0.0);
        self.accumulator.fill(0.0);
        self.output_buf.fill(0.0);
    }

    /// The block size (N) this convolver expects.
    pub fn block_size(&self) -> usize {
        self.ir.block_size()
    }
}

// ---------------------------------------------------------------------------
// Non-uniform partitioned convolution
// ---------------------------------------------------------------------------

/// A single tier in a [`NonUniformConvolver`].
///
/// Each tier handles a contiguous segment of the IR at a specific block size.
/// Tier 0 uses the base block size; subsequent tiers double. The tier
/// accumulates base-block-sized input chunks in `input_ring` until a full
/// tier block is ready, then runs its inner `PartitionedConvolver`.
struct ConvolutionTier {
    convolver: PartitionedConvolver,
    /// This tier's block size (power of two, ≥ base block size).
    tier_block_size: usize,
    /// Ratio of this tier's block size to the base block size.
    ratio: usize,
    /// Ring buffer accumulating input samples (length = tier_block_size).
    input_ring: Box<[f32]>,
    /// Current write position in `input_ring` (0..tier_block_size).
    input_pos: usize,
    /// Ring buffer holding this tier's output contribution (length = tier_block_size).
    output_ring: Box<[f32]>,
    /// Current read position in `output_ring` (advances by base_block_size per call).
    output_read_pos: usize,
}

impl ConvolutionTier {
    fn new(ir_segment: &[f32], tier_block_size: usize) -> Self {
        let parts = IrPartitions::from_ir(ir_segment, tier_block_size);
        let convolver = PartitionedConvolver::new(parts);
        let ratio = 1; // set by caller
        Self {
            convolver,
            tier_block_size,
            ratio,
            input_ring: vec![0.0f32; tier_block_size].into_boxed_slice(),
            input_pos: 0,
            output_ring: vec![0.0f32; tier_block_size].into_boxed_slice(),
            output_read_pos: 0,
        }
    }

    fn reset(&mut self) {
        self.convolver.reset();
        self.input_ring.fill(0.0);
        self.input_pos = 0;
        self.output_ring.fill(0.0);
        self.output_read_pos = 0;
    }
}

/// Non-uniform partitioned convolver.
///
/// Splits the impulse response into geometrically-growing tiers. Tier 0 uses
/// `base_block_size`; each subsequent tier doubles, up to `max_tier_block_size`.
/// Larger tiers process less frequently, reducing total work from O(P × N) to
/// approximately O(N × log P).
///
/// All buffers are pre-allocated at construction. `process_block` performs zero
/// heap allocations.
///
/// # Usage
///
/// Call [`process_block`](Self::process_block) once per `base_block_size` input
/// samples, exactly as with [`PartitionedConvolver`].
pub struct NonUniformConvolver {
    tiers: Vec<ConvolutionTier>,
    base_block_size: usize,
    /// Scratch buffer for tier convolver output (length = max tier block size).
    tier_output_scratch: Box<[f32]>,
}

impl NonUniformConvolver {
    /// Create a non-uniform convolver for the given IR.
    ///
    /// - `base_block_size`: input block size (power of 2, ≥ 2). This is the
    ///   granularity at which audio arrives.
    /// - `max_tier_block_size`: largest tier block size (power of 2, ≥
    ///   `base_block_size`). Tiers double from `base_block_size` up to this cap.
    ///
    /// # Panics
    ///
    /// Panics if `base_block_size` or `max_tier_block_size` is not a power of 2,
    /// if `max_tier_block_size < base_block_size`, or if `ir` is empty.
    pub fn new(ir: &[f32], base_block_size: usize, max_tier_block_size: usize) -> Self {
        assert!(base_block_size >= 2 && base_block_size.is_power_of_two());
        assert!(max_tier_block_size >= base_block_size && max_tier_block_size.is_power_of_two());
        assert!(!ir.is_empty());

        let mut tiers = Vec::new();
        let mut ir_offset = 0;
        let mut tier_block = base_block_size;

        while ir_offset < ir.len() {
            if tier_block < max_tier_block_size {
                // This tier covers one block's worth of IR at the current size.
                let end = (ir_offset + tier_block).min(ir.len());
                let segment = &ir[ir_offset..end];
                let mut tier = ConvolutionTier::new(segment, tier_block);
                tier.ratio = tier_block / base_block_size;
                tiers.push(tier);
                ir_offset = end;
                tier_block *= 2;
            } else {
                // Final tier: all remaining IR at max_tier_block_size.
                let segment = &ir[ir_offset..];
                let mut tier = ConvolutionTier::new(segment, max_tier_block_size);
                tier.ratio = max_tier_block_size / base_block_size;
                tiers.push(tier);
                break;
            }
        }

        let max_tier = tiers.iter().map(|t| t.tier_block_size).max().unwrap_or(base_block_size);
        Self {
            tiers,
            base_block_size,
            tier_output_scratch: vec![0.0f32; max_tier].into_boxed_slice(),
        }
    }

    /// Process one block of `base_block_size` input samples.
    ///
    /// # Panics
    ///
    /// Panics if `input.len()` or `output.len()` does not equal `base_block_size`.
    pub fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        let n = self.base_block_size;
        assert_eq!(input.len(), n);
        assert_eq!(output.len(), n);

        output.fill(0.0);

        for tier in &mut self.tiers {
            // Accumulate input into this tier's ring.
            tier.input_ring[tier.input_pos..tier.input_pos + n].copy_from_slice(input);
            tier.input_pos += n;

            // If the input ring is full, process a tier block.
            if tier.input_pos >= tier.tier_block_size {
                tier.input_pos = 0;
                let scratch = &mut self.tier_output_scratch[..tier.tier_block_size];
                tier.convolver.process_block(&tier.input_ring, scratch);
                // Copy result into the tier's output ring for reading.
                tier.output_ring.copy_from_slice(scratch);
                tier.output_read_pos = 0;
            }

            // Add this tier's contribution to output.
            let read = tier.output_read_pos;
            for (out, &tier_val) in output.iter_mut().zip(&tier.output_ring[read..read + n]) {
                *out += tier_val;
            }
            tier.output_read_pos += n;
        }
    }

    /// Serialize the pre-computed frequency-domain data into a flat `Vec<f32>`.
    ///
    /// The format is a private contract between `process_file` and
    /// `from_pre_fft`. Layout:
    ///
    /// ```text
    /// [tier_count, base_block_size]
    /// Per tier: [tier_block_size, partition_count, ratio, <partition_data...>]
    /// ```
    ///
    /// All header values are stored as `f32` (they are small integers).
    pub fn serialize_pre_fft(ir: &[f32], base_block_size: usize, max_tier_block_size: usize) -> Vec<f32> {
        // Build the convolver to partition the IR.
        let temp = Self::new(ir, base_block_size, max_tier_block_size);
        let mut data = Vec::new();
        data.push(temp.tiers.len() as f32);
        data.push(base_block_size as f32);
        for tier in &temp.tiers {
            data.push(tier.tier_block_size as f32);
            data.push(tier.convolver.ir.num_partitions() as f32);
            data.push(tier.ratio as f32);
            for part in &tier.convolver.ir.partitions {
                data.extend_from_slice(part);
            }
        }
        data
    }

    /// Reconstruct a `NonUniformConvolver` from pre-FFT'd data produced by
    /// [`serialize_pre_fft`](Self::serialize_pre_fft).
    ///
    /// # Panics
    ///
    /// Panics if `data` does not contain a valid serialized convolver.
    pub fn from_pre_fft(data: &[f32]) -> Self {
        let tier_count = data[0] as usize;
        let base_block_size = data[1] as usize;
        let mut offset = 2;
        let mut tiers = Vec::with_capacity(tier_count);

        for _ in 0..tier_count {
            let tier_block_size = data[offset] as usize;
            let partition_count = data[offset + 1] as usize;
            let ratio = data[offset + 2] as usize;
            offset += 3;

            let fft_size = 2 * tier_block_size;
            let mut partitions = Vec::with_capacity(partition_count);
            for _ in 0..partition_count {
                let part = data[offset..offset + fft_size].to_vec().into_boxed_slice();
                partitions.push(part);
                offset += fft_size;
            }

            let ir_parts = IrPartitions::from_packed(partitions, tier_block_size);
            let convolver = PartitionedConvolver::new(ir_parts);

            tiers.push(ConvolutionTier {
                convolver,
                tier_block_size,
                ratio,
                input_ring: vec![0.0f32; tier_block_size].into_boxed_slice(),
                input_pos: 0,
                output_ring: vec![0.0f32; tier_block_size].into_boxed_slice(),
                output_read_pos: 0,
            });
        }

        let max_tier = tiers.iter().map(|t| t.tier_block_size).max().unwrap_or(base_block_size);
        Self {
            tiers,
            base_block_size,
            tier_output_scratch: vec![0.0f32; max_tier].into_boxed_slice(),
        }
    }

    /// Clear all internal state. The next block processes as if freshly constructed.
    pub fn reset(&mut self) {
        for tier in &mut self.tiers {
            tier.reset();
        }
    }

    /// The base block size this convolver expects.
    pub fn block_size(&self) -> usize {
        self.base_block_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- complex_multiply_accumulate_packed tests ---

    #[test]
    fn cma_dc_and_nyquist() {
        let a = [2.0, 3.0, 0.0, 0.0];
        let b = [4.0, 5.0, 0.0, 0.0];
        let mut acc = [0.0; 4];
        complex_multiply_accumulate_packed(&mut acc, &a, &b);
        assert_eq!(acc[0], 8.0); // 2*4
        assert_eq!(acc[1], 15.0); // 3*5
    }

    #[test]
    fn cma_interior_bins() {
        // Single interior bin: a = 3+4i, b = 1+2i => (3+4i)(1+2i) = -5+10i
        let a = [0.0, 0.0, 3.0, 4.0];
        let b = [0.0, 0.0, 1.0, 2.0];
        let mut acc = [0.0; 4];
        complex_multiply_accumulate_packed(&mut acc, &a, &b);
        assert!((acc[2] - (-5.0)).abs() < 1e-6);
        assert!((acc[3] - 10.0).abs() < 1e-6);
    }

    #[test]
    fn cma_accumulates() {
        let a = [1.0, 1.0, 1.0, 0.0];
        let b = [2.0, 3.0, 2.0, 0.0];
        let mut acc = [10.0, 20.0, 30.0, 40.0];
        complex_multiply_accumulate_packed(&mut acc, &a, &b);
        assert_eq!(acc[0], 12.0); // 10 + 1*2
        assert_eq!(acc[1], 23.0); // 20 + 1*3
        assert_eq!(acc[2], 32.0); // 30 + 1*2 - 0*0
        assert_eq!(acc[3], 40.0); // 40 + 1*0 + 0*2
    }

    #[test]
    fn complex_multiply_packed_basic() {
        let a = [2.0, 3.0, 1.0, 2.0, 3.0, 4.0];
        let b = [4.0, 5.0, 5.0, 6.0, 1.0, 2.0];
        let mut out = [0.0; 6];
        complex_multiply_packed(&mut out, &a, &b);
        assert_eq!(out[0], 8.0);
        assert_eq!(out[1], 15.0);
        // (1+2i)(5+6i) = 5+6i+10i-12 = -7+16i
        assert!((out[2] - (-7.0)).abs() < 1e-6);
        assert!((out[3] - 16.0).abs() < 1e-6);
        // (3+4i)(1+2i) = 3+6i+4i-8 = -5+10i
        assert!((out[4] - (-5.0)).abs() < 1e-6);
        assert!((out[5] - 10.0).abs() < 1e-6);
    }

    // --- IrPartitions tests ---

    #[test]
    fn ir_partitions_count() {
        let ir = vec![1.0; 100];
        let parts = IrPartitions::from_ir(&ir, 32);
        // ceil(100/32) = 4
        assert_eq!(parts.num_partitions(), 4);
        assert_eq!(parts.block_size(), 32);
        assert_eq!(parts.fft_size(), 64);
    }

    #[test]
    fn ir_partitions_exact_fit() {
        let ir = vec![1.0; 64];
        let parts = IrPartitions::from_ir(&ir, 32);
        assert_eq!(parts.num_partitions(), 2);
    }

    #[test]
    fn ir_partition_roundtrip() {
        // Verify that IFFT of each partition recovers the original IR segment.
        let ir: Vec<f32> = (0..48).map(|i| (i as f32) * 0.1).collect();
        let block_size = 16;
        let fft_size = 32;
        let parts = IrPartitions::from_ir(&ir, block_size);
        let fft = RealPackedFft::new(fft_size);

        for i in 0..parts.num_partitions() {
            let mut buf = parts.partitions[i].to_vec();
            fft.inverse(&mut buf);
            let start = i * block_size;
            let end = (start + block_size).min(ir.len());
            for j in 0..block_size {
                let expected = if start + j < end { ir[start + j] } else { 0.0 };
                assert!(
                    (buf[j] - expected).abs() < 1e-3,
                    "partition {i} sample {j}: got {} expected {expected}",
                    buf[j],
                );
            }
        }
    }

    // --- PartitionedConvolver tests ---

    /// Naive time-domain convolution for reference.
    fn naive_convolve(signal: &[f32], ir: &[f32]) -> Vec<f32> {
        let out_len = signal.len() + ir.len() - 1;
        let mut out = vec![0.0f32; out_len];
        for (i, &s) in signal.iter().enumerate() {
            for (j, &h) in ir.iter().enumerate() {
                out[i + j] += s * h;
            }
        }
        out
    }

    #[test]
    fn identity_convolution() {
        // Convolving with [1, 0, 0, ...] should reproduce the input.
        let block_size = 16;
        let mut ir = vec![0.0f32; block_size];
        ir[0] = 1.0;
        let parts = IrPartitions::from_ir(&ir, block_size);
        let mut conv = PartitionedConvolver::new(parts);

        let input: Vec<f32> = (0..block_size).map(|i| (i as f32) * 0.1).collect();
        let mut output = vec![0.0f32; block_size];

        // First block: input history is [zeros | input], output should be the input.
        conv.process_block(&input, &mut output);
        // Measured ~0 error on aarch64 macOS debug (2026-04-02). Tightened from 1e-3.
        for i in 0..block_size {
            assert!(
                (output[i] - input[i]).abs() < 1e-5,
                "sample {i}: got {} expected {}",
                output[i],
                input[i],
            );
        }
    }

    #[test]
    fn delayed_impulse_convolution() {
        // IR = [0, 0, ..., 0, 1] with the 1 at position `delay`.
        let block_size = 16;
        let delay = 5;
        let mut ir = vec![0.0f32; block_size];
        ir[delay] = 1.0;
        let parts = IrPartitions::from_ir(&ir, block_size);
        let mut conv = PartitionedConvolver::new(parts);

        // Send several blocks and concatenate output.
        let num_blocks = 4;
        let signal: Vec<f32> = (0..block_size * num_blocks)
            .map(|i| if i < block_size { i as f32 + 1.0 } else { 0.0 })
            .collect();
        let mut output = vec![0.0f32; block_size * num_blocks];

        for b in 0..num_blocks {
            let s = b * block_size;
            conv.process_block(&signal[s..s + block_size], &mut output[s..s + block_size]);
        }

        // Output should be the input delayed by `delay` samples.
        let expected = naive_convolve(&signal, &ir);
        for i in 0..output.len() {
            assert!(
                (output[i] - expected[i]).abs() < 1e-2,
                "sample {i}: got {} expected {}",
                output[i],
                expected[i],
            );
        }
    }

    #[test]
    fn multi_partition_matches_naive() {
        // IR spans 3 partitions.
        let block_size = 16;
        let ir: Vec<f32> = (0..block_size * 3)
            .map(|i| 1.0 / (1.0 + i as f32))
            .collect();
        let parts = IrPartitions::from_ir(&ir, block_size);
        assert_eq!(parts.num_partitions(), 3);
        let mut conv = PartitionedConvolver::new(parts);

        let num_blocks = 8;
        let signal: Vec<f32> = (0..block_size * num_blocks)
            .map(|i| ((i as f32) * 0.37).sin())
            .collect();
        let mut output = vec![0.0f32; signal.len()];

        for b in 0..num_blocks {
            let s = b * block_size;
            conv.process_block(&signal[s..s + block_size], &mut output[s..s + block_size]);
        }

        let expected = naive_convolve(&signal, &ir);
        // Compare only the first signal.len() samples (the tail extends beyond).
        let mut max_err = 0.0f32;
        for i in 0..output.len() {
            let err = (output[i] - expected[i]).abs();
            if err > max_err {
                max_err = err;
            }
        }
        // Measured 1e-6 on aarch64 macOS debug (2026-04-02). Tightened from 0.05.
        assert!(
            max_err < 1e-4,
            "max error {max_err} exceeds tolerance; multi-partition output diverges from naive",
        );
    }

    #[test]
    fn block_boundary_continuity() {
        // Process a continuous signal and check there are no discontinuities
        // at block boundaries.
        let block_size = 32;
        let ir: Vec<f32> = (0..64).map(|i| (-0.01 * i as f32).exp()).collect();
        let parts = IrPartitions::from_ir(&ir, block_size);
        let mut conv = PartitionedConvolver::new(parts);

        let num_blocks = 10;
        let signal: Vec<f32> = (0..block_size * num_blocks)
            .map(|i| ((i as f32) * 0.1).sin())
            .collect();
        let mut output = vec![0.0f32; signal.len()];

        for b in 0..num_blocks {
            let s = b * block_size;
            conv.process_block(&signal[s..s + block_size], &mut output[s..s + block_size]);
        }

        // Check continuity: difference between adjacent samples should be small.
        // For a smooth input convolved with a smooth IR, max sample-to-sample
        // difference should be bounded.
        let expected = naive_convolve(&signal, &ir);
        for b in 1..num_blocks {
            let boundary = b * block_size;
            let err = (output[boundary] - expected[boundary]).abs();
            assert!(
                err < 0.05,
                "discontinuity at block boundary {boundary}: got {} expected {}, err {err}",
                output[boundary],
                expected[boundary],
            );
        }
    }

    #[test]
    fn reset_clears_state() {
        let block_size = 16;
        let ir: Vec<f32> = (0..32).map(|i| 1.0 / (1.0 + i as f32)).collect();
        let parts = IrPartitions::from_ir(&ir, block_size);
        let mut conv = PartitionedConvolver::new(parts);

        let input: Vec<f32> = (0..block_size).map(|i| (i as f32) * 0.1).collect();
        let mut out1 = vec![0.0f32; block_size];
        let mut out2 = vec![0.0f32; block_size];

        // Process one block.
        conv.process_block(&input, &mut out1);
        // Reset and process the same block again.
        conv.reset();
        conv.process_block(&input, &mut out2);

        for i in 0..block_size {
            assert!(
                (out1[i] - out2[i]).abs() < 1e-6,
                "sample {i}: first pass {} != second pass after reset {}",
                out1[i],
                out2[i],
            );
        }
    }

    #[test]
    fn single_sample_ir() {
        // Edge case: IR is a single sample (scalar multiplication).
        let block_size = 8;
        let ir = vec![0.5f32];
        let parts = IrPartitions::from_ir(&ir, block_size);
        let mut conv = PartitionedConvolver::new(parts);

        let input = vec![2.0f32; block_size];
        let mut output = vec![0.0f32; block_size];
        conv.process_block(&input, &mut output);

        for i in 0..block_size {
            assert!(
                (output[i] - 1.0).abs() < 1e-3,
                "sample {i}: got {} expected 1.0",
                output[i],
            );
        }
    }

    // --- NonUniformConvolver tests ---

    #[test]
    fn nu_identity_convolution() {
        let block_size = 16;
        let mut ir = vec![0.0f32; block_size];
        ir[0] = 1.0;
        let mut conv = NonUniformConvolver::new(&ir, block_size, block_size);

        let input: Vec<f32> = (0..block_size).map(|i| (i as f32) * 0.1).collect();
        let mut output = vec![0.0f32; block_size];

        conv.process_block(&input, &mut output);
        for i in 0..block_size {
            assert!(
                (output[i] - input[i]).abs() < 1e-3,
                "sample {i}: got {} expected {}",
                output[i],
                input[i],
            );
        }
    }

    #[test]
    fn nu_delayed_impulse() {
        let block_size = 16;
        let delay = 5;
        let mut ir = vec![0.0f32; block_size];
        ir[delay] = 1.0;
        let mut conv = NonUniformConvolver::new(&ir, block_size, block_size);

        let num_blocks = 4;
        let signal: Vec<f32> = (0..block_size * num_blocks)
            .map(|i| if i < block_size { i as f32 + 1.0 } else { 0.0 })
            .collect();
        let mut output = vec![0.0f32; block_size * num_blocks];

        for b in 0..num_blocks {
            let s = b * block_size;
            conv.process_block(&signal[s..s + block_size], &mut output[s..s + block_size]);
        }

        let expected = naive_convolve(&signal, &ir);
        for i in 0..output.len() {
            assert!(
                (output[i] - expected[i]).abs() < 1e-2,
                "sample {i}: got {} expected {}",
                output[i],
                expected[i],
            );
        }
    }

    #[test]
    fn nu_multi_tier_matches_naive() {
        // IR long enough to span multiple tiers (base=16, max=64).
        // Tier 0: block=16, covers IR[0..16]
        // Tier 1: block=32, covers IR[16..48]
        // Tier 2: block=64, covers IR[48..end]
        let block_size = 16;
        let ir: Vec<f32> = (0..200).map(|i| 1.0 / (1.0 + i as f32)).collect();
        let mut conv = NonUniformConvolver::new(&ir, block_size, 64);

        let num_blocks = 30;
        let signal: Vec<f32> = (0..block_size * num_blocks)
            .map(|i| ((i as f32) * 0.37).sin())
            .collect();
        let mut output = vec![0.0f32; signal.len()];

        for b in 0..num_blocks {
            let s = b * block_size;
            conv.process_block(&signal[s..s + block_size], &mut output[s..s + block_size]);
        }

        let expected = naive_convolve(&signal, &ir);
        let mut max_err = 0.0f32;
        for i in 0..output.len() {
            let err = (output[i] - expected[i]).abs();
            if err > max_err {
                max_err = err;
            }
        }
        assert!(
            max_err < 0.1,
            "max error {max_err} exceeds tolerance; non-uniform output diverges from naive",
        );
    }

    #[test]
    fn nu_matches_uniform() {
        // Compare non-uniform against uniform convolver for the same IR.
        let block_size = 16;
        let ir: Vec<f32> = (0..128).map(|i| (-0.02 * i as f32).exp()).collect();

        let parts = IrPartitions::from_ir(&ir, block_size);
        let mut uniform = PartitionedConvolver::new(parts);
        let mut non_uniform = NonUniformConvolver::new(&ir, block_size, 64);

        let num_blocks = 20;
        let signal: Vec<f32> = (0..block_size * num_blocks)
            .map(|i| ((i as f32) * 0.23).sin())
            .collect();
        let mut out_u = vec![0.0f32; signal.len()];
        let mut out_nu = vec![0.0f32; signal.len()];

        for b in 0..num_blocks {
            let s = b * block_size;
            uniform.process_block(&signal[s..s + block_size], &mut out_u[s..s + block_size]);
            non_uniform.process_block(&signal[s..s + block_size], &mut out_nu[s..s + block_size]);
        }

        let mut max_err = 0.0f32;
        for i in 0..out_u.len() {
            let err = (out_u[i] - out_nu[i]).abs();
            if err > max_err {
                max_err = err;
            }
        }
        assert!(
            max_err < 0.05,
            "max error {max_err} between uniform and non-uniform convolver",
        );
    }

    #[test]
    fn nu_reset_clears_state() {
        let block_size = 16;
        let ir: Vec<f32> = (0..64).map(|i| 1.0 / (1.0 + i as f32)).collect();
        let mut conv = NonUniformConvolver::new(&ir, block_size, 32);

        let input: Vec<f32> = (0..block_size).map(|i| (i as f32) * 0.1).collect();
        let mut out1 = vec![0.0f32; block_size];
        let mut out2 = vec![0.0f32; block_size];

        conv.process_block(&input, &mut out1);
        conv.reset();
        conv.process_block(&input, &mut out2);

        for i in 0..block_size {
            assert!(
                (out1[i] - out2[i]).abs() < 1e-6,
                "sample {i}: first pass {} != second pass after reset {}",
                out1[i],
                out2[i],
            );
        }
    }

    #[test]
    fn nu_single_sample_ir() {
        let block_size = 8;
        let ir = vec![0.5f32];
        let mut conv = NonUniformConvolver::new(&ir, block_size, 32);

        let input = vec![2.0f32; block_size];
        let mut output = vec![0.0f32; block_size];
        conv.process_block(&input, &mut output);

        for i in 0..block_size {
            assert!(
                (output[i] - 1.0).abs() < 1e-3,
                "sample {i}: got {} expected 1.0",
                output[i],
            );
        }
    }

    #[test]
    fn nu_tier_count() {
        // IR of 200 samples, base=16, max=64:
        // Tier 0: block=16 → IR[0..16]
        // Tier 1: block=32 → IR[16..48]
        // Tier 2: block=64 → IR[48..200] (final tier, multiple partitions)
        let ir = vec![1.0f32; 200];
        let conv = NonUniformConvolver::new(&ir, 16, 64);
        assert_eq!(conv.tiers.len(), 3);
        assert_eq!(conv.tiers[0].tier_block_size, 16);
        assert_eq!(conv.tiers[1].tier_block_size, 32);
        assert_eq!(conv.tiers[2].tier_block_size, 64);
    }

    // ── T-0240: Latency test ────────────────────────────────────────────────

    /// Verify that the first non-zero output appears at the expected sample offset.
    /// With an identity IR [1, 0, 0, ...], the convolver has zero algorithmic
    /// latency — the first output sample should be non-zero when the first
    /// input sample is non-zero.
    #[test]
    fn latency_first_nonzero_output() {
        let block_size = 16;
        let mut ir = vec![0.0f32; block_size];
        ir[0] = 1.0;
        let parts = IrPartitions::from_ir(&ir, block_size);
        let mut conv = PartitionedConvolver::new(parts);

        let mut input = vec![0.0f32; block_size];
        input[0] = 1.0; // impulse at sample 0
        let mut output = vec![0.0f32; block_size];

        conv.process_block(&input, &mut output);

        // First non-zero output should be at sample 0 (no added latency).
        assert!(
            output[0].abs() > 0.5,
            "expected non-zero output at sample 0, got {}",
            output[0]
        );
    }

    /// With a delayed IR [0, ..., 0, 1] (delay = d), the first non-zero output
    /// should appear at sample d.
    #[test]
    fn latency_delayed_ir_offset() {
        let block_size = 16;
        let delay = 7;
        let mut ir = vec![0.0f32; block_size];
        ir[delay] = 1.0;
        let parts = IrPartitions::from_ir(&ir, block_size);
        let mut conv = PartitionedConvolver::new(parts);

        // Send an impulse at sample 0 across multiple blocks.
        let num_blocks = 4;
        let mut all_output = Vec::new();
        for b in 0..num_blocks {
            let mut input = vec![0.0f32; block_size];
            if b == 0 {
                input[0] = 1.0;
            }
            let mut output = vec![0.0f32; block_size];
            conv.process_block(&input, &mut output);
            all_output.extend_from_slice(&output);
        }

        // Samples before `delay` should be zero.
        for i in 0..delay {
            assert!(
                all_output[i].abs() < 1e-6,
                "expected zero at sample {i}, got {}",
                all_output[i]
            );
        }
        // Sample at `delay` should be non-zero.
        assert!(
            all_output[delay].abs() > 0.5,
            "expected non-zero at sample {delay}, got {}",
            all_output[delay]
        );
    }

    // ── Exact latency assertions (T-0261) ─────────────────────────────────

    /// PartitionedConvolver with identity IR: first output sample equals first
    /// input sample (zero algorithmic latency).
    #[test]
    fn partitioned_exact_latency_identity_ir() {
        let block_size = 32;
        let mut ir = vec![0.0f32; block_size];
        ir[0] = 1.0;
        let parts = IrPartitions::from_ir(&ir, block_size);
        let mut conv = PartitionedConvolver::new(parts);

        let input: Vec<f32> = (0..block_size).map(|i| (i as f32 + 1.0) * 0.1).collect();
        let mut output = vec![0.0f32; block_size];
        conv.process_block(&input, &mut output);

        // Each output sample should match the corresponding input sample
        for i in 0..block_size {
            assert!(
                (output[i] - input[i]).abs() < 1e-3,
                "sample {i}: expected {}, got {} (identity IR should have zero latency)",
                input[i], output[i]
            );
        }
    }

    /// PartitionedConvolver with delayed impulse IR: first non-zero output
    /// appears at exactly sample index D.
    #[test]
    fn partitioned_exact_latency_delayed_impulse() {
        let block_size = 32;
        let delay = 11;
        let mut ir = vec![0.0f32; block_size];
        ir[delay] = 1.0;
        let parts = IrPartitions::from_ir(&ir, block_size);
        let mut conv = PartitionedConvolver::new(parts);

        // Send an impulse at sample 0
        let num_blocks = 4;
        let mut all_output = Vec::new();
        for b in 0..num_blocks {
            let mut input = vec![0.0f32; block_size];
            if b == 0 { input[0] = 1.0; }
            let mut output = vec![0.0f32; block_size];
            conv.process_block(&input, &mut output);
            all_output.extend_from_slice(&output);
        }

        // Samples 0..delay must be zero
        for i in 0..delay {
            assert!(
                all_output[i].abs() < 1e-6,
                "sample {i}: expected silence before delay, got {}",
                all_output[i]
            );
        }
        // Sample at exactly `delay` must be the impulse
        assert!(
            (all_output[delay] - 1.0).abs() < 1e-3,
            "sample {delay}: expected 1.0, got {} (delayed impulse should appear at exact offset)",
            all_output[delay]
        );
        // Sample after delay should be zero again
        if delay + 1 < all_output.len() {
            assert!(
                all_output[delay + 1].abs() < 1e-3,
                "sample {}: expected silence after impulse, got {}",
                delay + 1, all_output[delay + 1]
            );
        }
    }

    /// NonUniformConvolver with identity IR: latency equals base_block_size.
    #[test]
    fn non_uniform_exact_latency_identity_ir() {
        let base_block = 16;
        let mut ir = vec![0.0f32; base_block];
        ir[0] = 1.0;
        let mut conv = NonUniformConvolver::new(&ir, base_block, base_block);

        // Send impulse at sample 0 and collect several blocks
        let num_blocks = 4;
        let mut all_output = Vec::new();
        for b in 0..num_blocks {
            let mut input = vec![0.0f32; base_block];
            if b == 0 { input[0] = 1.0; }
            let mut output = vec![0.0f32; base_block];
            conv.process_block(&input, &mut output);
            all_output.extend_from_slice(&output);
        }

        // Find the first non-zero output sample
        let first_nonzero = all_output.iter().position(|&v| v.abs() > 0.5);
        assert!(
            first_nonzero.is_some(),
            "no non-zero output found in {} samples",
            all_output.len()
        );
        let idx = first_nonzero.unwrap();
        // Document actual latency — for single-tier NonUniform, it should be 0
        // (same as PartitionedConvolver since tier 0 processes immediately)
        assert!(
            idx == 0,
            "NonUniformConvolver identity IR: first non-zero at sample {idx}, expected 0"
        );
    }

    #[test]
    fn nu_long_ir_matches_naive() {
        // Simulate a realistic reverb: 2048-sample IR, base=64, max=512.
        let block_size = 64;
        let ir: Vec<f32> = (0..2048).map(|i| (-0.003 * i as f32).exp() * 0.1).collect();
        let mut conv = NonUniformConvolver::new(&ir, block_size, 512);

        let num_blocks = 60;
        let signal: Vec<f32> = (0..block_size * num_blocks)
            .map(|i| ((i as f32) * 0.11).sin())
            .collect();
        let mut output = vec![0.0f32; signal.len()];

        for b in 0..num_blocks {
            let s = b * block_size;
            conv.process_block(&signal[s..s + block_size], &mut output[s..s + block_size]);
        }

        let expected = naive_convolve(&signal, &ir);
        let mut max_err = 0.0f32;
        for i in 0..output.len() {
            let err = (output[i] - expected[i]).abs();
            if err > max_err {
                max_err = err;
            }
        }
        // Measured 3.2e-4 on aarch64 macOS debug (2026-04-02). Tightened from 0.1.
        assert!(
            max_err < 0.01,
            "max error {max_err} for long IR non-uniform convolution",
        );
    }
}
