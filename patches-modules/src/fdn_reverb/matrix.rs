//! Hadamard feedback matrix and stereo output gain vectors for the FDN.

pub(super) const LINES: usize = 8;

/// 1/√8.
///
/// Stereo output is summed using orthogonal sign patterns, each element ±1/√8:
///   OUT_L: + − + − + − + −
///   OUT_R: + + − − + + − −
/// The kernel emits these as sign-only adds, folding `INV_SQRT8` into the
/// wet gain — see `process_sample`.
pub(super) const INV_SQRT8: f32 = 0.353_553_4_f32;

#[inline]
pub(super) fn hadamard8(mut x: [f32; 8]) -> [f32; 8] {
    for step in [4_usize, 2, 1] {
        let mut i = 0;
        while i < 8 {
            for j in i..i + step {
                let a = x[j];
                let b = x[j + step];
                x[j]        = a + b;
                x[j + step] = a - b;
            }
            i += step * 2;
        }
    }
    // Normalise by 1/√8 to preserve energy.
    x.map(|v| v * INV_SQRT8)
}
