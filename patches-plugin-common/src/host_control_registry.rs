//! Name → CLAP parameter ID registry for host controls (ADR 0057 §6,
//! ticket 0811).
//!
//! This module is host-toolkit-agnostic and does not link CLAP. It owns
//! the lifecycle bookkeeping the CLAP plugin needs to expose host
//! controls as automatable DAW parameters with stable IDs across hot
//! reloads:
//!
//! - **Live** entries: a host control declared in the current manifest.
//!   Has a CLAP parameter ID and a backplane slot (taken from the
//!   manifest's alphabetical layout — slot ownership is the planner's,
//!   not this registry's).
//! - **Tombstoned** entries: a host control that *was* live and has
//!   since been removed. Retains its CLAP parameter ID for the rest of
//!   the session so DAW automation lanes referencing it are not
//!   silently rebound to a different control. Slotless.
//! - **Reborn** entries: a tombstoned name that re-appears in a new
//!   manifest. Reclaims its old ID; gets a new slot.
//!
//! `apply` is the single state-transition entry point. It diffs the
//! incoming manifest against the live + tombstoned sets and returns a
//! [`DiffOutcome`] describing what the CLAP layer must do (which
//! `RESCAN_*` flag to send the host, which IDs were freshly minted,
//! which were tombstoned, etc.).
//!
//! The registry is single-threaded: it lives on the plugin's main /
//! control thread. Audio side never reads it; the CLAP layer ships a
//! per-plan `(param_id → slot)` snapshot to the audio thread separately.

use std::collections::{BTreeMap, BTreeSet};

use patches_dsl::host_control_manifest::{
    HostControlDescriptor, HostControlKind, HostControlManifest, HostControlParamMap,
};

/// CLAP parameter identifier. CLAP itself uses `clap_id = u32`.
pub type ParamId = u32;

/// One currently-published host control.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveEntry {
    pub id: ParamId,
    pub kind: HostControlKind,
    /// Backplane slot taken from the manifest's alphabetical layout.
    pub slot: usize,
    /// Field map (`low`, `high`, `default`, …) verbatim from the
    /// declaration. Compared structurally to detect param-info changes.
    pub params: HostControlParamMap,
    /// Last value observed on this control. Updated by
    /// [`HostControlRegistry::record_value`]; preserved across replans
    /// when the entry stays live, copied into the tombstone on remove.
    pub last_value: f32,
}

/// One historically-live host control whose name has been removed from
/// the active manifest. Retains the ID so the DAW's automation lanes
/// don't silently rebind.
#[derive(Debug, Clone, PartialEq)]
pub struct Tombstone {
    pub id: ParamId,
    pub kind: HostControlKind,
    pub last_value: f32,
    /// Apply-call epoch at which this name was tombstoned. Used as the
    /// LRU eviction key when the tombstone table is full.
    pub epoch: u64,
}

/// Required CLAP rescan flag for the diff. Coalesced over the whole
/// `apply` — if any change demands `All`, the whole apply is `All`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RescanLevel {
    #[default]
    None,
    /// `CLAP_PARAM_RESCAN_INFO`: range / default / display-name change
    /// on a stable ID set.
    Info,
    /// `CLAP_PARAM_RESCAN_ALL`: parameter set changed (add / remove /
    /// kind change). Heavy on the host side.
    All,
}

/// Outcome of one `apply` call. The registry's internal state has
/// already been mutated; this struct is the description of what must be
/// communicated to the CLAP host afterwards.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiffOutcome {
    pub rescan: RescanLevel,
    /// Names that entered `live` for the first time in this session.
    pub added: Vec<String>,
    /// Names moved from `live` to `tombstones`.
    pub removed: Vec<String>,
    /// Names that re-entered `live` from the tombstone set, reusing
    /// their old ID.
    pub reborn: Vec<String>,
    /// Names whose kind changed: old ID was tombstoned, new ID was
    /// minted.
    pub kind_changed: Vec<String>,
    /// Names whose params (`low` / `high` / `default` / …) changed but
    /// kind and ID are stable.
    pub info_changed: Vec<String>,
    /// Tombstones evicted to make room for new ones (LRU by epoch).
    pub evicted_tombstones: Vec<String>,
}

