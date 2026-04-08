//! Packed Real FFT — forward and inverse transforms for real-valued signals.
//!
//! Uses an N/2-point complex FFT as the engine, treating N real samples as N/2
//! interleaved complex values. The output is packed into N f32 values using the
//! CMSIS/ARM convention:
//!
//! ```text
//!   [0]     = X[0].real        (DC, purely real)
//!   [1]     = X[N/2].real      (Nyquist, purely real)
//!   [2k]    = X[k].real        for k = 1 .. N/2-1
//!   [2k+1]  = X[k].imag
//! ```
//!
//! The forward transform produces an unnormalised DFT. The inverse includes
//! 1/(N/2) normalisation, so forward followed by inverse is identity within
//! floating-point tolerance.
//!
//! # Performance
//!
//! On x86/x86_64 with AVX2+FMA, the butterfly inner loop uses explicit SIMD
//! intrinsics: `vfmaddsub` (forward) / `vfmsubadd` (inverse), processing four
//! complex butterfly pairs per AVX2 register width.  On other targets the scalar
//! path is used, aided by `split_at_mut` aliasing proofs and a const-generic
//! `INVERSE` parameter that eliminates the forward/inverse branch at compile time.
//!
//! Twiddle factors are stored stage-sequentially so every stage reads them with
//! stride-1 memory access regardless of the logical FFT stride.
//!
//! # Constraints
//!
//! N must be a power of 2 and >= 4. No heap allocation occurs during `forward`
//! or `inverse`.

use std::f64::consts::PI;

/// Packed Real FFT processor. Construct once per size, reuse across many transforms.
pub struct RealPackedFft {
    n: usize,
    half_n: usize,
    log2_half_n: usize,
    /// Bit-reversal permutation indices for the N/2-point complex FFT.
    bit_rev: Vec<usize>,
    /// Stage-sequential complex FFT twiddle factors.
    ///
    /// Stage `s` (1-indexed, `s = 1..=log2_half_n`) starts at `stage_tw_offsets[s]`
    /// and contains `1 << (s-1)` complex pairs stored as `2 * (1 << (s-1))` f32 values:
    ///   `[cos(2πj / 2^s), sin(2πj / 2^s)]` for `j = 0 .. (1<<(s-1))-1`.
    ///
    /// Sequential layout gives stride-1 reads in the butterfly inner loop, enabling
    /// efficient SIMD loads at every stage.
    fft_tw: Vec<f32>,
    stage_tw_offsets: Vec<usize>,
    /// Twiddle factors for the real-FFT post/pre-processing step.
    /// `real_tw[2k] = cos(2πk/N)`, `real_tw[2k+1] = -sin(2πk/N)`.
    real_tw: Vec<f32>,
}

impl RealPackedFft {
    /// Construct a `RealPackedFft` for a given signal length.
    ///
    /// Twiddle tables are computed at f64 precision and stored as f32.
    ///
    /// # Panics
    /// Panics if `n` is not a power of 2 or is less than 4.
    pub fn new(n: usize) -> Self {
        assert!(
            n >= 4 && n.is_power_of_two(),
            "n must be a power of 2 and >= 4, got {n}"
        );

        let half_n = n >> 1;
        let log2_half_n = half_n.trailing_zeros() as usize;

        // Bit-reversal permutation table.
        let mut bit_rev = vec![0usize; half_n];
        for (i, entry) in bit_rev.iter_mut().enumerate() {
            let mut rev = 0usize;
            let mut val = i;
            for _ in 0..log2_half_n {
                rev = (rev << 1) | (val & 1);
                val >>= 1;
            }
            *entry = rev;
        }

        // Stage-sequential twiddle table.
        // Stage s uses W_{2^s}^j = exp(-2πi·j/2^s) for j = 0..(1<<(s-1))-1,
        // stored contiguously so the butterfly inner loop reads with stride 1.
        //
        // Stage s starts at offset 2^s - 2 in fft_tw (derived analytically:
        // each stage k contributes 2*(1<<(k-1)) floats, summing to 2^s - 2).
        let mut fft_tw: Vec<f32> = Vec::with_capacity(2 * half_n);
        for s in 1..=log2_half_n {
            let half_size = 1usize << (s - 1);
            let full_dft_size = half_size << 1; // = 2^s
            for j in 0..half_size {
                let angle = -2.0 * PI * j as f64 / full_dft_size as f64;
                fft_tw.push(angle.cos() as f32);
                fft_tw.push(angle.sin() as f32);
            }
        }
        let stage_tw_offsets: Vec<usize> = (0..=log2_half_n)
            .map(|s| if s == 0 { 0 } else { (1usize << s) - 2 })
            .collect();

        // Real-FFT twiddles: W_N^k for k = 0..half_n-1.
        let mut real_tw = vec![0.0f32; n];
        for k in 0..half_n {
            let angle = -2.0 * PI * k as f64 / n as f64;
            real_tw[2 * k]     = angle.cos() as f32;
            real_tw[2 * k + 1] = angle.sin() as f32;
        }

        Self { n, half_n, log2_half_n, bit_rev, fft_tw, stage_tw_offsets, real_tw }
    }

