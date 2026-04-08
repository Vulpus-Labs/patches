import numpy as np
from scipy.signal import firwin

# --- Parameters ---
numtaps = 33        # odd, 33 taps gives ~80 dB stopband
cutoff = 0.5        # halfband cutoff: exactly fs/4 (normalized to Nyquist)
window = 'hamming'  # smooth window

# --- Design half-band FIR ---
h = firwin(numtaps, cutoff=cutoff, window=window, pass_zero=True)

center_idx = numtaps // 2
center = h[center_idx]

# For a halfband FIR, nonzero off-center taps are at ODD indices before center
# (i.e. at odd distance from center_idx). Even-distance taps are ~zero by the
# halfband property and are skipped by the Rust implementation.
pre_center = [h[i] for i in range(center_idx) if i % 2 == 1]

# --- Rust-friendly output ---
rust_coeffs = ", ".join(f"{x:.8f}" for x in pre_center)
print("// Nonzero independent coefficients before center")
print(f"let pre_center: [f32; {len(pre_center)}] = [{rust_coeffs}];")
print(f"let center: f32 = {center};")