impl RescanLevel {
    fn promote(&mut self, other: RescanLevel) {
        if other.rank() > self.rank() {
            *self = other;
        }
    }
    fn rank(self) -> u8 {
        match self {
            RescanLevel::None => 0,
            RescanLevel::Info => 1,
            RescanLevel::All => 2,
        }
    }
}

/// Trait surface for the registry. The CLAP plugin holds one of these
/// behind a `Box<dyn HostControlRegistry>` so the plugin glue can be
/// tested with a stub if needed; the production impl is
/// [`StandardHostControlRegistry`].
pub trait HostControlRegistry {
    /// Diff `manifest` against current live + tombstone sets and apply
    /// the resulting state changes. Returns the description of what
    /// changed.
    fn apply(&mut self, manifest: &HostControlManifest) -> DiffOutcome;

    fn live(&self, name: &str) -> Option<&LiveEntry>;
    fn tombstone(&self, name: &str) -> Option<&Tombstone>;
    fn id_of(&self, name: &str) -> Option<ParamId>;

    /// Map every live name's ID to its current backplane slot. Output
    /// is sorted by ID for caller convenience; suitable for shipping
    /// to the audio thread inside a plan.
    fn id_to_slot(&self) -> Vec<(ParamId, usize)>;

    /// Update the cached `last_value` for the live entry holding `id`.
    /// No-op if `id` isn't currently live.
    fn record_value(&mut self, id: ParamId, value: f32);

    fn live_len(&self) -> usize;
    fn tombstone_len(&self) -> usize;
    fn next_id(&self) -> ParamId;
}

/// Production registry. Owns two `BTreeMap`s for deterministic iteration
/// order and an apply-epoch counter for LRU eviction.
pub struct StandardHostControlRegistry {
    live: BTreeMap<String, LiveEntry>,
    tombstones: BTreeMap<String, Tombstone>,
    next_id: ParamId,
    epoch: u64,
    max_tombstones: usize,
}

impl StandardHostControlRegistry {
    /// Create an empty registry. `max_tombstones` caps the historic-
    /// removed table; ADR 0057 §6 ties this to the host-control
    /// backplane capacity (64).
    pub fn new(max_tombstones: usize) -> Self {
        Self {
            live: BTreeMap::new(),
            tombstones: BTreeMap::new(),
            next_id: 0,
            epoch: 0,
            max_tombstones,
        }
    }

    fn allocate_id(&mut self) -> ParamId {
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).expect("ParamId space exhausted");
        id
    }

    fn evict_tombstones_if_full(&mut self, room_needed: usize, evicted: &mut Vec<String>) {
        while self.tombstones.len() + room_needed > self.max_tombstones
            && !self.tombstones.is_empty()
        {
            // Pick the oldest entry by epoch (ties broken by name).
            let victim = self
                .tombstones
                .iter()
                .min_by_key(|(name, t)| (t.epoch, (*name).clone()))
                .map(|(name, _)| name.clone())
                .expect("non-empty checked");
            self.tombstones.remove(&victim);
            evicted.push(victim);
        }
    }
}

impl HostControlRegistry for StandardHostControlRegistry {
    fn apply(&mut self, manifest: &HostControlManifest) -> DiffOutcome {
        self.epoch = self.epoch.wrapping_add(1);
        let mut out = DiffOutcome::default();

        let new_names: BTreeSet<&str> = manifest.iter().map(|d| d.name.as_str()).collect();

        // 1. Removals: live names not in the new manifest become
        //    tombstones. Collect first, mutate after, to avoid borrow
        //    juggling.
        let to_remove: Vec<String> = self
            .live
            .keys()
            .filter(|n| !new_names.contains(n.as_str()))
            .cloned()
            .collect();

        if !to_remove.is_empty() {
            self.evict_tombstones_if_full(to_remove.len(), &mut out.evicted_tombstones);
            for name in &to_remove {
                let entry = self.live.remove(name).expect("live");
                self.tombstones.insert(
                    name.clone(),
                    Tombstone {
                        id: entry.id,
                        kind: entry.kind,
                        last_value: entry.last_value,
                        epoch: self.epoch,
                    },
                );
            }
            out.removed = to_remove;
            out.rescan.promote(RescanLevel::All);
        }

        // 2. Walk the new manifest in order so freshly-minted IDs get
        //    a deterministic order.
        for desc in manifest {
            self.apply_descriptor(desc, &mut out);
        }

        out
    }