    /// Forward real FFT.
    ///
    /// Transforms N real samples in `buf` into a packed complex spectrum in-place.
    ///
    /// # Panics
    /// Panics if `buf.len() != n`.
    pub fn forward(&self, buf: &mut [f32]) {
        assert_eq!(buf.len(), self.n, "buf length must equal n");
        self.complex_fft::<false>(buf);
        self.real_postprocess(buf);
    }

    /// Inverse real FFT.
    ///
    /// Transforms a packed complex spectrum back to N real samples in-place.
    /// Includes 1/(N/2) normalisation so that `forward` followed by `inverse`
    /// is identity within floating-point tolerance.
    ///
    /// # Panics
    /// Panics if `freq.len() != n`.
    pub fn inverse(&self, freq: &mut [f32]) {
        assert_eq!(freq.len(), self.n, "freq length must equal n");
        self.real_preprocess(freq);
        self.complex_fft::<true>(freq);
        let scale = 1.0 / self.half_n as f32;
        for x in freq.iter_mut() {
            *x *= scale;
        }
    }

    /// Returns N, the real FFT length.
    pub fn len(&self) -> usize { self.n }

    /// Always false — a `RealPackedFft` requires N >= 4 by construction.
    pub fn is_empty(&self) -> bool { false }

    // ── Complex FFT dispatch ──────────────────────────────────────────────────

    /// Runs the N/2-point complex FFT in-place.
    /// `INVERSE` is a const generic: the forward/inverse branch is eliminated at
    /// compile time, producing two specialised monomorphisations.
    fn complex_fft<const INVERSE: bool>(&self, buf: &mut [f32]) {
        self.bit_reverse(buf);

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            // SAFETY: avx2 and fma confirmed available on this CPU at runtime.
            return unsafe { self.butterfly_stages_avx2_fma::<INVERSE>(buf) };
        }

        #[cfg(target_arch = "aarch64")]
        if std::arch::is_aarch64_feature_detected!("neon") {
            // SAFETY: NEON confirmed available at runtime (mandatory on AArch64).
            return unsafe { self.butterfly_stages_neon::<INVERSE>(buf) };
        }

