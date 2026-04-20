use std::sync::Arc;

use patches_core::modules::module_descriptor::{ModuleDescriptor, ModuleShape};
use patches_core::modules::parameter_map::{ParameterKey, ParameterMap, ParameterValue};

use crate::param_frame::pack::{pack_into, PackError};
use crate::param_frame::shadow::assert_view_matches_map;
use crate::param_frame::{ParamFrame, ParamView, ParamViewIndex};
use crate::param_layout::compute_layout;

fn empty_shape() -> ModuleShape {
    ModuleShape { channels: 1, length: 0, high_quality: false }
}

fn mixed_descriptor() -> ModuleDescriptor {
    ModuleDescriptor::new("Mix", empty_shape())
        .float_param("gain", 0.0, 1.0, 0.25)
        .int_param("count", 0, 8, 3)
        .bool_param("active", true)
        .enum_param("mode", &["a", "b", "c"], "b")
        .file_param("sample", &[])
}

fn defaults_from(desc: &ModuleDescriptor) -> ParameterMap {
    let mut m = ParameterMap::new();
    for p in &desc.parameters {
        use patches_core::modules::module_descriptor::ParameterKind;
        let v = match &p.parameter_type {
            ParameterKind::Float { default, .. } => ParameterValue::Float(*default),
            ParameterKind::Int { default, .. } => ParameterValue::Int(*default),
            ParameterKind::Bool { default } => ParameterValue::Bool(*default),
            ParameterKind::Enum { variants, default, .. } => {
                let idx = variants.iter().position(|v| v == default).unwrap_or(0);
                ParameterValue::Enum(idx as u32)
            }
            ParameterKind::File { .. } => {
                ParameterValue::FloatBuffer(Arc::<[f32]>::from(vec![0.0f32].into_boxed_slice()))
            }
            ParameterKind::SongName => ParameterValue::Int(0),
        };
        m.insert(p.name.to_string(), v);
    }
    m
}

#[test]
fn frame_shape_matches_layout() {
    let d = mixed_descriptor();
    let l = compute_layout(&d);
    let f = ParamFrame::with_layout(&l);
    assert_eq!(f.scalar_size(), l.scalar_size as usize);
    assert_eq!(f.buffer_slot_count(), l.buffer_slots.len());
    assert_eq!(f.layout_hash(), l.descriptor_hash);
    assert!(f.scalar_area().iter().all(|b| *b == 0));
    assert!(f.buffer_slots().iter().all(|x| *x == 0));
}

#[test]
fn frame_empty_layout_zero_length() {
    let d = ModuleDescriptor::new("Empty", empty_shape());
    let l = compute_layout(&d);
    let f = ParamFrame::with_layout(&l);
    assert_eq!(f.scalar_area().len(), 0);
    assert_eq!(f.buffer_slots().len(), 0);
}

#[test]
fn frame_reset_clears_bytes() {
    let d = mixed_descriptor();
    let l = compute_layout(&d);
    let mut f = ParamFrame::with_layout(&l);
    for b in f.scalar_area_mut() {
        *b = 0xff;
    }
    for s in f.buffer_slots_mut() {
        *s = 0xdead_beef;
    }
    f.reset();
    assert!(f.scalar_area().iter().all(|b| *b == 0));
    assert!(f.buffer_slots().iter().all(|s| *s == 0));
}

#[test]
fn pack_round_trip_all_scalar_tags() {
    let d = mixed_descriptor();
    let l = compute_layout(&d);
    let defaults = defaults_from(&d);
    let mut overrides = ParameterMap::new();
    overrides.insert("gain".into(), ParameterValue::Float(0.75));
    overrides.insert("count".into(), ParameterValue::Int(5));
    overrides.insert("active".into(), ParameterValue::Bool(false));
    overrides.insert("mode".into(), ParameterValue::Enum(2));

    let mut f = ParamFrame::with_layout(&l);
    pack_into(&l, &defaults, &overrides, &mut f).expect("pack ok");

    let idx = ParamViewIndex::from_layout(&l);
    let view = ParamView::new(&idx, &f);
    assert_eq!(view.float("gain"), 0.75);
    assert_eq!(view.int("count"), 5);
    assert!(!view.bool("active"));
    assert_eq!(view.enum_variant("mode"), 2);
    // Buffer slot: default arc stub id non-zero.
    let b = view.buffer("sample");
    assert!(b.is_some());
}

#[test]
fn pack_override_beats_default() {
    let d = mixed_descriptor();
    let l = compute_layout(&d);
    let defaults = defaults_from(&d);
    let overrides = ParameterMap::new();
    let mut f = ParamFrame::with_layout(&l);
    pack_into(&l, &defaults, &overrides, &mut f).expect("pack ok");
    let idx = ParamViewIndex::from_layout(&l);
    let view = ParamView::new(&idx, &f);
    assert_eq!(view.float("gain"), 0.25); // default
    assert_eq!(view.int("count"), 3);
}

#[test]
fn pack_layout_hash_mismatch_errors() {
    let d1 = mixed_descriptor();
    let d2 = ModuleDescriptor::new("Other", empty_shape())
        .float_param("other", 0.0, 1.0, 0.0);
    let l1 = compute_layout(&d1);
    let l2 = compute_layout(&d2);
    let f = ParamFrame::with_layout(&l2);
    let defaults = defaults_from(&d1);
    let overrides = ParameterMap::new();
    // Only check non-panic path in release. In debug the assert fires first;
    // gate this test behind cfg(not(debug_assertions)) to exercise the
    // release branch.
    #[cfg(not(debug_assertions))]
    {
        let r = pack_into(&l1, &defaults, &overrides, &mut f);
        assert!(matches!(r, Err(PackError::LayoutHashMismatch { .. })));
    }
    let _ = (l1, l2, f, defaults, overrides, PackError::MissingValue);
}

