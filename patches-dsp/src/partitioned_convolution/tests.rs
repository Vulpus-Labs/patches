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

    for (i, &v) in output.iter().enumerate().take(block_size) {
        assert!(
            (v - 1.0).abs() < 1e-3,
            "sample {i}: got {v} expected 1.0",
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

    for (i, &v) in output.iter().enumerate().take(block_size) {
        assert!(
            (v - 1.0).abs() < 1e-3,
            "sample {i}: got {v} expected 1.0",
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
    for (i, &v) in all_output.iter().enumerate().take(delay) {
        assert!(
            v.abs() < 1e-6,
            "expected zero at sample {i}, got {v}"
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
    for (i, &v) in all_output.iter().enumerate().take(delay) {
        assert!(
            v.abs() < 1e-6,
            "sample {i}: expected silence before delay, got {v}"
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

// --- Direct-convolution reference cross-check (E092 / ticket 0545) ---

/// f64 reference direct-convolution. Drives the candidate-vs-reference error
/// check below. f64 is used so accumulated rounding in the reference does not
/// mask algorithmic error in the f32 candidate.
fn naive_convolve_f64(signal: &[f32], ir: &[f32]) -> Vec<f64> {
    let out_len = signal.len() + ir.len() - 1;
    let mut out = vec![0.0_f64; out_len];
    for (i, &s) in signal.iter().enumerate() {
        let sf = s as f64;
        for (j, &h) in ir.iter().enumerate() {
            out[i + j] += sf * h as f64;
        }
    }
    out
}

/// Documented IR for the reference tests: a pair of impulses (at 0 and 7) with
/// a first-order exponentially-decaying tail. The tail exercises sustained
/// multi-partition contributions; the double impulses make partition-boundary
/// errors easy to read off.
fn reference_ir() -> Vec<f32> {
    let mut ir = vec![0.0_f32; 64];
    ir[0] = 1.0;
    ir[7] = -0.6;
    for i in 1..ir.len() {
        ir[i] += 0.9_f32 * (-0.05 * i as f32).exp() * ((i as f32) * 0.3).sin();
    }
    ir
}

/// Cross-check `PartitionedConvolver` against an f64 direct convolution.
///
/// Covers a 64-tap IR (4 partitions at block_size=16) driven by an input at
/// least 3× the partition length (so the input crosses multiple partition
/// boundaries) and asserts the output matches direct convolution sample-for-
/// sample within an FFT-precision tolerance.
///
/// # Latency invariant
///
/// `PartitionedConvolver` has zero algorithmic latency: block `b`'s output
/// covers input samples `[b·N, (b+1)·N)`, matching naive convolution position-
/// for-position. Since `latency == 0`, the "no nonzero samples in positions
/// `0..latency`" check is vacuously satisfied; the per-sample error check
/// already enforces that no output sample arrives earlier than the reference.
#[test]
fn partitioned_convolver_matches_direct_reference() {
    let block_size = 16;
    let ir = reference_ir();
    assert_eq!(ir.len(), 64);
    let parts = IrPartitions::from_ir(&ir, block_size);
    assert_eq!(parts.num_partitions(), 4);
    let mut conv = PartitionedConvolver::new(parts);

    let num_blocks = 12; // 192 samples = 12 × partition length
    let total = num_blocks * block_size;
    assert!(total >= 3 * block_size);

    // Deterministic broadband-ish input: two overlaid sinusoids at
    // incommensurate rates. The exact choice does not matter; what matters
    // is that partition boundaries fall at distinct input phases.
    let signal: Vec<f32> = (0..total)
        .map(|i| {
            let x = i as f32;
            0.8 * (x * 0.37).sin() + 0.3 * (x * 0.11 + 0.7).cos()
        })
        .collect();

    let mut output = vec![0.0_f32; total];
    for b in 0..num_blocks {
        let s = b * block_size;
        conv.process_block(&signal[s..s + block_size], &mut output[s..s + block_size]);
    }

    let reference = naive_convolve_f64(&signal, &ir);

    // Tolerance chosen to reflect accumulated f32 FFT error over 12 blocks
    // with 4 partitions; measured ~1e-5 on aarch64 macOS, 1e-4 gives comfortable
    // margin without masking a real regression.
    const TOLERANCE: f64 = 1.0e-4;
    let mut max_err = 0.0_f64;
    let mut worst_idx = 0usize;
    for i in 0..output.len() {
        let err = ((output[i] as f64) - reference[i]).abs();
        if err > max_err {
            max_err = err;
            worst_idx = i;
        }
    }
    assert!(
        max_err < TOLERANCE,
        "PartitionedConvolver max abs error {max_err:.3e} at sample {worst_idx} \
         exceeds tolerance {TOLERANCE:.0e}"
    );
}

/// Same cross-check against `NonUniformConvolver`.
///
/// NonUniform splits the IR into geometrically-growing tiers. Tier 0 processes
/// every call at `base_block_size`; larger tiers accumulate input before
/// firing. Internally each tier has its own delay, but the summed output
/// across tiers aligns position-for-position with naive convolution (same
/// latency invariant as `PartitionedConvolver`: zero).
#[test]
fn non_uniform_convolver_matches_direct_reference() {
    let base_block = 16;
    let max_tier_block = 32;
    let ir = reference_ir();
    let mut conv = NonUniformConvolver::new(&ir, base_block, max_tier_block);

    let num_blocks = 16; // 256 samples > 3× IR length
    let total = num_blocks * base_block;
    let signal: Vec<f32> = (0..total)
        .map(|i| {
            let x = i as f32;
            0.6 * (x * 0.29).sin() + 0.4 * (x * 0.17 + 1.1).cos()
        })
        .collect();

    let mut output = vec![0.0_f32; total];
    for b in 0..num_blocks {
        let s = b * base_block;
        conv.process_block(&signal[s..s + base_block], &mut output[s..s + base_block]);
    }

    let reference = naive_convolve_f64(&signal, &ir);

    const TOLERANCE: f64 = 1.0e-4;
    let mut max_err = 0.0_f64;
    let mut worst_idx = 0usize;
    for i in 0..output.len() {
        let err = ((output[i] as f64) - reference[i]).abs();
        if err > max_err {
            max_err = err;
            worst_idx = i;
        }
    }
    assert!(
        max_err < TOLERANCE,
        "NonUniformConvolver max abs error {max_err:.3e} at sample {worst_idx} \
         exceeds tolerance {TOLERANCE:.0e}"
    );
}
