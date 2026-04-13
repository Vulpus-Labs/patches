//! Host-side WASM module loader.
//!
//! `WasmModule` wraps a wasmtime Store+Instance and implements the `Module`
//! trait by copying cable data in/out of WASM linear memory.
//! `WasmModuleBuilder` implements `ModuleBuilder` for registry integration.

use std::path::Path;
use std::sync::Arc;

use wasmtime::{Engine, Instance, Memory, Module as WtModule, Store, TypedFunc};

type PrepareFunc = TypedFunc<(i32, i32, f32, i32, i32, i32, i32), ()>;

use patches_core::build_error::BuildError;
use patches_core::cable_pool::CablePool;
use patches_core::cables::{CableValue, InputPort, OutputPort};
use patches_core::modules::module::PeriodicUpdate;
use patches_core::modules::{InstanceId, ModuleDescriptor, ModuleShape, ParameterMap};
use patches_core::registries::ModuleBuilder;
use patches_core::{AudioEnvironment, Module};

use patches_ffi_common::json;
use patches_ffi_common::{FfiInputPort, FfiOutputPort};

use crate::cache;

// ── WASM wire types ─────────────────────────────────────────────────────────
//
// FfiInputPort/FfiOutputPort contain `usize` fields which are 8 bytes on a
// 64-bit host but 4 bytes in wasm32. These wire types use `u32` to match
// the WASM module's layout.

#[repr(C)]
#[derive(Clone, Copy)]
struct WasmInputPort {
    tag: u8,
    _pad1: [u8; 3],
    cable_idx: u32,
    scale: f32,
    connected: u8,
    _pad2: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct WasmOutputPort {
    tag: u8,
    _pad1: [u8; 3],
    cable_idx: u32,
    connected: u8,
    _pad2: [u8; 3],
}

// ── WasmModule ──────────────────────────────────────────────────────────────

/// A `Module` implementation backed by a WASM plugin loaded via wasmtime.
///
/// Each instance owns its own `Store` and `Instance`. The compiled
/// `wasmtime::Module` is shared via `Arc` across all instances from the
/// same `.wasm` file.
pub struct WasmModule {
    store: Store<()>,
    #[allow(dead_code)]
    instance: Instance,
    memory: Memory,

    // Cached typed function handles (no per-call lookup)
    fn_process: TypedFunc<(i32, i32, i32), ()>,
    fn_set_ports: TypedFunc<(i32, i32, i32, i32), ()>,
    fn_update_validated_params: TypedFunc<(i32, i32), ()>,
    fn_update_params: TypedFunc<(i32, i32), i32>,
    fn_periodic_update: TypedFunc<(i32, i32, i32), i32>,
    fn_alloc: TypedFunc<i32, i32>,
    fn_free: TypedFunc<(i32, i32), ()>,

    descriptor: ModuleDescriptor,
    instance_id: InstanceId,
    supports_periodic: bool,

    // Cable remapping: staging_slot[i] <-> host_cable_idx[i]
    input_cable_map: Vec<usize>,  // staging index -> host cable index (inputs)
    output_cable_map: Vec<usize>, // staging index -> host cable index (outputs)
    staging_ptr: i32,             // pointer to staging area in WASM linear memory
    staging_slots: usize,         // total number of staging slots allocated
}

// Safety: wasmtime::Store is Send. The WasmModule must only be used from
// one thread at a time, which is the same contract as all Module impls.
unsafe impl Send for WasmModule {}

impl WasmModule {
    /// Write bytes into WASM linear memory using patches_alloc.
    fn write_bytes(&mut self, data: &[u8]) -> Result<i32, String> {
        let ptr = self.fn_alloc.call(&mut self.store, data.len() as i32)
            .map_err(|e| format!("patches_alloc failed: {e}"))?;
        let mem_data = self.memory.data_mut(&mut self.store);
        let offset = ptr as usize;
        if offset + data.len() > mem_data.len() {
            return Err("WASM memory overflow".to_string());
        }
        mem_data[offset..offset + data.len()].copy_from_slice(data);
        Ok(ptr)
    }

    /// Read a length-prefixed byte buffer from WASM linear memory.
    /// Format: [len: u32 LE, data...]
    fn read_length_prefixed(&self, ptr: i32) -> Result<Vec<u8>, String> {
        let mem_data = self.memory.data(&self.store);
        let offset = ptr as usize;
        if offset + 4 > mem_data.len() {
            return Err("length-prefix read out of bounds".to_string());
        }
        let len = u32::from_le_bytes(
            mem_data[offset..offset + 4].try_into().unwrap()
        ) as usize;
        if offset + 4 + len > mem_data.len() {
            return Err("length-prefixed data out of bounds".to_string());
        }
        Ok(mem_data[offset + 4..offset + 4 + len].to_vec())
    }

