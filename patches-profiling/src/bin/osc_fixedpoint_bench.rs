//! Microbenchmark: f32 vs Q32 (fixed-point) phase accumulation for a 16-voice
//! poly oscillator. Measures whether moving the phase accumulator from
//! normalised f32 `[0,1)` to a `u32` Q32 representation (free wrap via integer
//! overflow, hypersaw-style) is a viable, faster substitute.
//!
//! Three scenarios, each comparing f32 baseline against Q32 candidate(s):
//!
//! 1. `wave` — 16 voices at fixed, log-spaced freqs across a broad range
//!    (higher voices wrap more, so BLEP discontinuities hit more often). Run
//!    separately for sine (table), saw (BLEP) and square (BLEP + duty edge).
//!    For saw/square the Q32 side is split into `tier1` (fixed-point
//!    accumulate, *same* branchy polyBLEP) and `tier2` (accumulate +
//!    branchless reciprocal-mul BLEP, hypersaw form) so the saving is
//!    attributable.
//! 2. `pm` — two 16-voice oscillators, the second phase-modulating the first
//!    (sine carrier). Differentiator: Q32 PM offset wraps for free vs f32
//!    `wrap_unit` (x - x.floor()), plus cheaper sine index.
//! 3. `sync` — two 16-voice oscillators, the slave hard-synced to the master
//!    via an f32 fractional index (the real module's reset_out /
//!    process_voice_synced 2-point polyBLEP path). BLEP math held identical
//!    across f32/Q32; only phase rep + accumulate + reset differ.
//!
//! The waveform/BLEP math mirrors `patches-modules` `PolyOsc` so the work shape
//! is representative. Output values are summed and black-boxed to defeat
//! elision; they are not asserted bit-correct (this measures cycles, not audio).
//!
//! Usage: `osc_fixedpoint_bench [wave|pm|sync|all] [n_samples]` (default: all).

use std::env;
use std::hint::black_box;
use std::time::Instant;

use patches_modules::common::approximate::{lookup_sine, lookup_sine_q32};

const SR: f32 = 48_000.0;
const N_VOICES: usize = 16;
/// Q31 → [-1, 1) scale: 2^-31 (saw via signed reinterpret, hypersaw form).
const SAW_SCALE: f32 = 1.0 / 2_147_483_648.0;
/// u32 → [0, 1) scale: 2^-32.
const U32_TO_UNIT: f32 = 1.0 / 4_294_967_296.0;

// ── shared BLEP helper (copy of patches_dsp::polyblep) ───────────────────────

#[inline(always)]
fn polyblep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        2.0 * t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

#[inline(always)]
fn wrap_unit(x: f32) -> f32 {
    x - x.floor()
}

// ── per-voice frequency layout ───────────────────────────────────────────────

/// 16 log-spaced frequencies from 30 Hz to 11 kHz. Higher voices wrap far more
/// often, exercising the BLEP discontinuity paths at a realistic spread.
fn voice_freqs() -> [f32; N_VOICES] {
    let lo = 30.0_f32;
    let hi = 11_000.0_f32;
    std::array::from_fn(|i| {
        let t = i as f32 / (N_VOICES as f32 - 1.0);
        lo * (hi / lo).powf(t)
    })
}

/// f32 increment (cycles/sample), clamped below 0.5 to keep polyBLEP valid.
fn inc_f32(freqs: &[f32; N_VOICES]) -> [f32; N_VOICES] {
    std::array::from_fn(|i| (freqs[i] / SR).min(0.49))
}

/// Q32 increment, plus its f32 reciprocal (the branchless-BLEP fraction
/// normaliser, computed at control rate exactly as hypersaw does).
fn inc_q32(freqs: &[f32; N_VOICES]) -> ([u32; N_VOICES], [f32; N_VOICES]) {
    let mut inc = [0u32; N_VOICES];
    let mut inv = [0.0f32; N_VOICES];
    for i in 0..N_VOICES {
        let frac = (freqs[i] / SR).min(0.49);
        inc[i] = (frac * 4_294_967_296.0) as u32;
        inv[i] = if inc[i] > 0 { 1.0 / inc[i] as f32 } else { 0.0 };
    }
    (inc, inv)
}

// ── timing harness ───────────────────────────────────────────────────────────

