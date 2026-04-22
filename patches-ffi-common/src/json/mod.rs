//! Hand-rolled JSON serialization for types that cross the FFI boundary on the
//! control thread. This avoids adding serde as a dependency to patches-core.
//!
//! Deserialized `&'static str` fields are produced by leaking `String`s via
//! `Box::leak`. This is intentional and bounded: one set of leaked strings per
//! module type per library load.

mod ser;
mod de;

pub use ser::serialize_module_descriptor;
pub use de::deserialize_module_descriptor;

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{CableKind, ModuleDescriptor, ModuleShape, ParameterDescriptor, ParameterKind, PolyLayout, PortDescriptor};

    #[test]
    fn module_descriptor_round_trip() {
        let desc = ModuleDescriptor {
            module_name: "TestGain",
            shape: ModuleShape { channels: 2, length: 8, high_quality: true },
            inputs: vec![
                PortDescriptor { name: "in", index: 0, kind: CableKind::Mono, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "sidechain", index: 0, kind: CableKind::Poly, poly_layout: PolyLayout::Audio },
            ],
            outputs: vec![
                PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, poly_layout: PolyLayout::Audio },
            ],
            parameters: vec![
                ParameterDescriptor { name: "gain", index: 0, parameter_type: ParameterKind::Float { min: 0.0, max: 2.0, default: 1.0 } },
                ParameterDescriptor { name: "mode", index: 0, parameter_type: ParameterKind::Enum { variants: &["linear", "log"], default: "linear" } },
                ParameterDescriptor { name: "active", index: 0, parameter_type: ParameterKind::Bool { default: true } },
                ParameterDescriptor { name: "voices", index: 0, parameter_type: ParameterKind::Int { min: 1, max: 8, default: 4 } },
            ],
        };

        let json = serialize_module_descriptor(&desc);
        let back = deserialize_module_descriptor(&json).expect("deserialize failed");

        assert_eq!(back.module_name, "TestGain");
        assert_eq!(back.shape.channels, 2);
        assert_eq!(back.shape.length, 8);
        assert!(back.shape.high_quality);
        assert_eq!(back.inputs.len(), 2);
        assert_eq!(back.inputs[0].name, "in");
        assert_eq!(back.inputs[0].kind, CableKind::Mono);
        assert_eq!(back.inputs[1].name, "sidechain");
        assert_eq!(back.inputs[1].kind, CableKind::Poly);
        assert_eq!(back.outputs.len(), 1);
        assert_eq!(back.outputs[0].name, "out");
        assert_eq!(back.parameters.len(), 4);

        // Float param
        match &back.parameters[0].parameter_type {
            ParameterKind::Float { min, max, default } => {
                assert_eq!(*min, 0.0);
                assert_eq!(*max, 2.0);
                assert_eq!(*default, 1.0);
            }
            other => panic!("expected Float, got {other:?}"),
        }

        // Enum param
        match &back.parameters[1].parameter_type {
            ParameterKind::Enum { variants, default } => {
                assert_eq!(variants.len(), 2);
                assert_eq!(variants[0], "linear");
                assert_eq!(variants[1], "log");
                assert_eq!(*default, "linear");
            }
            other => panic!("expected Enum, got {other:?}"),
        }

        // Bool param
        match &back.parameters[2].parameter_type {
            ParameterKind::Bool { default } => assert!(*default),
            other => panic!("expected Bool, got {other:?}"),
        }

        // Int param
        match &back.parameters[3].parameter_type {
            ParameterKind::Int { min, max, default } => {
                assert_eq!(*min, 1);
                assert_eq!(*max, 8);
                assert_eq!(*default, 4);
            }
            other => panic!("expected Int, got {other:?}"),
        }

    }

}