    /// Free a length-prefixed buffer (4 + data_len bytes).
    fn free_length_prefixed(&mut self, ptr: i32, data_len: usize) {
        let total = 4 + data_len;
        let _ = self.fn_free.call(&mut self.store, (ptr, total as i32));
    }

    /// Copy input cable data from host pool into WASM staging area.
    fn copy_inputs_in(&mut self, pool: &CablePool<'_>) {
        let mem_data = self.memory.data_mut(&mut self.store);
        let slot_size = std::mem::size_of::<[CableValue; 2]>();

        for (staging_idx, &host_idx) in self.input_cable_map.iter().enumerate() {
            let (pool_ptr, pool_len, _wi) = pool.as_raw_parts();
            if host_idx >= pool_len {
                continue;
            }
            let host_slot = unsafe { &*pool_ptr.add(host_idx) };
            let host_bytes = unsafe {
                std::slice::from_raw_parts(
                    host_slot as *const [CableValue; 2] as *const u8,
                    slot_size,
                )
            };
            let staging_offset = self.staging_ptr as usize + staging_idx * slot_size;
            if staging_offset + slot_size <= mem_data.len() {
                mem_data[staging_offset..staging_offset + slot_size]
                    .copy_from_slice(host_bytes);
            }
        }
    }

    /// Copy output cable data from WASM staging area back to host pool.
    fn copy_outputs_out(&self, pool: &mut CablePool<'_>) {
        let mem_data = self.memory.data(&self.store);
        let slot_size = std::mem::size_of::<[CableValue; 2]>();
        let (pool_ptr, pool_len, wi) = pool.as_raw_parts_mut();

        for (staging_idx, &host_idx) in self.output_cable_map.iter().enumerate() {
            if host_idx >= pool_len {
                continue;
            }
            // Output staging slots are placed after input staging slots
            let actual_staging_idx = self.input_cable_map.len() + staging_idx;
            let staging_offset = self.staging_ptr as usize + actual_staging_idx * slot_size;
            if staging_offset + slot_size > mem_data.len() {
                continue;
            }

            // Read the CableValue from the write slot of the staging area
            let staging_slot = unsafe {
                &*(mem_data.as_ptr().add(staging_offset) as *const [CableValue; 2])
            };
            // Copy only the write slot back to the host pool
            let host_slot = unsafe { &mut *pool_ptr.add(host_idx) };
            host_slot[wi] = staging_slot[wi];
        }
    }
}

impl Module for WasmModule {
    fn describe(_shape: &ModuleShape) -> ModuleDescriptor
    where
        Self: Sized,
    {
        unreachable!("WasmModule::describe is not callable directly; use WasmModuleBuilder")
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        _descriptor: ModuleDescriptor,
        _instance_id: InstanceId,
    ) -> Self
    where
        Self: Sized,
    {
        unreachable!("WasmModule::prepare is not callable directly; use WasmModuleBuilder")
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        let params_json = json::serialize_parameter_map(params);
        let ptr = match self.write_bytes(&params_json) {
            Ok(p) => p,
            Err(_) => return,
        };
        let _ = self.fn_update_validated_params.call(
            &mut self.store,
            (ptr, params_json.len() as i32),
        );
        let _ = self.fn_free.call(&mut self.store, (ptr, params_json.len() as i32));
    }

