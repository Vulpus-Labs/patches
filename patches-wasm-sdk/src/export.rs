//! The `export_wasm_module!` macro generates WASM export functions for a
//! `Module` implementation, analogous to `export_module!` in `patches-ffi`.

/// Generate WASM export functions for a module type.
///
/// # Usage
/// ```ignore
/// use patches_core::*;
/// use patches_wasm_sdk::export_wasm_module;
///
/// pub struct MyModule { /* ... */ }
/// impl Module for MyModule { /* ... */ }
/// export_wasm_module!(MyModule);
/// ```
///
/// This generates all `#[no_mangle]` WASM exports needed by the host-side
/// `patches-wasm` loader. The module singleton is stored as `static mut`
/// (safe: WASM is single-threaded).
#[macro_export]
macro_rules! export_wasm_module {
    ($module_type:ty) => {
        static mut __PATCHES_WASM_MODULE: Option<$module_type> = None;

        /// Return the module descriptor as JSON. Returns a pointer to
        /// `[len: u32, data...]` in WASM linear memory.
        #[no_mangle]
        pub extern "C" fn patches_describe(channels: i32, length: i32, hq: i32) -> i32 {
            let shape = $crate::__reexport::ModuleShape {
                channels: channels as usize,
                length: length as usize,
                high_quality: hq != 0,
            };
            let desc = <$module_type as $crate::__reexport::Module>::describe(&shape);
            let json = $crate::__reexport::serialize_module_descriptor(&desc);
            $crate::__wasm_return_bytes(&json)
        }

        /// Initialise the module singleton.
        ///
        /// `instance_id` is split into lo/hi i32 halves because WASM MVP
        /// does not support i64 parameters in all runtimes.
        #[no_mangle]
        pub extern "C" fn patches_prepare(
            desc_ptr: i32,
            desc_len: i32,
            sample_rate: f32,
            poly_voices: i32,
            periodic_interval: i32,
            instance_id_lo: i32,
            instance_id_hi: i32,
        ) {
            let json_slice = unsafe {
                ::core::slice::from_raw_parts(desc_ptr as *const u8, desc_len as usize)
            };
            let descriptor = $crate::__reexport::deserialize_module_descriptor(json_slice)
                .expect("invalid descriptor JSON in patches_prepare");
            let env = $crate::__reexport::AudioEnvironment {
                sample_rate,
                poly_voices: poly_voices as usize,
                periodic_update_interval: periodic_interval as u32,
            };
            let id_raw = (instance_id_lo as u32 as u64) | ((instance_id_hi as u32 as u64) << 32);
            let instance_id = $crate::__reexport::InstanceId::from_raw(id_raw);
            let module = <$module_type as $crate::__reexport::Module>::prepare(
                &env, descriptor, instance_id,
            );
            unsafe { __PATCHES_WASM_MODULE = Some(module); }
        }

        /// Process one sample. The staging area at `cable_ptr` contains
        /// `[CableValue; 2]` entries with 0-based cable indices.
        #[no_mangle]
        pub extern "C" fn patches_process(cable_ptr: i32, cable_count: i32, write_index: i32) {
            let module = unsafe { __PATCHES_WASM_MODULE.as_mut().expect("module not prepared") };
            let pool_slice = unsafe {
                ::core::slice::from_raw_parts_mut(
                    cable_ptr as *mut [$crate::__reexport::CableValue; 2],
                    cable_count as usize,
                )
            };
            let mut pool = $crate::__reexport::CablePool::new(pool_slice, write_index as usize);
            $crate::__reexport::Module::process(module, &mut pool);
        }

        /// Deliver remapped port objects. Port cable indices are 0-based
        /// into the staging area.
        #[no_mangle]
        pub extern "C" fn patches_set_ports(
            inputs_ptr: i32,
            inputs_len: i32,
            outputs_ptr: i32,
            outputs_len: i32,
        ) {
            let module = unsafe { __PATCHES_WASM_MODULE.as_mut().expect("module not prepared") };
            let ffi_inputs = unsafe {
                ::core::slice::from_raw_parts(
                    inputs_ptr as *const $crate::__reexport::FfiInputPort,
                    inputs_len as usize,
                )
            };
            let ffi_outputs = unsafe {
                ::core::slice::from_raw_parts(
                    outputs_ptr as *const $crate::__reexport::FfiOutputPort,
                    outputs_len as usize,
                )
            };
            let inputs: ::std::vec::Vec<$crate::__reexport::InputPort> =
                ffi_inputs.iter().map(|p| (*p).into()).collect();
            let outputs: ::std::vec::Vec<$crate::__reexport::OutputPort> =
                ffi_outputs.iter().map(|p| (*p).into()).collect();
            $crate::__reexport::Module::set_ports(module, &inputs, &outputs);
        }

        /// Apply pre-validated parameters (JSON).
        #[no_mangle]
        pub extern "C" fn patches_update_validated_parameters(params_ptr: i32, params_len: i32) {
            let module = unsafe { __PATCHES_WASM_MODULE.as_mut().expect("module not prepared") };
            let json_slice = unsafe {
                ::core::slice::from_raw_parts(params_ptr as *const u8, params_len as usize)
            };
            let mut params = $crate::__reexport::deserialize_parameter_map(json_slice)
                .expect("invalid params JSON");
            $crate::__reexport::Module::update_validated_parameters(module, &mut params);
        }

        /// Validate and apply parameters. Returns a pointer to error
        /// message (length-prefixed) on failure, or 0 on success.
        #[no_mangle]
        pub extern "C" fn patches_update_parameters(params_ptr: i32, params_len: i32) -> i32 {
            let module = unsafe { __PATCHES_WASM_MODULE.as_mut().expect("module not prepared") };
            let json_slice = unsafe {
                ::core::slice::from_raw_parts(params_ptr as *const u8, params_len as usize)
            };
            let params = $crate::__reexport::deserialize_parameter_map(json_slice)
                .expect("invalid params JSON");
            match $crate::__reexport::Module::update_parameters(module, &params) {
                Ok(()) => 0,
                Err(e) => {
                    let msg = format!("{e}");
                    $crate::__wasm_return_bytes(msg.as_bytes())
                }
            }
        }

        /// Periodic coefficient update. Returns 0 if unsupported, 1 if executed.
        #[no_mangle]
        pub extern "C" fn patches_periodic_update(
            cable_ptr: i32,
            cable_count: i32,
            write_index: i32,
        ) -> i32 {
            let module = unsafe { __PATCHES_WASM_MODULE.as_mut().expect("module not prepared") };
            if let Some(periodic) = $crate::__reexport::Module::as_periodic(module) {
                let pool_slice = unsafe {
                    ::core::slice::from_raw_parts_mut(
                        cable_ptr as *mut [$crate::__reexport::CableValue; 2],
                        cable_count as usize,
                    )
                };
                let pool = $crate::__reexport::CablePool::new(pool_slice, write_index as usize);
                periodic.periodic_update(&pool);
                1
            } else {
                0
            }
        }

        /// Returns 1 if periodic update is supported, 0 otherwise.
        /// Always returns 1; runtime check via `patches_periodic_update`
        /// determines actual support.
        #[no_mangle]
        pub extern "C" fn patches_supports_periodic() -> i32 {
            1
        }

        /// Allocate memory in WASM linear memory. Used by the host to
        /// write data (JSON, port arrays) into the module's address space.
        #[no_mangle]
        pub extern "C" fn patches_alloc(size: i32) -> i32 {
            let layout = ::core::alloc::Layout::from_size_align(size as usize, 8)
                .expect("invalid alloc layout");
            let ptr = unsafe { ::std::alloc::alloc(layout) };
            ptr as i32
        }

        /// Free memory in WASM linear memory.
        #[no_mangle]
        pub extern "C" fn patches_free(ptr: i32, size: i32) {
            if ptr == 0 { return; }
            let layout = ::core::alloc::Layout::from_size_align(size as usize, 8)
                .expect("invalid free layout");
            unsafe { ::std::alloc::dealloc(ptr as *mut u8, layout); }
        }
    };
}
