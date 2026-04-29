//! Structural-parameter wire format (ADR 0060, ticket 0739).
//!
//! Construction-time inputs that never reach the audio thread are carried
//! across the FFI as a positional packed blob, mirroring `ParamFrame` for
//! the realtime side. Slot order matches `descriptor.structural_params`.
//!
//! Wire format
//! -----------
//!
//! ```text
//! [u16 slot_count] [slot_0] [slot_1] ... [slot_{n-1}]
//!
//! slot ::= [u8 type_tag] [u32 value_len] [bytes; value_len]
//!
//! type_tag = 0 -> bool   (value_len = 1, byte 0/1)
//!            1 -> i64    (value_len = 8, little-endian)
//!            2 -> f64    (value_len = 8, little-endian bits)
//!            3 -> string (value_len = N, raw UTF-8)
//! ```
//!
//! All multi-byte integers are little-endian. The blob is constructed by
//! the host (`pack_structural`) and consumed by the plugin SDK
//! (`StructuralView` / `decode_structural_params`).

use patches_core::modules::{
    ModuleDescriptor, ParameterDescriptor, ParameterKind, StructuralParams, StructuralValue,
};

pub const TAG_BOOL: u8 = 0;
pub const TAG_I64: u8 = 1;
pub const TAG_F64: u8 = 2;
pub const TAG_STRING: u8 = 3;

/// Errors raised by the structural blob decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuralDecodeError {
    Truncated,
    SlotCountMismatch { expected: usize, actual: usize },
    UnknownTag(u8),
    BadValueLen { tag: u8, value_len: u32 },
    InvalidUtf8,
}

impl std::fmt::Display for StructuralDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated => write!(f, "structural blob truncated"),
            Self::SlotCountMismatch { expected, actual } => write!(
                f,
                "structural slot count mismatch: descriptor has {expected}, blob has {actual}"
            ),
            Self::UnknownTag(t) => write!(f, "unknown structural type tag {t}"),
            Self::BadValueLen { tag, value_len } => {
                write!(f, "bad value_len {value_len} for tag {tag}")
            }
            Self::InvalidUtf8 => write!(f, "structural string slot is not valid UTF-8"),
        }
    }
}

impl std::error::Error for StructuralDecodeError {}

/// Encode a structural blob for `descriptor.structural_params` from a
/// [`StructuralParams`] map.
///
/// Slots missing from `params` are emitted at the descriptor's declared
/// default. `ParameterKind::File` has no declared default; an absent file
/// slot is encoded as an empty string.
pub fn pack_structural(descriptor: &ModuleDescriptor, params: &StructuralParams) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + descriptor.structural_params.len() * 16);
    let n = descriptor.structural_params.len();
    out.extend_from_slice(&(n as u16).to_le_bytes());
    for slot in &descriptor.structural_params {
        let value = resolve_slot_value(slot, params);
        encode_slot(&mut out, &value);
    }
    out
}

fn resolve_slot_value(slot: &ParameterDescriptor, params: &StructuralParams) -> StructuralValue {
    if let Some(v) = params.get(slot.name, slot.index) {
        return v.clone();
    }
    match slot.parameter_type {
        ParameterKind::Bool { default } => StructuralValue::Bool(default),
        ParameterKind::Int { default, .. } => StructuralValue::Int(default),
        ParameterKind::Float { default, .. } => StructuralValue::Float(default),
        ParameterKind::Enum { variants, default } => {
            let idx = variants.iter().position(|v| v == &default).unwrap_or(0) as i64;
            StructuralValue::Int(idx)
        }
        ParameterKind::File { .. } => StructuralValue::String(String::new()),
        ParameterKind::SongName => StructuralValue::Int(-1),
    }
}

fn encode_slot(out: &mut Vec<u8>, value: &StructuralValue) {
    match value {
        StructuralValue::Bool(b) => {
            out.push(TAG_BOOL);
            out.extend_from_slice(&1u32.to_le_bytes());
            out.push(if *b { 1 } else { 0 });
        }
        StructuralValue::Int(i) => {
            out.push(TAG_I64);
            out.extend_from_slice(&8u32.to_le_bytes());
            out.extend_from_slice(&i.to_le_bytes());
        }
        StructuralValue::Float(f) => {
            out.push(TAG_F64);
            out.extend_from_slice(&8u32.to_le_bytes());
            out.extend_from_slice(&(*f as f64).to_le_bytes());
        }
        StructuralValue::String(s) => {
            out.push(TAG_STRING);
            let bytes = s.as_bytes();
            out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(bytes);
        }
    }
}