    fn update_parameters(&mut self, params: &ParameterMap) -> Result<(), BuildError> {
        let params_json = json::serialize_parameter_map(params);
        let ptr = self.write_bytes(&params_json)
            .map_err(|e| BuildError::Custom {
                module: self.descriptor.module_name,
                message: e,
            })?;
        let result = self.fn_update_params.call(
            &mut self.store,
            (ptr, params_json.len() as i32),
        ).map_err(|e| BuildError::Custom {
            module: self.descriptor.module_name,
            message: format!("patches_update_parameters trapped: {e}"),
        })?;
        let _ = self.fn_free.call(&mut self.store, (ptr, params_json.len() as i32));

        if result != 0 {
            // result is a pointer to a length-prefixed error message
            let error_data = self.read_length_prefixed(result)
                .unwrap_or_else(|_| b"unknown error".to_vec());
            let data_len = error_data.len();
            let msg = String::from_utf8_lossy(&error_data).into_owned();
            self.free_length_prefixed(result, data_len);
            Err(BuildError::Custom {
                module: self.descriptor.module_name,
                message: msg,
            })
        } else {
            Ok(())
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        if self.staging_slots == 0 {
            return;
        }

        let (_pool_ptr, _pool_len, wi) = pool.as_raw_parts();

        // Copy inputs into WASM staging area
        self.copy_inputs_in(pool);

        // Call patches_process with staging area pointer
        let _ = self.fn_process.call(
            &mut self.store,
            (self.staging_ptr, self.staging_slots as i32, wi as i32),
        );

        // Copy outputs back from WASM staging area
        self.copy_outputs_out(pool);
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        // Build cable remapping: map host cable indices to 0-based staging indices
        self.input_cable_map.clear();
        self.output_cable_map.clear();

        // Collect host cable indices for inputs
        for input in inputs {
            let host_idx = match input {
                InputPort::Mono(m) => m.cable_idx,
                InputPort::Poly(p) => p.cable_idx,
            };
            self.input_cable_map.push(host_idx);
        }

        // Collect host cable indices for outputs
        for output in outputs {
            let host_idx = match output {
                OutputPort::Mono(m) => m.cable_idx,
                OutputPort::Poly(p) => p.cable_idx,
            };
            self.output_cable_map.push(host_idx);
        }

        let total_slots = inputs.len() + outputs.len();

        // Reallocate staging area if needed
        let slot_size = std::mem::size_of::<[CableValue; 2]>();
        if total_slots != self.staging_slots && total_slots > 0 {
            // Free old staging area
            if self.staging_ptr != 0 {
                let old_size = self.staging_slots * slot_size;
                let _ = self.fn_free.call(&mut self.store, (self.staging_ptr, old_size as i32));
            }
            // Allocate new staging area
            let new_size = total_slots * slot_size;
            self.staging_ptr = self.fn_alloc.call(&mut self.store, new_size as i32)
                .unwrap_or(0);
            self.staging_slots = total_slots;

            // Zero the staging area
            if self.staging_ptr != 0 {
                let mem_data = self.memory.data_mut(&mut self.store);
                let offset = self.staging_ptr as usize;
                if offset + new_size <= mem_data.len() {
                    for byte in &mut mem_data[offset..offset + new_size] {
                        *byte = 0;
                    }
                }
            }
        } else if total_slots == 0 && self.staging_ptr != 0 {
            let old_size = self.staging_slots * slot_size;
            let _ = self.fn_free.call(&mut self.store, (self.staging_ptr, old_size as i32));
            self.staging_ptr = 0;
            self.staging_slots = 0;
        }

        // Remap ports to 0-based staging indices using WASM-compatible wire
        // types (u32 instead of usize to match wasm32 layout).
        let remapped_inputs: Vec<WasmInputPort> = inputs.iter().enumerate().map(|(i, port)| {
            let ffi = FfiInputPort::from(port);
            WasmInputPort {
                tag: ffi.tag,
                _pad1: [0; 3],
                cable_idx: i as u32, // 0-based staging index
                scale: ffi.scale,
                connected: ffi.connected,
                _pad2: [0; 3],
            }
        }).collect();

        let remapped_outputs: Vec<WasmOutputPort> = outputs.iter().enumerate().map(|(i, port)| {
            let ffi = FfiOutputPort::from(port);
            WasmOutputPort {
                tag: ffi.tag,
                _pad1: [0; 3],
                cable_idx: (inputs.len() + i) as u32, // after inputs in staging area
                connected: ffi.connected,
                _pad2: [0; 3],
            }
        }).collect();

        // Write port structs into WASM memory and call patches_set_ports
        let input_bytes = unsafe {
            std::slice::from_raw_parts(
                remapped_inputs.as_ptr() as *const u8,
                remapped_inputs.len() * std::mem::size_of::<WasmInputPort>(),
            )
        };
        let output_bytes = unsafe {
            std::slice::from_raw_parts(
                remapped_outputs.as_ptr() as *const u8,
                remapped_outputs.len() * std::mem::size_of::<WasmOutputPort>(),
            )
        };

        let inputs_ptr = if input_bytes.is_empty() {
            0
        } else {
            self.write_bytes(input_bytes).unwrap_or(0)
        };
        let outputs_ptr = if output_bytes.is_empty() {
            0
        } else {
            self.write_bytes(output_bytes).unwrap_or(0)
        };

        let _ = self.fn_set_ports.call(
            &mut self.store,
            (inputs_ptr, remapped_inputs.len() as i32,
             outputs_ptr, remapped_outputs.len() as i32),
        );

        // Free port data
        if inputs_ptr != 0 {
            let _ = self.fn_free.call(&mut self.store, (inputs_ptr, input_bytes.len() as i32));
        }
        if outputs_ptr != 0 {
            let _ = self.fn_free.call(&mut self.store, (outputs_ptr, output_bytes.len() as i32));
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        if self.supports_periodic {
            Some(self)
        } else {
            None
        }
    }
}

impl PeriodicUpdate for WasmModule {
    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        if self.staging_slots == 0 {
            return;
        }
        let (_pool_ptr, _pool_len, wi) = pool.as_raw_parts();
        self.copy_inputs_in(pool);
        let _ = self.fn_periodic_update.call(
            &mut self.store,
            (self.staging_ptr, self.staging_slots as i32, wi as i32),
        );
    }
}

// ── WasmModuleBuilder ───────────────────────────────────────────────────────

/// A `ModuleBuilder` that creates `WasmModule` instances from a compiled
/// WASM plugin. The compiled `wasmtime::Module` is shared via `Arc` across
/// all instances.
pub struct WasmModuleBuilder {
    engine: Arc<Engine>,
    compiled: Arc<WtModule>,
}

// Safety: wasmtime Engine and Module are Send+Sync.
unsafe impl Send for WasmModuleBuilder {}
unsafe impl Sync for WasmModuleBuilder {}

impl WasmModuleBuilder {
    fn create_instance(&self) -> Result<(Store<()>, Instance, Memory), String> {
        let mut store = Store::new(&self.engine, ());
        let instance = Instance::new(&mut store, &self.compiled, &[])
            .map_err(|e| format!("failed to instantiate WASM module: {e}"))?;
        let memory = instance.get_memory(&mut store, "memory")
            .ok_or("WASM module has no 'memory' export")?;
        Ok((store, instance, memory))
    }
}

impl ModuleBuilder for WasmModuleBuilder {
    fn describe(&self, shape: &ModuleShape) -> ModuleDescriptor {
        let (mut store, instance, memory) = self.create_instance()
            .expect("failed to create WASM instance for describe");

        let fn_describe: TypedFunc<(i32, i32, i32), i32> = instance
            .get_typed_func(&mut store, "patches_describe")
            .expect("WASM module missing patches_describe export");

        let result_ptr = fn_describe.call(
            &mut store,
            (shape.channels as i32, shape.length as i32, shape.high_quality as i32),
        ).expect("patches_describe trapped");

        // Read length-prefixed JSON from WASM memory
        let mem_data = memory.data(&store);
        let offset = result_ptr as usize;
        let len = u32::from_le_bytes(
            mem_data[offset..offset + 4].try_into().unwrap()
        ) as usize;
        let json_data = &mem_data[offset + 4..offset + 4 + len];

        json::deserialize_module_descriptor(json_data)
            .expect("patches_describe returned invalid JSON")
    }

