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
mod tests;
