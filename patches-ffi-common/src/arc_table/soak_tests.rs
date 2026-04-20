//! Multi-threaded soak coverage for ADR 0045 spike 2
//! (ticket 0584). The main soak run is `#[ignore]`-gated so the
//! default test suite stays fast; invoke with
//! `cargo test -p patches-ffi-common -- --ignored arc_table_soak`.

#![cfg(test)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use super::runtime::{RuntimeArcTables, RuntimeArcTablesConfig};

fn seeded_rng() -> (u64, SmallRng) {
    let seed = std::env::var("ARC_TABLE_SOAK_SEED")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
        });
    (seed, SmallRng::new(seed))
}

/// Tiny xorshift RNG — no external dep, deterministic per seed.
struct SmallRng(u64);

impl SmallRng {
    fn new(seed: u64) -> Self {
        SmallRng(if seed == 0 { 0x9E3779B97F4A7C15 } else { seed })
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn next_u32(&mut self, bound: u32) -> u32 {
        (self.next() as u32) % bound.max(1)
    }
}

#[test]
fn exhaustion_recovers_after_release() {
    let (mut control, mut audio) = RuntimeArcTables::new(RuntimeArcTablesConfig {
        float_buffers: 8,
        song_data: 1,
    });
    let mut ids = Vec::new();
    for _ in 0..8 {
        let buf: Arc<[f32]> = Arc::from(vec![0.0f32].into_boxed_slice());
        ids.push(control.mint_float_buffer(buf).unwrap());
    }
    assert!(
        control
            .mint_float_buffer(Arc::from(vec![0.0f32].into_boxed_slice()))
            .is_err(),
        "ninth mint must be Exhausted"
    );

    // Release four, drain, confirm we can mint four more.
    for id in ids.drain(..4) {
        audio.release_float_buffer(id);
    }
    control.drain_released();
    for _ in 0..4 {
        let buf: Arc<[f32]> = Arc::from(vec![0.0f32].into_boxed_slice());
        ids.push(control.mint_float_buffer(buf).unwrap());
    }
    assert!(
        control
            .mint_float_buffer(Arc::from(vec![0.0f32].into_boxed_slice()))
            .is_err()
    );
}

#[test]
#[ignore = "slow soak; run with --ignored"]
fn arc_table_soak() {
    let iterations: u64 = std::env::var("ARC_TABLE_SOAK_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);

    let (seed, mut ctl_rng) = seeded_rng();
    eprintln!("arc_table_soak seed = {seed} iterations = {iterations}");

    let (mut control, mut audio) = RuntimeArcTables::new(RuntimeArcTablesConfig {
        float_buffers: 256,
        song_data: 16,
    });

    // Channel of ids from control -> "audio" thread.
    let (tx, rx) = std::sync::mpsc::channel::<u64>();
    let stop = Arc::new(AtomicBool::new(false));

    let audio_stop = Arc::clone(&stop);
    let audio_handle = thread::spawn(move || {
        let mut rng = SmallRng::new(seed ^ 0xA5A5_A5A5_A5A5_A5A5);
        let mut held: Vec<u64> = Vec::with_capacity(256);
        while !audio_stop.load(Ordering::Acquire) || !held.is_empty() {
            while let Ok(id) = rx.try_recv() {
                held.push(id);
            }
            if !held.is_empty() && rng.next() & 1 == 0 {
                let idx = rng.next_u32(held.len() as u32) as usize;
                let id = held.swap_remove(idx);
                // Randomly retain then release an extra time.
                if rng.next() & 0b11 == 0 {
                    audio.retain_float_buffer(
                        crate::ids::FloatBufferId::from_u64_unchecked(id),
                    );
                    audio.release_float_buffer(
                        crate::ids::FloatBufferId::from_u64_unchecked(id),
                    );
                }
                audio.release_float_buffer(
                    crate::ids::FloatBufferId::from_u64_unchecked(id),
                );
            } else {
                thread::sleep(Duration::from_micros(1));
            }
        }
    });

    let mut minted = Vec::<Arc<[f32]>>::new();
    let mut in_flight = 0u32;
    for i in 0..iterations {
        // Drain periodically so slots recycle.
        if i % 64 == 0 {
            control.drain_released();
        }
        if in_flight < 200 && ctl_rng.next() & 0b11 != 0 {
            let payload: Arc<[f32]> =
                Arc::from(vec![(i as f32) * 0.5].into_boxed_slice());
            match control.mint_float_buffer(Arc::clone(&payload)) {
                Ok(id) => {
                    minted.push(payload);
                    tx.send(id.as_u64()).unwrap();
                    in_flight += 1;
                }
                Err(_) => {
                    control.drain_released();
                }
            }
        } else {
            // Let the audio thread catch up.
            thread::yield_now();
        }
        // Track drain so `in_flight` stays loosely coupled; we
        // don't need exact bookkeeping — the post-loop assert is
        // the real check.
        in_flight = in_flight.saturating_sub(1);
    }

    stop.store(true, Ordering::Release);
    drop(tx);
    audio_handle.join().unwrap();

    // Final drain.
    for _ in 0..4 {
        control.drain_released();
        thread::sleep(Duration::from_millis(1));
    }

    assert_eq!(
        control.float_buffer_live_count(),
        0,
        "soak left {} live ids (seed {})",
        control.float_buffer_live_count(),
        seed,
    );
    for payload in &minted {
        assert_eq!(
            Arc::strong_count(payload),
            1,
            "Arc still held after drain (seed {seed})",
        );
    }
}