/// Run `step` (one sample = 16 voices, returns a scalar to consume) for `n`
/// samples after `warmup`, returning per-sample ns (batched to amortise the
/// clock read).
fn time_loop(mut step: impl FnMut() -> f32, n: usize, warmup: usize) -> Vec<u64> {
    for _ in 0..warmup {
        black_box(step());
    }
    const BATCH: usize = 256;
    let batches = n / BATCH;
    let mut per: Vec<u64> = Vec::with_capacity(batches);
    for _ in 0..batches {
        let t0 = Instant::now();
        let mut acc = 0.0f32;
        for _ in 0..BATCH {
            acc += step();
        }
        black_box(acc);
        per.push(t0.elapsed().as_nanos() as u64 / BATCH as u64);
    }
    per
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

struct Stat {
    label: String,
    mean: f64,
    p50: u64,
    p90: u64,
    p99: u64,
}

fn summarize(label: &str, mut per: Vec<u64>) -> Stat {
    per.sort_unstable();
    let sum: u128 = per.iter().map(|&x| x as u128).sum();
    Stat {
        label: label.to_string(),
        mean: sum as f64 / per.len() as f64,
        p50: percentile(&per, 0.50),
        p90: percentile(&per, 0.90),
        p99: percentile(&per, 0.99),
    }
}

fn print_group(title: &str, stats: &[Stat]) {
    println!("\n{title}");
    let baseline = stats.first().map(|s| s.mean).unwrap_or(1.0);
    println!(
        "  {:<22} {:>9} {:>7} {:>7} {:>7}   {:>8}",
        "variant", "mean(ns)", "p50", "p90", "p99", "vs f32"
    );
    for s in stats {
        let rel = s.mean / baseline;
        println!(
            "  {:<22} {:>9.2} {:>7} {:>7} {:>7}   {:>7.1}%",
            s.label,
            s.mean,
            s.p50,
            s.p90,
            s.p99,
            rel * 100.0
        );
    }
}

// ── scenario 1: single 16-voice oscillator, per waveform ─────────────────────

fn bench_sine(n: usize, warmup: usize) -> Vec<Stat> {
    let freqs = voice_freqs();
    let inc_f = inc_f32(&freqs);
    let (inc_u, _) = inc_q32(&freqs);

    let mut ph_f = [0.0f32; N_VOICES];
    let f32_stat = summarize(
        "f32",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    acc += lookup_sine(ph_f[v]);
                    let next = ph_f[v] + inc_f[v];
                    ph_f[v] = if next >= 1.0 { next - 1.0 } else { next };
                }
                acc
            },
            n,
            warmup,
        ),
    );

    let mut ph_u = [0u32; N_VOICES];
    let q32_stat = summarize(
        "Q32",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    acc += lookup_sine_q32(ph_u[v]);
                    ph_u[v] = ph_u[v].wrapping_add(inc_u[v]);
                }
                acc
            },
            n,
            warmup,
        ),
    );

    vec![f32_stat, q32_stat]
}

fn bench_saw(n: usize, warmup: usize) -> Vec<Stat> {
    let freqs = voice_freqs();
    let inc_f = inc_f32(&freqs);
    let (inc_u, inv_u) = inc_q32(&freqs);

    // f32 baseline: value at current phase, then advance.
    let mut ph_f = [0.0f32; N_VOICES];
    let f32_stat = summarize(
        "f32",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    let p = ph_f[v];
                    acc += (2.0 * p - 1.0) - polyblep(p, inc_f[v]);
                    let next = p + inc_f[v];
                    ph_f[v] = if next >= 1.0 { next - 1.0 } else { next };
                }
                acc
            },
            n,
            warmup,
        ),
    );

    // Q32 tier1: fixed-point accumulate, but the *same* branchy polyBLEP fed an
    // f32 phase/dt derived from the u32 state. Isolates the accumulate saving.
    let mut ph_u = [0u32; N_VOICES];
    let t1_stat = summarize(
        "Q32 tier1 (same blep)",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    let p01 = ph_u[v] as f32 * U32_TO_UNIT;
                    let dt = inc_u[v] as f32 * U32_TO_UNIT;
                    acc += (2.0 * p01 - 1.0) - polyblep(p01, dt);
                    ph_u[v] = ph_u[v].wrapping_add(inc_u[v]);
                }
                acc
            },
            n,
            warmup,
        ),
    );

    // Q32 tier2: branchless reciprocal-mul BLEP, hypersaw form (single quadratic,
    // unsigned-compare zone tests, free wrap).
    let mut ph_u2 = [0u32; N_VOICES];
    let t2_stat = summarize(
        "Q32 tier2 (branchless)",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    let p = ph_u2[v];
                    let inc_v = inc_u[v];
                    let naive = (p.wrapping_sub(0x8000_0000) as i32) as f32 * SAW_SCALE;
                    let after = p < inc_v;
                    let before = !after & (p.wrapping_neg() < inc_v);
                    let local = if after { p } else { p.wrapping_neg() } as f32;
                    let frac = local * inv_u[v];
                    let om = 1.0 - frac;
                    let sq = om * om;
                    let sign = (before as u32 as f32) - (after as u32 as f32);
                    acc += naive - sign * sq;
                    ph_u2[v] = p.wrapping_add(inc_v);
                }
                acc
            },
            n,
            warmup,
        ),
    );

    vec![f32_stat, t1_stat, t2_stat]
}