/// Zero-copy view over an encoded structural blob.
///
/// `StructuralView` holds slice references into the host-supplied blob
/// and exposes typed accessors by slot index. Strings borrow from the
/// blob, so the view's lifetime is tied to the call.
#[derive(Debug)]
pub struct StructuralView<'a> {
    bytes: &'a [u8],
    offsets: Vec<SlotOffset>,
}

#[derive(Debug, Clone, Copy)]
struct SlotOffset {
    tag: u8,
    value_start: usize,
    value_len: usize,
}

impl<'a> StructuralView<'a> {
    /// Parse the slot table out of `bytes`. The `expected_slots` argument
    /// is checked against the blob's `slot_count` header.
    pub fn parse(
        bytes: &'a [u8],
        expected_slots: usize,
    ) -> Result<Self, StructuralDecodeError> {
        if bytes.len() < 2 {
            return Err(StructuralDecodeError::Truncated);
        }
        let slot_count = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
        if slot_count != expected_slots {
            return Err(StructuralDecodeError::SlotCountMismatch {
                expected: expected_slots,
                actual: slot_count,
            });
        }
        let mut cursor = 2usize;
        let mut offsets = Vec::with_capacity(slot_count);
        for _ in 0..slot_count {
            if cursor + 5 > bytes.len() {
                return Err(StructuralDecodeError::Truncated);
            }
            let tag = bytes[cursor];
            let value_len = u32::from_le_bytes([
                bytes[cursor + 1],
                bytes[cursor + 2],
                bytes[cursor + 3],
                bytes[cursor + 4],
            ]) as usize;
            cursor += 5;
            if cursor + value_len > bytes.len() {
                return Err(StructuralDecodeError::Truncated);
            }
            match tag {
                TAG_BOOL if value_len != 1 => {
                    return Err(StructuralDecodeError::BadValueLen {
                        tag,
                        value_len: value_len as u32,
                    });
                }
                TAG_I64 | TAG_F64 if value_len != 8 => {
                    return Err(StructuralDecodeError::BadValueLen {
                        tag,
                        value_len: value_len as u32,
                    });
                }
                TAG_BOOL | TAG_I64 | TAG_F64 | TAG_STRING => {}
                t => return Err(StructuralDecodeError::UnknownTag(t)),
            }
            offsets.push(SlotOffset {
                tag,
                value_start: cursor,
                value_len,
            });
            cursor += value_len;
        }
        Ok(Self { bytes, offsets })
    }

    pub fn slot_count(&self) -> usize {
        self.offsets.len()
    }

    pub fn tag(&self, slot: usize) -> u8 {
        self.offsets[slot].tag
    }

    pub fn get_bool(&self, slot: usize) -> Option<bool> {
        let s = self.offsets[slot];
        if s.tag != TAG_BOOL {
            return None;
        }
        Some(self.bytes[s.value_start] != 0)
    }

    pub fn get_i64(&self, slot: usize) -> Option<i64> {
        let s = self.offsets[slot];
        if s.tag != TAG_I64 {
            return None;
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&self.bytes[s.value_start..s.value_start + 8]);
        Some(i64::from_le_bytes(buf))
    }

    pub fn get_f64(&self, slot: usize) -> Option<f64> {
        let s = self.offsets[slot];
        if s.tag != TAG_F64 {
            return None;
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&self.bytes[s.value_start..s.value_start + 8]);
        Some(f64::from_le_bytes(buf))
    }

    pub fn get_str(&self, slot: usize) -> Option<&'a str> {
        let s = self.offsets[slot];
        if s.tag != TAG_STRING {
            return None;
        }
        std::str::from_utf8(&self.bytes[s.value_start..s.value_start + s.value_len]).ok()
    }
}