    fn live(&self, name: &str) -> Option<&LiveEntry> { self.live.get(name) }
    fn tombstone(&self, name: &str) -> Option<&Tombstone> { self.tombstones.get(name) }

    fn id_of(&self, name: &str) -> Option<ParamId> {
        self.live.get(name).map(|e| e.id).or_else(|| self.tombstones.get(name).map(|t| t.id))
    }

    fn id_to_slot(&self) -> Vec<(ParamId, usize)> {
        let mut v: Vec<(ParamId, usize)> = self.live.values().map(|e| (e.id, e.slot)).collect();
        v.sort_by_key(|(id, _)| *id);
        v
    }

    fn record_value(&mut self, id: ParamId, value: f32) {
        if let Some(e) = self.live.values_mut().find(|e| e.id == id) {
            e.last_value = value;
        }
    }

    fn live_len(&self) -> usize { self.live.len() }
    fn tombstone_len(&self) -> usize { self.tombstones.len() }
    fn next_id(&self) -> ParamId { self.next_id }
}

impl StandardHostControlRegistry {
    /// Snapshot of live entries sorted by ParamId. Used by the CLAP
    /// `params` extension's `count` / `get_info` callbacks: indexing
    /// over this slice gives the host a stable parameter order.
    pub fn live_sorted_by_id(&self) -> Vec<(&str, &LiveEntry)> {
        let mut v: Vec<(&str, &LiveEntry)> = self
            .live
            .iter()
            .map(|(n, e)| (n.as_str(), e))
            .collect();
        v.sort_by_key(|(_, e)| e.id);
        v
    }

    /// Look up a live entry by its CLAP parameter id.
    pub fn live_by_id(&self, id: ParamId) -> Option<(&str, &LiveEntry)> {
        self.live.iter().find_map(|(n, e)| {
            if e.id == id { Some((n.as_str(), e)) } else { None }
        })
    }
}