fn bench_square(n: usize, warmup: usize) -> Vec<Stat> {
    let freqs = voice_freqs();
    let inc_f = inc_f32(&freqs);
    let (inc_u, inv_u) = inc_q32(&freqs);
    let duty = 0.5_f32;
    let duty_u = (duty * 4_294_967_296.0) as u32;

    let mut ph_f = [0.0f32; N_VOICES];
    let f32_stat = summarize(
        "f32",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    let p = ph_f[v];
                    let dt = inc_f[v];
                    let raw = if p < duty { 1.0 } else { -1.0 };
                    let we = polyblep(p, dt);
                    let de = polyblep((p - duty).rem_euclid(1.0), dt);
                    acc += raw + we - de;
                    let next = p + dt;
                    ph_f[v] = if next >= 1.0 { next - 1.0 } else { next };
                }
                acc
            },
            n,
            warmup,
        ),
    );

    let mut ph_u = [0u32; N_VOICES];
    let t1_stat = summarize(
        "Q32 tier1 (same blep)",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    let p01 = ph_u[v] as f32 * U32_TO_UNIT;
                    let dt = inc_u[v] as f32 * U32_TO_UNIT;
                    let raw = if p01 < duty { 1.0 } else { -1.0 };
                    let we = polyblep(p01, dt);
                    let de = polyblep((p01 - duty).rem_euclid(1.0), dt);
                    acc += raw + we - de;
                    ph_u[v] = ph_u[v].wrapping_add(inc_u[v]);
                }
                acc
            },
            n,
            warmup,
        ),
    );

    // tier2: both the wrap edge (phase == 0) and the duty edge (phase == duty_u)
    // detected by unsigned compares; the duty edge uses a free wrapping_sub
    // instead of f32 rem_euclid.
    let mut ph_u2 = [0u32; N_VOICES];
    let t2_stat = summarize(
        "Q32 tier2 (branchless)",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    let p = ph_u2[v];
                    let inc_v = inc_u[v];
                    let inv = inv_u[v];
                    let raw = if p < duty_u { 1.0 } else { -1.0 };

                    // wrap edge at phase == 0
                    let after = p < inc_v;
                    let before = !after & (p.wrapping_neg() < inc_v);
                    let local = if after { p } else { p.wrapping_neg() } as f32;
                    let f0 = local * inv;
                    let om0 = 1.0 - f0;
                    let sq0 = om0 * om0;
                    let s0 = (after as u32 as f32) - (before as u32 as f32);

                    // duty edge at phase == duty_u (free shift by wrapping_sub)
                    let d = p.wrapping_sub(duty_u);
                    let after_d = d < inc_v;
                    let before_d = !after_d & (d.wrapping_neg() < inc_v);
                    let local_d = if after_d { d } else { d.wrapping_neg() } as f32;
                    let fd = local_d * inv;
                    let omd = 1.0 - fd;
                    let sqd = omd * omd;
                    let sd = (after_d as u32 as f32) - (before_d as u32 as f32);

                    acc += raw + s0 * sq0 - sd * sqd;
                    ph_u2[v] = p.wrapping_add(inc_v);
                }
                acc
            },
            n,
            warmup,
        ),
    );

    vec![f32_stat, t1_stat, t2_stat]
}