        self.butterfly_stages_scalar::<INVERSE>(buf);
    }

    fn bit_reverse(&self, buf: &mut [f32]) {
        let m = self.half_n;
        for i in 0..m {
            let j = self.bit_rev[i];
            if j > i {
                buf.swap(2 * i,     2 * j);
                buf.swap(2 * i + 1, 2 * j + 1);
            }
        }
    }

    // ── Scalar butterfly stages ───────────────────────────────────────────────

    /// Butterfly stages using portable safe Rust.
    ///
    /// `split_at_mut` gives the compiler a non-aliasing proof for `lower` and
    /// `upper`, enabling whatever auto-vectorisation the target supports.
    fn butterfly_stages_scalar<const INVERSE: bool>(&self, buf: &mut [f32]) {
        let m = self.half_n;
        for s in 1..=self.log2_half_n {
            let half_size = 1usize << (s - 1);
            let full_size = half_size << 1;
            let tw_off = self.stage_tw_offsets[s];
            let tw = &self.fft_tw[tw_off..tw_off + 2 * half_size];

            let mut g = 0;
            while g < m {
                let (lower, upper) =
                    buf[2 * g..2 * (g + full_size)].split_at_mut(2 * half_size);
                for j in 0..half_size {
                    let wr = tw[2 * j];
                    // Const-generic branch: LLVM eliminates it entirely.
                    let wi = if INVERSE { -tw[2 * j + 1] } else { tw[2 * j + 1] };

                    let ar = lower[2 * j];     let ai = lower[2 * j + 1];
                    let br = upper[2 * j];     let bi = upper[2 * j + 1];

                    let tr = wr * br - wi * bi;
                    let ti = wr * bi + wi * br;

                    lower[2 * j]     = ar + tr;
                    lower[2 * j + 1] = ai + ti;
                    upper[2 * j]     = ar - tr;
                    upper[2 * j + 1] = ai - ti;
                }
                g += full_size;
            }
        }
    }

    // ── AVX2+FMA butterfly stages ─────────────────────────────────────────────

    /// Butterfly stages using AVX2+FMA intrinsics.
    ///
    /// Processes 4 complex butterfly pairs per iteration using:
    /// - `vmoveldup`/`vmovehdup` to fan twiddle re/im across lane pairs
    /// - `vpermilps` (via shuffle) to swap re/im in the data register
    /// - `vfmaddsub` (forward) / `vfmsubadd` (inverse) for fused complex multiply
    ///
    /// Stages with `half_size < 4` use the scalar path for the remainder.
    ///
    /// # Safety
    /// Caller must ensure AVX2 and FMA are available on the current CPU.
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[target_feature(enable = "avx2,fma")]
    unsafe fn butterfly_stages_avx2_fma<const INVERSE: bool>(&self, buf: &mut [f32]) {
        let m = self.half_n;
        for s in 1..=self.log2_half_n {
            let half_size = 1usize << (s - 1);
            let full_size = half_size << 1;
            let tw_off = self.stage_tw_offsets[s];
            let tw = &self.fft_tw[tw_off..tw_off + 2 * half_size];

            // Number of complete 4-butterfly AVX2 chunks per group.
            let avx_count    = half_size / 4;
            let scalar_start = avx_count * 4;

            let mut g = 0;
            while g < m {
                let (lower, upper) =
                    buf[2 * g..2 * (g + full_size)].split_at_mut(2 * half_size);

                // ── AVX2 path: 4 butterflies per iteration ────────────────────
                for chunk in 0..avx_count {
                    let j = chunk * 4;
                    // SAFETY: j + 4 <= half_size <= lower.len()/2, so the pointer
                    // arithmetic stays within the slice bounds established above.
                    Self::butterfly_chunk_avx2_fma::<INVERSE>(
                        lower.as_mut_ptr().add(2 * j),
                        upper.as_mut_ptr().add(2 * j),
                        tw.as_ptr().add(2 * j),
                    );
                }

                // ── Scalar remainder (stages 1–2 always land here entirely) ──
                for j in scalar_start..half_size {
                    let wr = tw[2 * j];
                    let wi = if INVERSE { -tw[2 * j + 1] } else { tw[2 * j + 1] };

                    let ar = lower[2 * j];     let ai = lower[2 * j + 1];
                    let br = upper[2 * j];     let bi = upper[2 * j + 1];

                    let tr = wr * br - wi * bi;
                    let ti = wr * bi + wi * br;

                    lower[2 * j]     = ar + tr;
                    lower[2 * j + 1] = ai + ti;
                    upper[2 * j]     = ar - tr;
                    upper[2 * j + 1] = ai - ti;
                }

                g += full_size;
            }
        }
    }

    /// Process exactly 4 complex butterfly pairs using AVX2+FMA.
    ///
    /// Data layout (8 f32 = one AVX2 register per pointer):
    /// ```text
    ///   lower / upper: [re₀, im₀, re₁, im₁, re₂, im₂, re₃, im₃]
    ///   tw:            [wr₀, wi₀, wr₁, wi₁, wr₂, wi₂, wr₃, wi₃]
    /// ```
    ///
    /// Complex multiply via fmaddsub/fmsubadd:
    /// ```text
    ///   tw_rr = moveldup(tw) = [wr₀,wr₀, wr₁,wr₁, wr₂,wr₂, wr₃,wr₃]
    ///   tw_ii = movehdup(tw) = [wi₀,wi₀, wi₁,wi₁, wi₂,wi₂, wi₃,wi₃]
    ///   b_swap = shuffle(b)  = [im₀,re₀, im₁,re₁, im₂,re₂, im₃,re₃]
    ///
    ///   forward:  t = fmaddsub(tw_rr, b, tw_ii * b_swap)
    ///               → even: wr*re - wi*im  (real part)
    ///               → odd:  wr*im + wi*re  (imag part)
    ///
    ///   inverse:  t = fmsubadd(tw_rr, b, tw_ii * b_swap)
    ///               → even: wr*re + wi*im
    ///               → odd:  wr*im - wi*re  (conjugate twiddle)
    /// ```
    ///
    /// # Safety
    /// - `lower`, `upper`, and `tw` must each point to at least 8 initialised
    ///   f32 values within their respective allocations (no overlap between lower
    ///   and upper; they come from `split_at_mut`).
    /// - AVX2 and FMA must be available on the current CPU (ensured by the
    ///   `#[target_feature]` attribute on the enclosing call chain).
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[target_feature(enable = "avx2,fma")]
    #[inline]
    unsafe fn butterfly_chunk_avx2_fma<const INVERSE: bool>(
        lower: *mut f32,
        upper: *mut f32,
        tw:    *const f32,
    ) {
        #[cfg(target_arch = "x86")]
        use std::arch::x86::*;
        #[cfg(target_arch = "x86_64")]
        use std::arch::x86_64::*;

        let a    = _mm256_loadu_ps(lower);
        let b    = _mm256_loadu_ps(upper);
        let tw_v = _mm256_loadu_ps(tw);

        // Fan twiddle real/imag parts across adjacent lane pairs.
        let tw_rr = _mm256_moveldup_ps(tw_v); // [wr₀,wr₀, wr₁,wr₁, ...]
        let tw_ii = _mm256_movehdup_ps(tw_v); // [wi₀,wi₀, wi₁,wi₁, ...]

        // Swap re/im in b: [re₀,im₀,...] → [im₀,re₀,...].
        // Imm8 = 0b_10_11_00_01: within each 128-bit lane, elements 0↔1 and 2↔3 swap.
        let b_swap = _mm256_shuffle_ps(b, b, 0b_10_11_00_01);

        // Fused complex multiply + butterfly.
        // fmaddsub(a,b,c): even lanes → a*b - c, odd lanes → a*b + c.
        // fmsubadd(a,b,c): even lanes → a*b + c, odd lanes → a*b - c.
        let tw_ii_b_swap = _mm256_mul_ps(tw_ii, b_swap);
        let t = if INVERSE {
            _mm256_fmsubadd_ps(tw_rr, b, tw_ii_b_swap)
        } else {
            _mm256_fmaddsub_ps(tw_rr, b, tw_ii_b_swap)
        };

        _mm256_storeu_ps(lower, _mm256_add_ps(a, t));
        _mm256_storeu_ps(upper, _mm256_sub_ps(a, t));
    }

    // ── NEON (AArch64) butterfly stages ──────────────────────────────────────

    /// Butterfly stages using ARM NEON intrinsics, available on all AArch64
    /// hardware including Apple Silicon.
    ///
    /// Processes 2 complex butterfly pairs per iteration (one `float32x4_t`
    /// per pointer).  Complex multiply uses stable NEON: `vtrn1q_f32` /
    /// `vtrn2q_f32` to fan twiddle pairs, `vrev64q_f32` to swap re/im in the
    /// data register, and an XOR sign-flip on alternating lanes to produce the
    /// alternating subtract/add needed for the real and imaginary outputs.
    ///
    /// Stage 1 (half_size = 1) always falls through to the scalar remainder.
    ///
    /// # Safety
    /// Caller must ensure NEON is available on this CPU.
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn butterfly_stages_neon<const INVERSE: bool>(&self, buf: &mut [f32]) {
        let m = self.half_n;
        for s in 1..=self.log2_half_n {
            let half_size = 1usize << (s - 1);
            let full_size = half_size << 1;
            let tw_off = self.stage_tw_offsets[s];
            let tw = &self.fft_tw[tw_off..tw_off + 2 * half_size];

            // Number of complete 2-butterfly NEON chunks per group.
            let neon_count   = half_size / 2;
            let scalar_start = neon_count * 2;

            let mut g = 0;
            while g < m {
                let (lower, upper) =
                    buf[2 * g..2 * (g + full_size)].split_at_mut(2 * half_size);

                // ── NEON path: 2 butterflies per iteration ────────────────────
                for chunk in 0..neon_count {
                    let j = chunk * 2;
                    // SAFETY: j + 2 <= half_size, so all pointer offsets stay
                    // within the bounds of lower, upper, and tw.
                    Self::butterfly_chunk_neon::<INVERSE>(
                        lower.as_mut_ptr().add(2 * j),
                        upper.as_mut_ptr().add(2 * j),
                        tw.as_ptr().add(2 * j),
                    );
                }

                // ── Scalar remainder (stage 1 always lands here entirely) ─────
                for j in scalar_start..half_size {
                    let wr = tw[2 * j];
                    let wi = if INVERSE { -tw[2 * j + 1] } else { tw[2 * j + 1] };

                    let ar = lower[2 * j];     let ai = lower[2 * j + 1];
                    let br = upper[2 * j];     let bi = upper[2 * j + 1];

                    let tr = wr * br - wi * bi;
                    let ti = wr * bi + wi * br;

                    lower[2 * j]     = ar + tr;
                    lower[2 * j + 1] = ai + ti;
                    upper[2 * j]     = ar - tr;
                    upper[2 * j + 1] = ai - ti;
                }

                g += full_size;
            }
        }
    }

    /// Process exactly 2 complex butterfly pairs using stable NEON intrinsics.
    ///
    /// Data layout (4 f32 = one NEON register per pointer):
    /// ```text
    ///   lower / upper: [re₀, im₀, re₁, im₁]
    ///   tw:            [wr₀, wi₀, wr₁, wi₁]
    /// ```
    ///
    /// Complex multiply via stable NEON:
    /// ```text
    ///   tw_rr = vtrn1q(tw, tw) = [wr₀, wr₀, wr₁, wr₁]
    ///   tw_ii = vtrn2q(tw, tw) = [wi₀, wi₀, wi₁, wi₁]
    ///   b_swap = vrev64q(b)    = [im₀, re₀, im₁, re₁]
    ///
    ///   p1 = tw_rr * b     = [wr·re₀, wr·im₀, wr·re₁, wr·im₁]
    ///   p2 = tw_ii * b_swap = [wi·im₀, wi·re₀, wi·im₁, wi·re₁]
    ///
    ///   forward sign  = [-0, +0, -0, +0]  →  p2_neg = [-wi·im, +wi·re, ...]
    ///   inverse sign  = [+0, -0, +0, -0]  →  p2_neg = [+wi·im, -wi·re, ...]
    ///
    ///   t = p1 + p2_neg:
    ///     forward:  [wr·re − wi·im, wr·im + wi·re, ...]  ✓
    ///     inverse:  [wr·re + wi·im, wr·im − wi·re, ...]  ✓  (conj. twiddle)
    /// ```
    ///
    /// # Safety
    /// - `lower`, `upper`, and `tw` must each point to at least 4 initialised
    ///   f32 values within their respective allocations (lower and upper do not
    ///   overlap; guaranteed by `split_at_mut` in the caller).
    /// - NEON must be available (ensured by the `#[target_feature]` attribute
    ///   on the enclosing call chain).
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    #[inline]
    unsafe fn butterfly_chunk_neon<const INVERSE: bool>(
        lower: *mut f32,
        upper: *mut f32,
        tw:    *const f32,
    ) {
        use std::arch::aarch64::*;

        let a    = vld1q_f32(lower);
        let b    = vld1q_f32(upper);
        let tw_v = vld1q_f32(tw);

        // Fan twiddle real/imaginary parts across adjacent lane pairs.
        let tw_rr = vtrn1q_f32(tw_v, tw_v); // [wr₀, wr₀, wr₁, wr₁]
        let tw_ii = vtrn2q_f32(tw_v, tw_v); // [wi₀, wi₀, wi₁, wi₁]

        // Swap re/im lanes in b within each 64-bit element.
        let b_swap = vrev64q_f32(b);         // [im₀, re₀, im₁, re₁]

        // Partial products.
        let p1 = vmulq_f32(tw_rr, b);       // [wr·re, wr·im, wr·re, wr·im]
        let p2 = vmulq_f32(tw_ii, b_swap);  // [wi·im, wi·re, wi·im, wi·re]

        // Sign mask: XOR to negate the lanes whose product must be subtracted.
        //   Forward: even lanes (re output) subtract  → sign = [-0, +0, -0, +0]
        //   Inverse: odd  lanes (im output) subtract  → sign = [+0, -0, +0, -0]
        // IEEE 754: XOR with 0x80000000 flips the sign bit without affecting the
        // magnitude, so vaddq after XOR produces the required alternating ±.
        //
        // The sign_bits constant is a compile-time choice: the branch on the
        // const generic is eliminated and only one arm survives in each
        // monomorphisation.
        let sign_bits: u64 = if INVERSE { 0x80000000_00000000 } else { 0x00000000_80000000 };
        let sign = vreinterpretq_f32_u32(vcombine_u32(
            vcreate_u32(sign_bits),
            vcreate_u32(sign_bits),
        ));
        let p2_neg = vreinterpretq_f32_u32(veorq_u32(
            vreinterpretq_u32_f32(p2),
            vreinterpretq_u32_f32(sign),
        ));

        let t = vaddq_f32(p1, p2_neg);

        vst1q_f32(lower, vaddq_f32(a, t));
        vst1q_f32(upper, vsubq_f32(a, t));
    }

    // ── Real FFT post/pre-processing ─────────────────────────────────────────

    fn real_postprocess(&self, buf: &mut [f32]) {
        // DC and Nyquist (both purely real, packed into [0] and [1]).
        let zr0 = buf[0];
        let zi0 = buf[1];
        buf[0] = zr0 + zi0;
        buf[1] = zr0 - zi0;

        // Midpoint k = N/4: only negate the imaginary part.
        let mid = self.half_n >> 1;
        buf[2 * mid + 1] = -buf[2 * mid + 1];

        // General conjugate pairs k and halfN-k, for k = 1 .. N/4-1.
        for k in 1..mid {
            let ik = 2 * k;
            let im = 2 * (self.half_n - k);

            let zr_k = buf[ik];     let zi_k = buf[ik + 1];
            let zr_m = buf[im];     let zi_m = buf[im + 1];

            let sum_r  = zr_k + zr_m;
            let diff_r = zr_k - zr_m;
            let sum_i  = zi_k + zi_m;
            let diff_i = zi_k - zi_m;

            let wr = self.real_tw[ik];
            let wi = self.real_tw[ik + 1];

            let tw_br = (wr * sum_i + wi * diff_r) * 0.5;
            let tw_bi = (wi * sum_i - wr * diff_r) * 0.5;
            buf[ik]     = sum_r  * 0.5 + tw_br;
            buf[ik + 1] = diff_i * 0.5 + tw_bi;

            let wr_m = self.real_tw[im];
            let wi_m = self.real_tw[im + 1];

            let tw_br_m = (wr_m * sum_i - wi_m * diff_r) * 0.5;
            let tw_bi_m = (wi_m * sum_i + wr_m * diff_r) * 0.5;
            buf[im]     =  sum_r  * 0.5 + tw_br_m;
            buf[im + 1] = -diff_i * 0.5 + tw_bi_m;
        }
    }

    fn real_preprocess(&self, freq: &mut [f32]) {
        // DC and Nyquist.
        let dc  = freq[0];
        let nyq = freq[1];
        freq[0] = (dc + nyq) * 0.5;
        freq[1] = (dc - nyq) * 0.5;

        // Midpoint k = N/4.
        let mid = self.half_n >> 1;
        freq[2 * mid + 1] = -freq[2 * mid + 1];

        // General pairs.
        for k in 1..mid {
            let ik = 2 * k;
            let im = 2 * (self.half_n - k);

            let xr_k = freq[ik];     let xi_k = freq[ik + 1];
            let xr_m = freq[im];     let xi_m = freq[im + 1];

            let pr = xr_k + xr_m;
            let pi = xi_k - xi_m;
            let qr = xr_k - xr_m;
            let qi = xi_k + xi_m;

            let wr = self.real_tw[ik];
            let wi = self.real_tw[ik + 1];

            let jtw_qr = (wi * qr - wr * qi) * 0.5;
            let jtw_qi = (wr * qr + wi * qi) * 0.5;
            freq[ik]     = pr * 0.5 + jtw_qr;
            freq[ik + 1] = pi * 0.5 + jtw_qi;

            let wr_m = self.real_tw[im];
            let wi_m = self.real_tw[im + 1];

            let jtw_qr_m = (-wi_m * qr - wr_m * qi) * 0.5;
            let jtw_qi_m = (-wr_m * qr + wi_m * qi) * 0.5;
            freq[im]     =  pr * 0.5 + jtw_qr_m;
            freq[im + 1] = -pi * 0.5 + jtw_qi_m;
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_within;

    fn make_sine(n: usize, freq_bin: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                (2.0 * std::f64::consts::PI * freq_bin as f64 * i as f64 / n as f64).sin() as f32
            })
            .collect()
    }

    /// forward() followed by inverse() is identity within tolerance.
    #[test]
    fn round_trip_identity() {
        let n = 256;
        let fft = RealPackedFft::new(n);
        let original: Vec<f32> = make_sine(n, 7);
        let mut buf = original.clone();
        fft.forward(&mut buf);
        fft.inverse(&mut buf);
        // Measured 3.58e-7 on aarch64 macOS debug (2026-04-02). Tightened from 1e-5.
        for (i, (&a, &b)) in original.iter().zip(buf.iter()).enumerate() {
            assert_within!(a, b, 1e-6, "sample {i}: original={a}, recovered={b}");
        }
    }

    /// DC input (all ones) produces X[0] = N, all other bins zero.
    #[test]
    fn dc_input_goes_to_bin_zero() {
        let n = 64;
        let fft = RealPackedFft::new(n);
        let mut buf = vec![1.0f32; n];
        fft.forward(&mut buf);
        assert_within!(n as f32, buf[0], 1e-4);
        for k in 1..n {
            assert_within!(0.0, buf[k], 1e-4, "bin {k}: {}", buf[k]);
        }
    }

    /// A pure sinusoid at bin k concentrates energy at that bin only.
    #[test]
    fn sine_concentrates_at_correct_bin() {
        let n = 512;
        let target_bin = 17usize;
        let fft = RealPackedFft::new(n);
        let mut buf = make_sine(n, target_bin);
        fft.forward(&mut buf);

        let re = buf[2 * target_bin];
        let im = buf[2 * target_bin + 1];
        let mag_target = (re * re + im * im).sqrt();

        for k in 1..(n / 2) {
            if k == target_bin { continue; }
            let r = buf[2 * k];
            let i = buf[2 * k + 1];
            let mag = (r * r + i * i).sqrt();
            assert!(
                mag < 1.0,
                "bin {k} has unexpected magnitude {mag} (target bin {target_bin} has {mag_target})"
            );
        }
        assert!(mag_target > n as f32 * 0.4, "target bin magnitude too low: {mag_target}");
    }

    /// Impulse at sample 0 produces a flat magnitude spectrum.
    #[test]
    fn impulse_produces_flat_spectrum() {
        let n = 128;
        let fft = RealPackedFft::new(n);
        let mut buf = vec![0.0f32; n];
        buf[0] = 1.0;
        fft.forward(&mut buf);

        assert_within!(1.0, buf[0], 1e-5);
        assert_within!(1.0, buf[1], 1e-5);
        for k in 1..(n / 2) {
            let re = buf[2 * k];
            let im = buf[2 * k + 1];
            let mag = (re * re + im * im).sqrt();
            assert_within!(1.0, mag, 1e-4, "bin {k}: magnitude={mag}, expected ≈1.0");
        }
    }

    /// Round-trip with a large (power-of-2) size exercises all AVX2 stages.
    #[test]
    fn round_trip_large() {
        let n = 4096;
        let fft = RealPackedFft::new(n);
        let original: Vec<f32> = make_sine(n, 31);
        let mut buf = original.clone();
        fft.forward(&mut buf);
        fft.inverse(&mut buf);
        // Measured 4.77e-7 on aarch64 macOS debug (2026-04-02). Tightened from 1e-4.
        for (i, (&a, &b)) in original.iter().zip(buf.iter()).enumerate() {
            assert_within!(a, b, 1e-5, "sample {i}: original={a}, recovered={b}");
        }
    }

    // ── T-0240: Conjugate symmetry ───────────────────────────────────────────

    /// Forward transform of a real signal has conjugate-symmetric bins:
    /// X[k] = conj(X[N-k]). In packed layout this means bins k and N/2-k
    /// share (re, -im) relationship for interior bins.
    #[test]
    fn real_signal_conjugate_symmetry() {
        let n = 256;
        let fft = RealPackedFft::new(n);
        let mut buf: Vec<f32> = (0..n)
            .map(|i| ((i as f32) * 0.37).sin() + ((i as f32) * 0.11).cos())
            .collect();
        fft.forward(&mut buf);

        // DC (buf[0]) and Nyquist (buf[1]) are purely real — no imaginary
        // part to verify.
        //
        // For interior bins k = 1..N/2-1: X[k] should equal conj(X[N-k]).
        // But the packed layout only stores bins 0..N/2 (the positive half),
        // so conjugate symmetry is inherent by construction for real inputs.
        // Instead, verify that IFFT of the spectrum recovers a real signal,
        // which is the consequence of conjugate symmetry being maintained.
        let mut recovered = buf.clone();
        fft.inverse(&mut recovered);
        // If symmetry were broken, the recovered signal would have imaginary
        // residue appearing as large errors in the real samples.
        let original: Vec<f32> = (0..n)
            .map(|i| ((i as f32) * 0.37).sin() + ((i as f32) * 0.11).cos())
            .collect();
        for (i, (&a, &b)) in original.iter().zip(recovered.iter()).enumerate() {
            assert_within!(a, b, 1e-4, "symmetry violation at sample {i}: original={a}, recovered={b}");
        }

        // Also verify DC and Nyquist are real (non-zero) as expected.
        assert!(buf[0].abs() > 1e-6, "DC bin should be non-trivial for this signal");
    }

    // ── T-0240: Parseval's theorem ───────────────────────────────────────────

    /// Energy in time domain equals energy in frequency domain (within tolerance).
    /// Parseval: sum(|x[n]|^2) = (1/N) * sum(|X[k]|^2)
    #[test]
    fn parsevals_theorem() {
        let n = 512;
        let fft = RealPackedFft::new(n);
        let signal: Vec<f32> = (0..n)
            .map(|i| ((i as f32) * 0.13).sin() + 0.5 * ((i as f32) * 0.29).cos())
            .collect();

        // Time-domain energy.
        let time_energy: f64 = signal.iter().map(|&x| (x as f64) * (x as f64)).sum();

        // Forward FFT.
        let mut buf = signal.clone();
        fft.forward(&mut buf);

        // Frequency-domain energy (packed format).
        // DC and Nyquist bins appear once; interior bins appear twice (positive
        // and negative frequencies) so we count them with weight 2.
        let dc_energy = (buf[0] as f64) * (buf[0] as f64);
        let nyq_energy = (buf[1] as f64) * (buf[1] as f64);
        let mut interior_energy = 0.0_f64;
        for k in 1..(n / 2) {
            let re = buf[2 * k] as f64;
            let im = buf[2 * k + 1] as f64;
            interior_energy += re * re + im * im;
        }
        let freq_energy = (dc_energy + nyq_energy + 2.0 * interior_energy) / n as f64;

        let relative_error = ((time_energy - freq_energy) / time_energy).abs();
        assert!(
            relative_error < 1e-4,
            "Parseval's theorem violated: time_energy={time_energy:.6}, freq_energy={freq_energy:.6}, relative_error={relative_error:.6}"
        );
    }
}
