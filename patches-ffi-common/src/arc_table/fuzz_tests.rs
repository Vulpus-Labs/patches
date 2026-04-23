//! Randomised retain/release/drain/grow fuzz for `ArcTable`
//! (ticket 0650, ADR 0045 Spike 9).
//!
//! Complements the multi-threaded soak in `soak_tests.rs` by running a
//! proptest-shrinkable single-threaded op sequence against a shadow
//! model. Per-step invariants are checked, not just end-state; proptest
//! yields a minimal failing sequence on regression.
//!
//! Invariants checked per step:
//!
//! - `live_count` matches the shadow set of undrained, non-zero-refcount
//!   ids.
//! - `drain_released` removes exactly the set of ids whose refcount hit
//!   zero since the previous drain; `live_count` drops by that count.
//! - Post-`grow`, old ids retain/release correctly (via continued op
//!   sequence).
//! - Final sequence (release all + drain): `live_count == 0`, every
//!   minted `Arc<u32>` strong count is 1.
//!
//! Kept single-threaded for model tractability. Loom/Miri coverage of
//! the RCU index swap is a separate path (documented gap in the
//! ticket's AC).

#![cfg(test)]

use std::collections::HashMap;
use std::sync::Arc;

use proptest::prelude::*;

use super::{ArcTable, ArcTableError};

#[derive(Debug, Clone)]
enum Op {
    /// Mint a new id. No-op if the table is exhausted.
    Mint,
    /// Retain the live id at this index (mod live.len()).
    Retain(u16),
    /// Release the live id at this index (mod live.len()).
    Release(u16),
    /// Drain queued releases.
    Drain,
    /// Grow by one chunk.
    Grow,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => Just(Op::Mint),
        2 => any::<u16>().prop_map(Op::Retain),
        6 => any::<u16>().prop_map(Op::Release),
        2 => Just(Op::Drain),
        1 => Just(Op::Grow),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn arc_table_ops_preserve_invariants(ops in prop::collection::vec(op_strategy(), 1..200)) {
        let (mut control, mut audio) = ArcTable::new::<u32>(2);

        // Shadow model.
        let mut arcs: HashMap<u64, Arc<u32>> = HashMap::new();
        let mut refcounts: HashMap<u64, u32> = HashMap::new();
        let mut live: Vec<u64> = Vec::new(); // ids with refcount > 0 or pending drain
        let mut pending_drain: Vec<u64> = Vec::new();
        let mut next_payload: u32 = 0;

        for op in &ops {
            match op {
                Op::Mint => {
                    let payload = Arc::new(next_payload);
                    next_payload += 1;
                    match control.mint(Arc::clone(&payload)) {
                        Ok(id) => {
                            arcs.insert(id, payload);
                            refcounts.insert(id, 1);
                            live.push(id);
                        }
                        Err(ArcTableError::Exhausted) => {
                            // Legal outcome; drop the payload arc.
                        }
                    }
                }
                Op::Retain(idx) => {
                    if live.is_empty() { continue; }
                    let id = live[(*idx as usize) % live.len()];
                    audio.retain(id);
                    *refcounts.get_mut(&id).expect("retain on live id") += 1;
                }
                Op::Release(idx) => {
                    if live.is_empty() { continue; }
                    let i = (*idx as usize) % live.len();
                    let id = live[i];
                    audio.release(id);
                    let rc = refcounts.get_mut(&id).expect("release on live id");
                    *rc -= 1;
                    if *rc == 0 {
                        pending_drain.push(id);
                        live.swap_remove(i);
                    }
                }
                Op::Drain => {
                    let drained = pending_drain.len();
                    let pre = control.live_count();
                    control.drain_released();
                    let post = control.live_count();
                    prop_assert_eq!(
                        pre - post,
                        drained,
                        "drain removed {} ids, model expected {}",
                        pre - post,
                        drained,
                    );
                    for id in pending_drain.drain(..) {
                        let arc = arcs.remove(&id).expect("arc in model");
                        refcounts.remove(&id);
                        // Post-drain, the shadow arc is the sole reference.
                        prop_assert_eq!(Arc::strong_count(&arc), 1);
                    }
                }
                Op::Grow => {
                    let _ = control.grow(1);
                }
            }

            // Invariant: control.live_count() == live.len() + pending_drain.len().
            prop_assert_eq!(
                control.live_count(),
                live.len() + pending_drain.len(),
                "live_count drift: live={}, pending={}",
                live.len(),
                pending_drain.len(),
            );
        }

        // Teardown: release everything, drain, verify zero live + arcs
        // held solely by the model.
        for id in live.drain(..) {
            let rc = refcounts.remove(&id).expect("live id in model");
            for _ in 0..rc {
                audio.release(id);
            }
            pending_drain.push(id);
        }
        control.drain_released();
        prop_assert_eq!(control.live_count(), 0);
        for (_, arc) in arcs.drain() {
            prop_assert_eq!(Arc::strong_count(&arc), 1);
        }
    }
}