    fn build(
        &self,
        audio_environment: &AudioEnvironment,
        shape: &ModuleShape,
        params: &ParameterMap,
        instance_id: InstanceId,
    ) -> Result<Box<dyn Module>, BuildError> {
        let (mut store, instance, memory) = self.create_instance()
            .map_err(|e| BuildError::Custom { module: "wasm", message: e })?;

        // Get all function handles
        let fn_describe: TypedFunc<(i32, i32, i32), i32> = instance
            .get_typed_func(&mut store, "patches_describe")
            .map_err(|e| BuildError::Custom { module: "wasm", message: format!("{e}") })?;
        let fn_prepare: PrepareFunc = instance
            .get_typed_func(&mut store, "patches_prepare")
            .map_err(|e| BuildError::Custom { module: "wasm", message: format!("{e}") })?;
        let fn_process: TypedFunc<(i32, i32, i32), ()> = instance
            .get_typed_func(&mut store, "patches_process")
            .map_err(|e| BuildError::Custom { module: "wasm", message: format!("{e}") })?;
        let fn_set_ports: TypedFunc<(i32, i32, i32, i32), ()> = instance
            .get_typed_func(&mut store, "patches_set_ports")
            .map_err(|e| BuildError::Custom { module: "wasm", message: format!("{e}") })?;
        let fn_update_validated_params: TypedFunc<(i32, i32), ()> = instance
            .get_typed_func(&mut store, "patches_update_validated_parameters")
            .map_err(|e| BuildError::Custom { module: "wasm", message: format!("{e}") })?;
        let fn_update_params: TypedFunc<(i32, i32), i32> = instance
            .get_typed_func(&mut store, "patches_update_parameters")
            .map_err(|e| BuildError::Custom { module: "wasm", message: format!("{e}") })?;
        let fn_periodic_update: TypedFunc<(i32, i32, i32), i32> = instance
            .get_typed_func(&mut store, "patches_periodic_update")
            .map_err(|e| BuildError::Custom { module: "wasm", message: format!("{e}") })?;
        let fn_supports_periodic: TypedFunc<(), i32> = instance
            .get_typed_func(&mut store, "patches_supports_periodic")
            .map_err(|e| BuildError::Custom { module: "wasm", message: format!("{e}") })?;
        let fn_alloc: TypedFunc<i32, i32> = instance
            .get_typed_func(&mut store, "patches_alloc")
            .map_err(|e| BuildError::Custom { module: "wasm", message: format!("{e}") })?;
        let fn_free: TypedFunc<(i32, i32), ()> = instance
            .get_typed_func(&mut store, "patches_free")
            .map_err(|e| BuildError::Custom { module: "wasm", message: format!("{e}") })?;

        // 1. Describe
        let result_ptr = fn_describe.call(
            &mut store,
            (shape.channels as i32, shape.length as i32, shape.high_quality as i32),
        ).map_err(|e| BuildError::Custom { module: "wasm", message: format!("describe: {e}") })?;

        let descriptor = {
            let mem_data = memory.data(&store);
            let offset = result_ptr as usize;
            let len = u32::from_le_bytes(
                mem_data[offset..offset + 4].try_into().unwrap()
            ) as usize;
            let json_data = &mem_data[offset + 4..offset + 4 + len];
            json::deserialize_module_descriptor(json_data)
                .map_err(|e| BuildError::Custom { module: "wasm", message: e })?
        };

        // Free the describe result
        {
            let mem_data = memory.data(&store);
            let offset = result_ptr as usize;
            let len = u32::from_le_bytes(
                mem_data[offset..offset + 4].try_into().unwrap()
            ) as usize;
            let _ = fn_free.call(&mut store, (result_ptr, (4 + len) as i32));
        }

        // 2. Serialize descriptor for prepare
        let desc_json = json::serialize_module_descriptor(&descriptor);
        let desc_ptr = fn_alloc.call(&mut store, desc_json.len() as i32)
            .map_err(|e| BuildError::Custom { module: descriptor.module_name, message: format!("alloc: {e}") })?;
        {
            let mem_data = memory.data_mut(&mut store);
            let offset = desc_ptr as usize;
            mem_data[offset..offset + desc_json.len()].copy_from_slice(&desc_json);
        }

        // Split instance_id into lo/hi
        let id_raw = instance_id.as_u64();
        let id_lo = id_raw as u32 as i32;
        let id_hi = (id_raw >> 32) as u32 as i32;

        // 3. Prepare
        fn_prepare.call(
            &mut store,
            (
                desc_ptr,
                desc_json.len() as i32,
                audio_environment.sample_rate,
                audio_environment.poly_voices as i32,
                audio_environment.periodic_update_interval as i32,
                id_lo,
                id_hi,
            ),
        ).map_err(|e| BuildError::Custom { module: descriptor.module_name, message: format!("prepare: {e}") })?;

        let _ = fn_free.call(&mut store, (desc_ptr, desc_json.len() as i32));

        // Check periodic support
        let supports_periodic = fn_supports_periodic.call(&mut store, ())
            .map(|v| v != 0)
            .unwrap_or(false);

        let mut module = WasmModule {
            store,
            instance,
            memory,
            fn_process,
            fn_set_ports,
            fn_update_validated_params,
            fn_update_params,
            fn_periodic_update,
            fn_alloc,
            fn_free,
            descriptor,
            instance_id,
            supports_periodic,
            input_cable_map: Vec::new(),
            output_cable_map: Vec::new(),
            staging_ptr: 0,
            staging_slots: 0,
        };

        // 4. Fill defaults and validate+apply parameters
        let mut filled = params.clone();
        for param_desc in module.descriptor.parameters.iter() {
            filled.get_or_insert(param_desc.name, param_desc.index, || {
                param_desc.parameter_type.default_value()
            });
        }
        module.update_parameters(&filled)?;

        Ok(Box::new(module))
    }
}

// ── load_wasm_plugin ────────────────────────────────────────────────────────

/// Load a WASM plugin from a `.wasm` file.
///
/// Returns a `WasmModuleBuilder` that can be registered in the `Registry`.
/// The `Engine` is shared across all plugins for efficiency.
///
/// # Errors
/// Returns an error if the file cannot be read or compiled.
pub fn load_wasm_plugin(
    engine: &Arc<Engine>,
    path: &Path,
) -> Result<WasmModuleBuilder, String> {
    let compiled = cache::load_or_compile(engine, path)?;
    Ok(WasmModuleBuilder {
        engine: Arc::clone(engine),
        compiled: Arc::new(compiled),
    })
}