// ── scenario 2: phase modulation (carrier + modulator) ───────────────────────

fn bench_pm(n: usize, warmup: usize) -> Vec<Stat> {
    let car = voice_freqs();
    // Modulator at ~2× carrier per voice (a representative ratio).
    let modf: [f32; N_VOICES] = std::array::from_fn(|i| car[i] * 2.0);
    let depth = 0.3_f32;

    let inc_car_f = inc_f32(&car);
    let inc_mod_f = inc_f32(&modf);
    let (inc_car_u, _) = inc_q32(&car);
    let (inc_mod_u, _) = inc_q32(&modf);

    let mut cph_f = [0.0f32; N_VOICES];
    let mut mph_f = [0.0f32; N_VOICES];
    let f32_stat = summarize(
        "f32",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    let m = lookup_sine(mph_f[v]) * depth;
                    acc += lookup_sine(wrap_unit(cph_f[v] + m));
                    let nc = cph_f[v] + inc_car_f[v];
                    cph_f[v] = if nc >= 1.0 { nc - 1.0 } else { nc };
                    let nm = mph_f[v] + inc_mod_f[v];
                    mph_f[v] = if nm >= 1.0 { nm - 1.0 } else { nm };
                }
                acc
            },
            n,
            warmup,
        ),
    );

    let mut cph_u = [0u32; N_VOICES];
    let mut mph_u = [0u32; N_VOICES];
    let q32_stat = summarize(
        "Q32",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    let m = lookup_sine_q32(mph_u[v]) * depth;
                    // PM offset wraps for free; no wrap_unit / floor.
                    let off = (m * 4_294_967_296.0) as i64 as u32;
                    acc += lookup_sine_q32(cph_u[v].wrapping_add(off));
                    cph_u[v] = cph_u[v].wrapping_add(inc_car_u[v]);
                    mph_u[v] = mph_u[v].wrapping_add(inc_mod_u[v]);
                }
                acc
            },
            n,
            warmup,
        ),
    );

    vec![f32_stat, q32_stat]
}

// ── scenario 2b: PolyOp 2-op FM (self-feedback modulator → carrier) ──────────

/// Faithful PolyOp operator inner loop: phase + (pm + 2-sample rolling-avg
/// feedback), `rem_euclid` wrap, `lookup_sine` (the `op_waveform` Sine case).
/// A self-feedback modulator drives a carrier — the ubiquitous 2-op FM cell.
/// The ADSR is omitted (port-invariant: identical cost on both sides), so this
/// isolates the phase/PM/feedback/lookup work the Q32 port actually changes.
fn bench_op(n: usize, warmup: usize) -> Vec<Stat> {
    let car = voice_freqs();
    let modf: [f32; N_VOICES] = std::array::from_fn(|i| car[i] * 2.0);
    let index = 2.0_f32;
    let fb_amt = 0.5_f32;

    let inc_car_f = inc_f32(&car);
    let inc_mod_f = inc_f32(&modf);
    let (inc_car_u, _) = inc_q32(&car);
    let (inc_mod_u, _) = inc_q32(&modf);

    let mut cph_f = [0.0f32; N_VOICES];
    let mut mph_f = [0.0f32; N_VOICES];
    let mut mfb_z1 = [0.0f32; N_VOICES];
    let mut mprev = [0.0f32; N_VOICES];
    let f32_stat = summarize(
        "f32",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    // self-feedback modulator
                    let fb = mprev[v] * fb_amt;
                    let fb_avg = (fb + mfb_z1[v]) * 0.5;
                    mfb_z1[v] = fb;
                    let mread = (mph_f[v] + fb_avg).rem_euclid(1.0);
                    let mout = lookup_sine(mread);
                    mprev[v] = mout;
                    let nm = mph_f[v] + inc_mod_f[v];
                    mph_f[v] = if nm >= 1.0 { nm - 1.0 } else { nm };
                    // carrier modulated by the operator
                    let pm = mout * index;
                    let cread = (cph_f[v] + pm).rem_euclid(1.0);
                    acc += lookup_sine(cread);
                    let nc = cph_f[v] + inc_car_f[v];
                    cph_f[v] = if nc >= 1.0 { nc - 1.0 } else { nc };
                }
                acc
            },
            n,
            warmup,
        ),
    );

    let mut cph_u = [0u32; N_VOICES];
    let mut mph_u = [0u32; N_VOICES];
    let mut mfb_z1q = [0.0f32; N_VOICES];
    let mut mprevq = [0.0f32; N_VOICES];
    let q32_stat = summarize(
        "Q32",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    let fb = mprevq[v] * fb_amt;
                    let fb_avg = (fb + mfb_z1q[v]) * 0.5;
                    mfb_z1q[v] = fb;
                    // feedback offset wraps for free; no rem_euclid.
                    let fb_off = (fb_avg * 4_294_967_296.0) as i64 as u32;
                    let mout = lookup_sine_q32(mph_u[v].wrapping_add(fb_off));
                    mprevq[v] = mout;
                    mph_u[v] = mph_u[v].wrapping_add(inc_mod_u[v]);
                    let pm = mout * index;
                    let pm_off = (pm * 4_294_967_296.0) as i64 as u32;
                    acc += lookup_sine_q32(cph_u[v].wrapping_add(pm_off));
                    cph_u[v] = cph_u[v].wrapping_add(inc_car_u[v]);
                }
                acc
            },
            n,
            warmup,
        ),
    );

    vec![f32_stat, q32_stat]
}

