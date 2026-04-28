pub mod instance_id;
pub mod module;
pub mod module_descriptor;
pub mod parameter_map;
pub mod params_enum;
pub mod structural_params;

pub use instance_id::InstanceId;
pub use module::{validate_and_pack, validate_parameters, Module, PortConnectivity};
pub use module_descriptor::{ModuleDescriptor, ModuleShape, ParameterDescriptor, ParameterKind, ParameterRef, PortDescriptor, PortRef};
pub use parameter_map::{ParameterKey, ParameterMap, ParameterValue};
pub use structural_params::{StructuralParams, StructuralValue};