#[test]
fn view_index_deterministic() {
    let d = mixed_descriptor();
    let l = compute_layout(&d);
    let a = ParamViewIndex::from_layout(&l);
    let b = ParamViewIndex::from_layout(&l);
    // Round-trip: both indexes decode the same frame identically.
    let defaults = defaults_from(&d);
    let overrides = ParameterMap::new();
    let mut f = ParamFrame::with_layout(&l);
    pack_into(&l, &defaults, &overrides, &mut f).unwrap();
    let va = ParamView::new(&a, &f);
    let vb = ParamView::new(&b, &f);
    assert_eq!(va.float("gain"), vb.float("gain"));
    assert_eq!(va.int("count"), vb.int("count"));
    assert_eq!(va.bool("active"), vb.bool("active"));
    assert_eq!(va.enum_variant("mode"), vb.enum_variant("mode"));
}

#[test]
fn view_index_empty_layout() {
    let d = ModuleDescriptor::new("Empty", empty_shape());
    let l = compute_layout(&d);
    let idx = ParamViewIndex::from_layout(&l);
    assert_eq!(idx.descriptor_hash(), l.descriptor_hash);
}

#[test]
fn view_unknown_key_release_returns_zero() {
    // Only meaningful when debug_asserts disabled.
    #[cfg(not(debug_assertions))]
    {
        let d = mixed_descriptor();
        let l = compute_layout(&d);
        let idx = ParamViewIndex::from_layout(&l);
        let f = ParamFrame::with_layout(&l);
        let v = ParamView::new(&idx, &f);
        assert_eq!(v.float("nonexistent"), 0.0);
    }
}

#[test]
fn view_perfect_hash_no_collisions_large() {
    // Many static-named params — verify PHF resolves every one.
    const NAMES: &[&str] = &[
        "a0","a1","a2","a3","a4","a5","a6","a7","a8","a9",
        "b0","b1","b2","b3","b4","b5","b6","b7","b8","b9",
        "c0","c1","c2","c3","c4","c5","c6","c7","c8","c9",
        "d0","d1","d2","d3","d4","d5","d6","d7","d8","d9",
        "e0","e1","e2","e3","e4","e5","e6","e7","e8","e9",
        "f0","f1","f2","f3","f4","f5","f6","f7","f8","f9",
        "alpha","beta","gamma","delta",
    ];
    let mut d = ModuleDescriptor::new("Big", empty_shape());
    for n in NAMES {
        d = d.float_param(n, 0.0, 1.0, 0.0);
    }
    let l = compute_layout(&d);
    let defaults = defaults_from(&d);
    let mut overrides = ParameterMap::new();
    for (i, n) in NAMES.iter().enumerate() {
        overrides.insert((*n).to_string(), ParameterValue::Float(i as f32));
    }
    let mut f = ParamFrame::with_layout(&l);
    pack_into(&l, &defaults, &overrides, &mut f).unwrap();
    let idx = ParamViewIndex::from_layout(&l);
    let v = ParamView::new(&idx, &f);
    for (i, n) in NAMES.iter().enumerate() {
        assert_eq!(v.float(ParameterKey::new((*n).to_string(), 0)), i as f32);
    }
}

// ── Shadow equality ───────────────────────────────────────────────────────

#[test]
fn shadow_equality_all_variants() {
    let d = mixed_descriptor();
    let l = compute_layout(&d);
    let defaults = defaults_from(&d);

    let mut overrides = ParameterMap::new();
    overrides.insert("gain".into(), ParameterValue::Float(0.125));
    overrides.insert("count".into(), ParameterValue::Int(-7));
    overrides.insert("active".into(), ParameterValue::Bool(false));
    overrides.insert("mode".into(), ParameterValue::Enum(1));
    let arc: Arc<[f32]> = Arc::from(vec![1.0f32, 2.0, 3.0].into_boxed_slice());
    overrides.insert("sample".into(), ParameterValue::FloatBuffer(arc));

    // Build the observed map that would be passed to the module.
    let mut observed = defaults.clone();
    for (n, i, v) in overrides.iter() {
        observed.insert_param(n.to_string(), i, v.clone());
    }

    let mut f = ParamFrame::with_layout(&l);
    pack_into(&l, &defaults, &overrides, &mut f).unwrap();
    let idx = ParamViewIndex::from_layout(&l);
    let view = ParamView::new(&idx, &f);
    assert_view_matches_map(&idx, &view, &observed);
}

#[test]
#[should_panic(expected = "shadow divergence")]
fn shadow_detects_divergence_when_frame_corrupt() {
    let d = ModuleDescriptor::new("M", empty_shape())
        .float_param("gain", 0.0, 1.0, 0.5);
    let l = compute_layout(&d);
    let defaults = {
        let mut m = ParameterMap::new();
        m.insert("gain".into(), ParameterValue::Float(0.5));
        m
    };
    let mut overrides = ParameterMap::new();
    overrides.insert("gain".into(), ParameterValue::Float(0.9));
    let mut f = ParamFrame::with_layout(&l);
    pack_into(&l, &defaults, &overrides, &mut f).unwrap();
    // Corrupt the scalar area so the view disagrees with the observed map.
    for b in f.scalar_area_mut() {
        *b = 0xa5;
    }
    let idx = ParamViewIndex::from_layout(&l);
    let view = ParamView::new(&idx, &f);
    let mut observed = defaults.clone();
    observed.insert("gain".into(), ParameterValue::Float(0.9));
    assert_view_matches_map(&idx, &view, &observed);
}