// ── scenario 3: hard sync (master → slave) via f32 fractional index ──────────

fn bench_sync(n: usize, warmup: usize) -> Vec<Stat> {
    // Master ~0.38× the slave so the slave is synced mid-cycle, partial wraps.
    let slave = voice_freqs();
    let master: [f32; N_VOICES] = std::array::from_fn(|i| slave[i] * 0.38);

    let inc_s_f = inc_f32(&slave);
    let inc_m_f = inc_f32(&master);
    let (inc_s_u, _) = inc_q32(&slave);
    let (inc_m_u, inv_m_u) = inc_q32(&master);

    // f32 variant: master + slave both f32; slave runs the module's deferred
    // 2-point polyBLEP synced-saw path.
    let mut mph_f = [0.0f32; N_VOICES];
    let mut sph_f = [0.0f32; N_VOICES];
    let mut pending = [false; N_VOICES];
    let mut pend_blep = [0.0f32; N_VOICES];
    let f32_stat = summarize(
        "f32",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    // Master advance → reset_out frac.
                    let mnext = mph_f[v] + inc_m_f[v];
                    let frac = if mnext >= 1.0 {
                        mph_f[v] = mnext - 1.0;
                        (1.0 - mph_f[v] / inc_m_f[v]).clamp(f32::MIN_POSITIVE, 1.0)
                    } else {
                        mph_f[v] = mnext;
                        0.0
                    };
                    let dt = inc_s_f[v];
                    if frac > 0.0 {
                        let cur = sph_f[v];
                        let read = wrap_unit(cur);
                        let mut reset_raw = cur + frac * dt;
                        if reset_raw >= 1.0 {
                            reset_raw -= 1.0;
                        }
                        let reset_read = wrap_unit(reset_raw);
                        sph_f[v] = (1.0 - frac) * dt;
                        let post_raw = sph_f[v];
                        let post_read = wrap_unit(post_raw);
                        let before = polyblep(1.0 - frac * dt, dt);
                        let after = polyblep(post_raw, dt);
                        let delta = (2.0 * reset_read - 1.0) - (2.0 * post_read - 1.0);
                        let wrap = if pending[v] {
                            pend_blep[v]
                        } else {
                            -polyblep(read, dt)
                        };
                        acc += (2.0 * read - 1.0) + wrap - before * 0.5 * delta;
                        pend_blep[v] = -after * 0.5 * delta;
                        pending[v] = true;
                    } else {
                        let p = wrap_unit(sph_f[v]);
                        let wrap = if pending[v] {
                            pend_blep[v]
                        } else {
                            -polyblep(p, dt)
                        };
                        acc += (2.0 * p - 1.0) + wrap;
                        pending[v] = false;
                        let next = sph_f[v] + dt;
                        sph_f[v] = if next >= 1.0 { next - 1.0 } else { next };
                    }
                }
                acc
            },
            n,
            warmup,
        ),
    );

    // Q32 variant: master + slave phases are u32; the sync frac crossing the
    // boundary is still f32 (the module's interface). BLEP math identical to f32.
    let mut mph_u = [0u32; N_VOICES];
    let mut sph_u = [0u32; N_VOICES];
    let mut pending2 = [false; N_VOICES];
    let mut pend_blep2 = [0.0f32; N_VOICES];
    let q32_stat = summarize(
        "Q32",
        time_loop(
            || {
                let mut acc = 0.0;
                for v in 0..N_VOICES {
                    // Master advance (free wrap) → reset_out frac in f32.
                    let before_add = mph_u[v];
                    let p = before_add.wrapping_add(inc_m_u[v]);
                    mph_u[v] = p;
                    let frac = if p < inc_m_u[v] {
                        // wrapped this tick
                        (1.0 - (p as f32) * inv_m_u[v]).clamp(f32::MIN_POSITIVE, 1.0)
                    } else {
                        0.0
                    };
                    let dt = inc_s_u[v] as f32 * U32_TO_UNIT;
                    if frac > 0.0 {
                        let cur = sph_u[v] as f32 * U32_TO_UNIT;
                        let read = wrap_unit(cur);
                        let mut reset_raw = cur + frac * dt;
                        if reset_raw >= 1.0 {
                            reset_raw -= 1.0;
                        }
                        let reset_read = wrap_unit(reset_raw);
                        // sync_reset in fixed point.
                        sph_u[v] = ((1.0 - frac) * inc_s_u[v] as f32) as u32;
                        let post_raw = sph_u[v] as f32 * U32_TO_UNIT;
                        let post_read = wrap_unit(post_raw);
                        let before_b = polyblep(1.0 - frac * dt, dt);
                        let after_b = polyblep(post_raw, dt);
                        let delta = (2.0 * reset_read - 1.0) - (2.0 * post_read - 1.0);
                        let wrap = if pending2[v] {
                            pend_blep2[v]
                        } else {
                            -polyblep(read, dt)
                        };
                        acc += (2.0 * read - 1.0) + wrap - before_b * 0.5 * delta;
                        pend_blep2[v] = -after_b * 0.5 * delta;
                        pending2[v] = true;
                    } else {
                        let pp = sph_u[v] as f32 * U32_TO_UNIT;
                        let wrap = if pending2[v] {
                            pend_blep2[v]
                        } else {
                            -polyblep(pp, dt)
                        };
                        acc += (2.0 * pp - 1.0) + wrap;
                        pending2[v] = false;
                        sph_u[v] = sph_u[v].wrapping_add(inc_s_u[v]);
                    }
                }
                acc
            },
            n,
            warmup,
        ),
    );

    vec![f32_stat, q32_stat]
}