impl StandardHostControlRegistry {
    fn apply_descriptor(&mut self, desc: &HostControlDescriptor, out: &mut DiffOutcome) {
        let name = &desc.name;

        // Case A: name currently live.
        if let Some(existing) = self.live.get_mut(name) {
            if existing.kind != desc.kind {
                // Kind change: tombstone the old ID, mint a fresh one.
                let old = existing.clone();
                self.evict_tombstones_if_full(1, &mut out.evicted_tombstones);
                self.tombstones.insert(
                    name.clone(),
                    Tombstone {
                        id: old.id,
                        kind: old.kind,
                        last_value: old.last_value,
                        epoch: self.epoch,
                    },
                );
                let new_id = self.allocate_id();
                self.live.insert(
                    name.clone(),
                    LiveEntry {
                        id: new_id,
                        kind: desc.kind,
                        slot: desc.slot,
                        params: desc.params.clone(),
                        last_value: 0.0,
                    },
                );
                out.kind_changed.push(name.clone());
                out.rescan.promote(RescanLevel::All);
                return;
            }
            // Kind stable. Update slot (always — alphabetical re-layout
            // is allowed without rescan).
            existing.slot = desc.slot;
            if existing.params != desc.params {
                existing.params = desc.params.clone();
                out.info_changed.push(name.clone());
                out.rescan.promote(RescanLevel::Info);
            }
            return;
        }

        // Case B: name was tombstoned — rebirth.
        if let Some(t) = self.tombstones.remove(name) {
            // If kind has changed since the tombstone, treat as a
            // fresh add: keep the old tombstone (with its old ID),
            // mint a new ID. We re-insert the tombstone and fall
            // through to the fresh-add branch.
            if t.kind != desc.kind {
                self.evict_tombstones_if_full(1, &mut out.evicted_tombstones);
                self.tombstones.insert(name.clone(), t);
                let new_id = self.allocate_id();
                self.live.insert(
                    name.clone(),
                    LiveEntry {
                        id: new_id,
                        kind: desc.kind,
                        slot: desc.slot,
                        params: desc.params.clone(),
                        last_value: 0.0,
                    },
                );
                out.added.push(name.clone());
                out.rescan.promote(RescanLevel::All);
                return;
            }
            // Same kind: rebirth with the old ID and last_value.
            self.live.insert(
                name.clone(),
                LiveEntry {
                    id: t.id,
                    kind: desc.kind,
                    slot: desc.slot,
                    params: desc.params.clone(),
                    last_value: t.last_value,
                },
            );
            out.reborn.push(name.clone());
            out.rescan.promote(RescanLevel::All);
            return;
        }

        // Case C: brand new name.
        let id = self.allocate_id();
        self.live.insert(
            name.clone(),
            LiveEntry {
                id,
                kind: desc.kind,
                slot: desc.slot,
                params: desc.params.clone(),
                last_value: 0.0,
            },
        );
        out.added.push(name.clone());
        out.rescan.promote(RescanLevel::All);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{provenance::Provenance, source_span::Span};
    use patches_dsl::host_control_manifest::{HostControlParamValue, HostControlParamMap};

    fn span() -> Span { Span::synthetic() }

    fn desc(slot: usize, name: &str, kind: HostControlKind) -> HostControlDescriptor {
        HostControlDescriptor {
            slot,
            name: name.into(),
            kind,
            params: HostControlParamMap::new(),
            source: Provenance::root(span()),
        }
    }

    fn desc_with(
        slot: usize,
        name: &str,
        kind: HostControlKind,
        params: &[(&str, HostControlParamValue)],
    ) -> HostControlDescriptor {
        let mut map = HostControlParamMap::new();
        for (k, v) in params {
            map.insert((*k).into(), v.clone());
        }
        HostControlDescriptor {
            slot,
            name: name.into(),
            kind,
            params: map,
            source: Provenance::root(span()),
        }
    }

    fn manifest_alpha(decls: Vec<HostControlDescriptor>) -> HostControlManifest {
        // Mirror the planner: alphabetical, slot = sort index.
        let mut v = decls;
        v.sort_by(|a, b| a.name.cmp(&b.name));
        for (i, d) in v.iter_mut().enumerate() {
            d.slot = i;
        }
        v
    }

    #[test]
    fn empty_apply_is_noop() {
        let mut r = StandardHostControlRegistry::new(64);
        let out = r.apply(&Vec::new());
        assert_eq!(out.rescan, RescanLevel::None);
        assert!(out.added.is_empty() && out.removed.is_empty());
        assert_eq!(r.live_len(), 0);
        assert_eq!(r.next_id(), 0);
    }

    #[test]
    fn first_manifest_adds_all_with_fresh_ids() {
        let mut r = StandardHostControlRegistry::new(64);
        let m = manifest_alpha(vec![
            desc(0, "cutoff", HostControlKind::Knob),
            desc(0, "fire", HostControlKind::Trigger),
        ]);
        let out = r.apply(&m);
        assert_eq!(out.rescan, RescanLevel::All);
        assert_eq!(out.added, vec!["cutoff".to_string(), "fire".to_string()]);
        assert_eq!(r.id_of("cutoff"), Some(0));
        assert_eq!(r.id_of("fire"), Some(1));
        assert_eq!(r.live("cutoff").unwrap().slot, 0);
        assert_eq!(r.live("fire").unwrap().slot, 1);
        assert_eq!(r.next_id(), 2);
    }

    #[test]
    fn no_change_is_rescan_none() {
        let mut r = StandardHostControlRegistry::new(64);
        let m = manifest_alpha(vec![desc(0, "k", HostControlKind::Knob)]);
        r.apply(&m);
        let out = r.apply(&m);
        assert_eq!(out.rescan, RescanLevel::None);
        assert!(out.added.is_empty() && out.removed.is_empty() && out.info_changed.is_empty());
    }

    #[test]
    fn add_preserves_existing_ids_and_shifts_slots() {
        let mut r = StandardHostControlRegistry::new(64);
        // First: bbb, ccc.
        let m1 = manifest_alpha(vec![
            desc(0, "bbb", HostControlKind::Knob),
            desc(0, "ccc", HostControlKind::Knob),
        ]);
        r.apply(&m1);
        let bbb_id = r.id_of("bbb").unwrap();
        let ccc_id = r.id_of("ccc").unwrap();
        // Add aaa — slots shift, IDs do not.
        let m2 = manifest_alpha(vec![
            desc(0, "aaa", HostControlKind::Knob),
            desc(0, "bbb", HostControlKind::Knob),
            desc(0, "ccc", HostControlKind::Knob),
        ]);
        let out = r.apply(&m2);
        assert_eq!(out.rescan, RescanLevel::All);
        assert_eq!(out.added, vec!["aaa".to_string()]);
        assert_eq!(r.id_of("bbb"), Some(bbb_id));
        assert_eq!(r.id_of("ccc"), Some(ccc_id));
        assert_eq!(r.live("aaa").unwrap().slot, 0);
        assert_eq!(r.live("bbb").unwrap().slot, 1);
        assert_eq!(r.live("ccc").unwrap().slot, 2);
    }

    #[test]
    fn remove_moves_to_tombstone_with_id_preserved() {
        let mut r = StandardHostControlRegistry::new(64);
        let m1 = manifest_alpha(vec![
            desc(0, "k", HostControlKind::Knob),
            desc(0, "j", HostControlKind::Knob),
        ]);
        r.apply(&m1);
        let k_id = r.id_of("k").unwrap();
        r.record_value(k_id, 0.42);
        let m2 = manifest_alpha(vec![desc(0, "j", HostControlKind::Knob)]);
        let out = r.apply(&m2);
        assert_eq!(out.rescan, RescanLevel::All);
        assert_eq!(out.removed, vec!["k".to_string()]);
        assert!(r.live("k").is_none());
        let t = r.tombstone("k").unwrap();
        assert_eq!(t.id, k_id);
        assert_eq!(t.last_value, 0.42);
    }

    #[test]
    fn reborn_reclaims_old_id_and_last_value() {
        let mut r = StandardHostControlRegistry::new(64);
        r.apply(&manifest_alpha(vec![desc(0, "k", HostControlKind::Knob)]));
        let id = r.id_of("k").unwrap();
        r.record_value(id, 0.7);
        // Remove.
        r.apply(&Vec::new());
        assert!(r.tombstone("k").is_some());
        // Re-add.
        let out = r.apply(&manifest_alpha(vec![desc(0, "k", HostControlKind::Knob)]));
        assert_eq!(out.reborn, vec!["k".to_string()]);
        assert_eq!(r.id_of("k"), Some(id));
        assert_eq!(r.live("k").unwrap().last_value, 0.7);
        assert!(r.tombstone("k").is_none());
        // No new IDs minted.
        assert_eq!(r.next_id(), 1);
    }

    #[test]
    fn kind_change_tombstones_old_id_and_mints_new() {
        let mut r = StandardHostControlRegistry::new(64);
        r.apply(&manifest_alpha(vec![desc(0, "k", HostControlKind::Knob)]));
        let old_id = r.id_of("k").unwrap();
        let out = r.apply(&manifest_alpha(vec![desc(0, "k", HostControlKind::Toggle)]));
        assert_eq!(out.rescan, RescanLevel::All);
        assert_eq!(out.kind_changed, vec!["k".to_string()]);
        let new_id = r.live("k").unwrap().id;
        assert_ne!(new_id, old_id);
        // Old ID lives on in the tombstone.
        let t = r.tombstone("k").unwrap();
        assert_eq!(t.id, old_id);
        assert_eq!(t.kind, HostControlKind::Knob);
    }

    #[test]
    fn params_only_change_is_rescan_info() {
        let mut r = StandardHostControlRegistry::new(64);
        let m1 = manifest_alpha(vec![desc_with(
            0,
            "cutoff",
            HostControlKind::Knob,
            &[("low", HostControlParamValue::Int(20))],
        )]);
        r.apply(&m1);
        let id = r.id_of("cutoff").unwrap();
        let m2 = manifest_alpha(vec![desc_with(
            0,
            "cutoff",
            HostControlKind::Knob,
            &[("low", HostControlParamValue::Int(40))],
        )]);
        let out = r.apply(&m2);
        assert_eq!(out.rescan, RescanLevel::Info);
        assert_eq!(out.info_changed, vec!["cutoff".to_string()]);
        assert_eq!(r.id_of("cutoff"), Some(id));
    }

    #[test]
    fn info_change_does_not_promote_to_all() {
        // Only param-info change → Info, even with multiple controls.
        let mut r = StandardHostControlRegistry::new(64);
        let m1 = manifest_alpha(vec![
            desc_with(0, "a", HostControlKind::Knob, &[("low", HostControlParamValue::Int(0))]),
            desc(0, "b", HostControlKind::Knob),
        ]);
        r.apply(&m1);
        let m2 = manifest_alpha(vec![
            desc_with(0, "a", HostControlKind::Knob, &[("low", HostControlParamValue::Int(1))]),
            desc(0, "b", HostControlKind::Knob),
        ]);
        let out = r.apply(&m2);
        assert_eq!(out.rescan, RescanLevel::Info);
    }

    #[test]
    fn add_and_info_change_promotes_to_all() {
        let mut r = StandardHostControlRegistry::new(64);
        let m1 = manifest_alpha(vec![desc_with(
            0,
            "a",
            HostControlKind::Knob,
            &[("low", HostControlParamValue::Int(0))],
        )]);
        r.apply(&m1);
        let m2 = manifest_alpha(vec![
            desc_with(0, "a", HostControlKind::Knob, &[("low", HostControlParamValue::Int(1))]),
            desc(0, "b", HostControlKind::Knob),
        ]);
        let out = r.apply(&m2);
        assert_eq!(out.rescan, RescanLevel::All);
        assert!(out.info_changed.contains(&"a".to_string()));
        assert!(out.added.contains(&"b".to_string()));
    }

    #[test]
    fn record_value_persists_through_remove_and_rebirth() {
        let mut r = StandardHostControlRegistry::new(64);
        r.apply(&manifest_alpha(vec![desc(0, "k", HostControlKind::Knob)]));
        let id = r.id_of("k").unwrap();
        r.record_value(id, 0.31);
        r.apply(&Vec::new());
        assert_eq!(r.tombstone("k").unwrap().last_value, 0.31);
        r.apply(&manifest_alpha(vec![desc(0, "k", HostControlKind::Knob)]));
        assert_eq!(r.live("k").unwrap().last_value, 0.31);
    }

    #[test]
    fn record_value_on_unknown_id_is_noop() {
        let mut r = StandardHostControlRegistry::new(64);
        r.record_value(999, 1.0);  // does not panic, no state mutation.
        assert_eq!(r.live_len(), 0);
    }

    #[test]
    fn id_to_slot_reflects_live_only_sorted_by_id() {
        let mut r = StandardHostControlRegistry::new(64);
        let m = manifest_alpha(vec![
            desc(0, "z", HostControlKind::Knob),
            desc(0, "a", HostControlKind::Knob),
            desc(0, "m", HostControlKind::Knob),
        ]);
        r.apply(&m);
        // IDs were minted in apply order: alphabetical → a=0, m=1, z=2.
        let lookup = r.id_to_slot();
        assert_eq!(lookup, vec![(0, 0), (1, 1), (2, 2)]);
        // Remove m → live shrinks; tombstone not in lookup.
        r.apply(&manifest_alpha(vec![
            desc(0, "z", HostControlKind::Knob),
            desc(0, "a", HostControlKind::Knob),
        ]));
        let lookup = r.id_to_slot();
        // Slots re-laid out: a=0, z=1.
        assert_eq!(lookup, vec![(0, 0), (2, 1)]);
    }

    #[test]
    fn next_id_is_monotonic_across_remove_and_add() {
        let mut r = StandardHostControlRegistry::new(64);
        r.apply(&manifest_alpha(vec![desc(0, "a", HostControlKind::Knob)]));
        r.apply(&Vec::new());  // tombstone a
        assert_eq!(r.next_id(), 1);
        // Add a new name (not reborn) — gets ID 1, not 0.
        r.apply(&manifest_alpha(vec![desc(0, "b", HostControlKind::Knob)]));
        assert_eq!(r.id_of("b"), Some(1));
        assert_eq!(r.next_id(), 2);
    }

    #[test]
    fn kind_change_during_rebirth_mints_new_id_and_keeps_old_tombstone() {
        // Tombstone a knob, then re-add it as a toggle.
        let mut r = StandardHostControlRegistry::new(64);
        r.apply(&manifest_alpha(vec![desc(0, "k", HostControlKind::Knob)]));
        let old_id = r.id_of("k").unwrap();
        r.apply(&Vec::new());
        let out = r.apply(&manifest_alpha(vec![desc(0, "k", HostControlKind::Toggle)]));
        // Treated as fresh add (not rebirth) because kind differs.
        assert_eq!(out.added, vec!["k".to_string()]);
        assert!(out.reborn.is_empty());
        let new_id = r.live("k").unwrap().id;
        assert_ne!(new_id, old_id);
        // The old tombstone is preserved so a future Knob "k" can
        // still reclaim its original ID.
        let t = r.tombstone("k").unwrap();
        assert_eq!(t.id, old_id);
        assert_eq!(t.kind, HostControlKind::Knob);
    }

    #[test]
    fn tombstone_eviction_drops_oldest_by_epoch() {
        let mut r = StandardHostControlRegistry::new(2); // tiny cap
        // Add three, then tombstone all three across separate epochs
        // so we can observe LRU.
        r.apply(&manifest_alpha(vec![desc(0, "a", HostControlKind::Knob)]));
        r.apply(&Vec::new()); // a tombstoned at epoch 2
        r.apply(&manifest_alpha(vec![desc(0, "b", HostControlKind::Knob)]));
        r.apply(&Vec::new()); // b tombstoned at epoch 4 — evicts a? cap=2, 1+1<=2, ok
        assert!(r.tombstone("a").is_some());
        assert!(r.tombstone("b").is_some());
        // Add c, tombstone it: room_needed=1 + existing 2 > cap 2, evict oldest (a).
        r.apply(&manifest_alpha(vec![desc(0, "c", HostControlKind::Knob)]));
        let out = r.apply(&Vec::new());
        assert_eq!(out.evicted_tombstones, vec!["a".to_string()]);
        assert!(r.tombstone("a").is_none());
        assert!(r.tombstone("b").is_some());
        assert!(r.tombstone("c").is_some());
    }

    #[test]
    fn tombstone_eviction_at_64_matches_backplane_cap() {
        // 64 live, all removed, then add 1 more and remove → eviction.
        let cap = 64;
        let mut r = StandardHostControlRegistry::new(cap);
        let names: Vec<String> = (0..cap).map(|i| format!("c{i:03}")).collect();
        let live: Vec<HostControlDescriptor> = names
            .iter()
            .map(|n| desc(0, n, HostControlKind::Knob))
            .collect();
        r.apply(&manifest_alpha(live));
        r.apply(&Vec::new()); // all 64 tombstoned
        assert_eq!(r.tombstone_len(), cap);
        // Add a 65th name and remove it → forces eviction of oldest.
        r.apply(&manifest_alpha(vec![desc(0, "extra", HostControlKind::Knob)]));
        let out = r.apply(&Vec::new());
        assert_eq!(out.evicted_tombstones.len(), 1);
        assert_eq!(r.tombstone_len(), cap);
    }

    #[test]
    fn reborn_does_not_consume_tombstone_eviction_room() {
        // Rebirth shrinks the tombstone table, so it should never
        // trigger eviction.
        let mut r = StandardHostControlRegistry::new(2);
        r.apply(&manifest_alpha(vec![
            desc(0, "a", HostControlKind::Knob),
            desc(0, "b", HostControlKind::Knob),
        ]));
        r.apply(&Vec::new()); // both tombstoned, count=2 at cap
        let out = r.apply(&manifest_alpha(vec![desc(0, "a", HostControlKind::Knob)]));
        assert!(out.evicted_tombstones.is_empty());
        assert_eq!(r.tombstone_len(), 1);
        assert!(r.tombstone("b").is_some());
    }

    #[test]
    fn trigger_kind_works_like_others() {
        // Trigger has no required value-state, but lifecycle treats it
        // identically.
        let mut r = StandardHostControlRegistry::new(64);
        let out = r.apply(&manifest_alpha(vec![desc(0, "fire", HostControlKind::Trigger)]));
        assert_eq!(out.added, vec!["fire".to_string()]);
        assert_eq!(r.live("fire").unwrap().kind, HostControlKind::Trigger);
        // Kind change Trigger -> Knob still tombstones.
        let out = r.apply(&manifest_alpha(vec![desc(0, "fire", HostControlKind::Knob)]));
        assert_eq!(out.kind_changed, vec!["fire".to_string()]);
    }
}
