//! Hand-rolled JSON serialization for types that cross the FFI boundary on the
//! control thread. This avoids adding serde as a dependency to patches-core.
//!
//! Deserialized `&'static str` fields are produced by leaking `String`s via
//! `Box::leak`. This is intentional and bounded: one set of leaked strings per
//! module type per library load.

mod ser;
mod de;

pub use ser::{serialize_module_descriptor, serialize_parameter_map, serialize_error};
pub use de::{deserialize_module_descriptor, deserialize_parameter_map, deserialize_error};

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{CableKind, ModuleDescriptor, ModuleShape, ParameterDescriptor, ParameterKind, ParameterMap, ParameterValue, PolyLayout, PortDescriptor};

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
                ParameterDescriptor { name: "label", index: 0, parameter_type: ParameterKind::String { default: "default" } },
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
        assert_eq!(back.parameters.len(), 5);

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

        // String param
        match &back.parameters[4].parameter_type {
            ParameterKind::String { default } => assert_eq!(*default, "default"),
            other => panic!("expected String, got {other:?}"),
        }
    }

    #[test]
    fn parameter_map_round_trip() {
        let mut params = ParameterMap::new();
        params.insert("gain".to_string(), ParameterValue::Float(0.75));
        params.insert_param("pan".to_string(), 1, ParameterValue::Float(-0.5));
        params.insert("active".to_string(), ParameterValue::Bool(true));
        params.insert("voices".to_string(), ParameterValue::Int(6));
        params.insert("mode".to_string(), ParameterValue::Enum("log"));
        params.insert("path".to_string(), ParameterValue::String("/tmp/test.wav".to_string()));

        let json = serialize_parameter_map(&params);
        let back = deserialize_parameter_map(&json).expect("deserialize failed");

        assert_eq!(back.get_scalar("gain"), Some(&ParameterValue::Float(0.75)));
        assert_eq!(back.get("pan", 1), Some(&ParameterValue::Float(-0.5)));
        assert_eq!(back.get_scalar("active"), Some(&ParameterValue::Bool(true)));
        assert_eq!(back.get_scalar("voices"), Some(&ParameterValue::Int(6)));
        // Enum: the deserialized variant is a leaked &'static str, compare by value
        match back.get_scalar("mode") {
            Some(ParameterValue::Enum(v)) => assert_eq!(*v, "log"),
            other => panic!("expected Enum(\"log\"), got {other:?}"),
        }
        assert_eq!(back.get_scalar("path"), Some(&ParameterValue::String("/tmp/test.wav".to_string())));
    }

    #[test]
    fn empty_parameter_map_round_trip() {
        let params = ParameterMap::new();
        let json = serialize_parameter_map(&params);
        let back = deserialize_parameter_map(&json).expect("deserialize failed");
        assert!(back.is_empty());
    }

    #[test]
    fn error_round_trip() {
        let msg = "parameter 'gain' out of range";
        let bytes = serialize_error(msg);
        let back = deserialize_error(&bytes);
        assert_eq!(back, msg);
    }
}