fn main() {
    let _ftz = patches_engine::FtzGuard::enable();
    let mut args = env::args().skip(1);
    let which = args.next().unwrap_or_else(|| "all".to_string());
    let n: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(4_000_000);
    let warmup = (SR as usize) / 2;

    println!("PolyOsc f32 vs Q32 phase accumulation — n={n} samples/voice-set, {N_VOICES} voices, sr={SR} Hz");
    println!("(per-tick ns = one sample across all {N_VOICES} voices)");

    let run_wave = which == "all" || which == "wave";
    let run_pm = which == "all" || which == "pm";
    let run_op = which == "all" || which == "op";
    let run_sync = which == "all" || which == "sync";

    if run_wave {
        print_group("[1] sine — 16 voices, 30 Hz..11 kHz", &bench_sine(n, warmup));
        print_group("[1] saw  — 16 voices, 30 Hz..11 kHz", &bench_saw(n, warmup));
        print_group("[1] square — 16 voices, 30 Hz..11 kHz", &bench_square(n, warmup));
    }
    if run_pm {
        print_group("[2] phase-mod (sine carrier + 2x modulator)", &bench_pm(n, warmup));
    }
    if run_op {
        print_group("[2b] PolyOp 2-op FM (self-fb modulator -> carrier)", &bench_op(n, warmup));
    }
    if run_sync {
        print_group("[3] hard-sync saw (master 0.38x slave, f32 frac)", &bench_sync(n, warmup));
    }
}