/// Decode a structural blob into an owned [`StructuralParams`] keyed by
/// the descriptor's slot names. Allocation is allowed: the structural
/// path is control-thread, never audio-thread.
pub fn decode_structural_params(
    bytes: &[u8],
    descriptor: &ModuleDescriptor,
) -> Result<StructuralParams, StructuralDecodeError> {
    let view = StructuralView::parse(bytes, descriptor.structural_params.len())?;
    let mut out = StructuralParams::new();
    for (i, slot) in descriptor.structural_params.iter().enumerate() {
        let value = match view.tag(i) {
            TAG_BOOL => StructuralValue::Bool(view.get_bool(i).unwrap()),
            TAG_I64 => StructuralValue::Int(view.get_i64(i).unwrap()),
            TAG_F64 => StructuralValue::Float(view.get_f64(i).unwrap() as f32),
            TAG_STRING => StructuralValue::String(
                view.get_str(i)
                    .ok_or(StructuralDecodeError::InvalidUtf8)?
                    .to_string(),
            ),
            t => return Err(StructuralDecodeError::UnknownTag(t)),
        };
        out.insert(slot.name.to_string(), slot.index, value);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::modules::ModuleShape;

    fn descriptor_all_kinds() -> ModuleDescriptor {
        ModuleDescriptor::new("S", ModuleShape::default())
            .structural_bool_param("b", true)
            .structural_int_param("n", -10, 10, 7)
            .structural_float_param("f", 0.0, 1.0, 0.25)
            .structural_string_param("path", &["wav"])
    }

    #[test]
    fn defaults_round_trip() {
        let desc = descriptor_all_kinds();
        let blob = pack_structural(&desc, &StructuralParams::new());
        let decoded = decode_structural_params(&blob, &desc).unwrap();
        assert_eq!(decoded.get_bool("b", 0), Some(true));
        assert_eq!(decoded.get_int("n", 0), Some(7));
        assert!((decoded.get_float("f", 0).unwrap() - 0.25).abs() < 1e-6);
        assert_eq!(decoded.get_string("path", 0), Some(""));
    }

    #[test]
    fn overrides_round_trip() {
        let desc = descriptor_all_kinds();
        let mut params = StructuralParams::new();
        params.insert("b", 0, StructuralValue::Bool(false));
        params.insert("n", 0, StructuralValue::Int(-3));
        params.insert("f", 0, StructuralValue::Float(0.875));
        params.insert("path", 0, StructuralValue::String("kick.wav".into()));
        let blob = pack_structural(&desc, &params);
        let decoded = decode_structural_params(&blob, &desc).unwrap();
        assert_eq!(decoded.get_bool("b", 0), Some(false));
        assert_eq!(decoded.get_int("n", 0), Some(-3));
        assert!((decoded.get_float("f", 0).unwrap() - 0.875).abs() < 1e-6);
        assert_eq!(decoded.get_string("path", 0), Some("kick.wav"));
    }

    #[test]
    fn empty_descriptor_round_trip() {
        let desc = ModuleDescriptor::new("Empty", ModuleShape::default());
        let blob = pack_structural(&desc, &StructuralParams::new());
        assert_eq!(blob, vec![0, 0]);
        let decoded = decode_structural_params(&blob, &desc).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn slot_count_mismatch() {
        let desc = descriptor_all_kinds();
        let err = StructuralView::parse(&[0, 0], desc.structural_params.len()).unwrap_err();
        assert!(matches!(
            err,
            StructuralDecodeError::SlotCountMismatch { .. }
        ));
    }

    #[test]
    fn truncated_blob() {
        assert!(matches!(
            StructuralView::parse(&[0], 0).unwrap_err(),
            StructuralDecodeError::Truncated
        ));
        // header says one slot, body cut.
        assert!(matches!(
            StructuralView::parse(&[1, 0, TAG_I64], 1).unwrap_err(),
            StructuralDecodeError::Truncated
        ));
    }

    #[test]
    fn unknown_tag() {
        let mut blob = vec![1u8, 0];
        blob.push(99);
        blob.extend_from_slice(&0u32.to_le_bytes());
        let err = StructuralView::parse(&blob, 1).unwrap_err();
        assert!(matches!(err, StructuralDecodeError::UnknownTag(99)));
    }

    #[test]
    fn view_zero_copy_string() {
        let desc = ModuleDescriptor::new("S", ModuleShape::default())
            .structural_string_param("path", &["wav"]);
        let mut params = StructuralParams::new();
        params.insert("path", 0, StructuralValue::String("hello".into()));
        let blob = pack_structural(&desc, &params);
        let view = StructuralView::parse(&blob, 1).unwrap();
        assert_eq!(view.get_str(0), Some("hello"));
        // The borrowed string points into the blob.
        let s = view.get_str(0).unwrap();
        assert_eq!(s.as_ptr() as usize - blob.as_ptr() as usize, 2 + 5);
    }
}
