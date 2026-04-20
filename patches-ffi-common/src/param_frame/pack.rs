//! Control-thread encoder: `ParameterMap` → `ParamFrame`.
//!
//! Zero allocation after the frame is constructed. Each `ScalarSlot` is
//! written at its layout offset via `write_unaligned`; buffer slots are
//! written as `u64` into the tail slot table. `String`/`File` variants are
//! rejected here (Spike 5 tightens this to a compile-time split).

use std::sync::Arc;

use patches_core::modules::parameter_map::{ParameterMap, ParameterValue};

use crate::param_layout::{ParamLayout, ScalarTag};

use super::ParamFrame;

/// Errors returned by `pack_into` in release builds. Debug builds panic
/// instead — these are planner bugs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackError {
    /// Frame's `layout_hash` doesn't match the layout passed to `pack_into`.
    LayoutHashMismatch { expected: u64, actual: u64 },
    /// A scalar key was missing from both `overrides` and `defaults`.
    MissingValue,
    /// A scalar key resolved to a `ParameterValue` whose variant doesn't
    /// match the slot's `ScalarTag`.
    TypeMismatch,
    /// Encountered `ParameterValue::String` or `ParameterValue::File` in a
    /// slot whose frame cannot represent it. Spike 5 removes these variants.
    UnsupportedVariant,
}

/// Write the values from `defaults` + `overrides` into `frame` according to
/// `layout`. Overrides take precedence per-key.
///
/// # Allocation
///
/// No allocation inside the function body.
pub fn pack_into(
    layout: &ParamLayout,
    defaults: &ParameterMap,
    overrides: &ParameterMap,
    frame: &mut ParamFrame,
) -> Result<(), PackError> {
    debug_assert_eq!(
        frame.layout_hash(),
        layout.descriptor_hash,
        "pack_into: frame/layout descriptor_hash mismatch",
    );
    if frame.layout_hash() != layout.descriptor_hash {
        return Err(PackError::LayoutHashMismatch {
            expected: layout.descriptor_hash,
            actual: frame.layout_hash(),
        });
    }

    let scalar_area = frame.scalar_area_mut();
    for slot in &layout.scalars {
        let value = lookup(overrides, defaults, &slot.key.name, slot.key.index);
        let value = match value {
            Some(v) => v,
            None => {
                debug_assert!(
                    false,
                    "pack_into: missing value for scalar {:?}",
                    slot.key,
                );
                return Err(PackError::MissingValue);
            }
        };
        write_scalar(scalar_area, slot.offset as usize, slot.tag, value)?;
    }

    let buf_slots = frame.buffer_slots_mut();
    for slot in &layout.buffer_slots {
        let value = lookup(overrides, defaults, &slot.key.name, slot.key.index);
        let id = match value {
            None => 0u64,
            Some(v) => buffer_id_from_value(v)?,
        };
        buf_slots[slot.slot_index as usize] = id;
    }

    Ok(())
}

fn lookup<'a>(
    overrides: &'a ParameterMap,
    defaults: &'a ParameterMap,
    name: &str,
    index: usize,
) -> Option<&'a ParameterValue> {
    overrides.get(name, index).or_else(|| defaults.get(name, index))
}

fn write_scalar(
    area: &mut [u8],
    offset: usize,
    tag: ScalarTag,
    value: &ParameterValue,
) -> Result<(), PackError> {
    // SAFETY: `offset` and the tag's size fit in `area` by layout
    // construction; `write_unaligned` requires only that the pointer is
    // within the allocated slice. We assert that statically with a
    // bounds-check on the slice indexing below.
    match (tag, value) {
        (ScalarTag::Float, ParameterValue::Float(f)) => {
            let dst = &mut area[offset..offset + 4];
            unsafe {
                std::ptr::write_unaligned(dst.as_mut_ptr() as *mut f32, *f);
            }
        }
        (ScalarTag::Int, ParameterValue::Int(i)) => {
            let dst = &mut area[offset..offset + 8];
            unsafe {
                std::ptr::write_unaligned(dst.as_mut_ptr() as *mut i64, *i);
            }
        }
        (ScalarTag::Bool, ParameterValue::Bool(b)) => {
            area[offset] = if *b { 1 } else { 0 };
        }
        (ScalarTag::Enum, ParameterValue::Enum(v)) => {
            let dst = &mut area[offset..offset + 4];
            unsafe {
                std::ptr::write_unaligned(dst.as_mut_ptr() as *mut u32, *v);
            }
        }
        // `SongName` classified as Int in the layout but flows as
        // `ParameterValue::Int` from the planner — handled above.
        (_, ParameterValue::File(_)) => {
            debug_assert!(false, "pack_into: File variant in scalar slot");
            return Err(PackError::UnsupportedVariant);
        }
        _ => {
            debug_assert!(
                false,
                "pack_into: tag {:?} does not match value kind {}",
                tag,
                value.kind_name(),
            );
            return Err(PackError::TypeMismatch);
        }
    }
    Ok(())
}

fn buffer_id_from_value(value: &ParameterValue) -> Result<u64, PackError> {
    match value {
        ParameterValue::FloatBuffer(arc) => Ok(arc_stub_id(arc)),
        ParameterValue::File(_) => {
            debug_assert!(
                false,
                "pack_into: File in buffer slot — planner must resolve first",
            );
            Err(PackError::UnsupportedVariant)
        }
        _ => {
            debug_assert!(false, "pack_into: non-buffer value in buffer slot");
            Err(PackError::TypeMismatch)
        }
    }
}

/// Spike-3 stand-in: derive a u64 buffer id from the `Arc<[f32]>` identity.
/// Real ids come from the `ArcTable` in Spike 6/7; this keeps the shadow
/// path self-consistent until then.
#[inline]
pub(crate) fn arc_stub_id(arc: &Arc<[f32]>) -> u64 {
    // Arc<[f32]>::as_ptr returns a thin pointer to the slice head.
    Arc::as_ptr(arc) as *const () as usize as u64
}

