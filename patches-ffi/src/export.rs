//! The `export_module!` macro generates the `extern "C"` entry point and all
//! vtable wrapper functions for a `Module` implementation.

/// Generate the FFI entry point and vtable wrappers for a module type.
///
/// # Usage
/// ```ignore
/// use patches_core::*;
/// use patches_ffi::export_module;
///
/// pub struct MyModule { /* ... */ }
/// impl Module for MyModule { /* ... */ }
/// export_module!(MyModule);
/// ```
///
/// This generates:
/// - `#[no_mangle] pub extern "C" fn patches_plugin_init() -> FfiPluginVTable`
/// - All `extern "C"` wrapper functions for each vtable entry
/// - `catch_unwind` on every wrapper to prevent UB from unwinding across FFI
#[macro_export]
macro_rules! export_module {
    ($module_type:ty) => {
        #[no_mangle]
        pub extern "C" fn patches_plugin_init() -> $crate::types::FfiPluginVTable {
            $crate::types::FfiPluginVTable {
                abi_version: $crate::types::ABI_VERSION,
                supports_periodic: {
                    // We always generate the periodic wrapper; at runtime it
                    // checks as_periodic() and returns 0 if unsupported.
                    // To set the flag correctly, we use a trait-detection trick:
                    // we try to call as_periodic on a null-like check. Instead,
                    // we just set it to 1 and let the runtime check handle it.
                    // This is option (b) from the ticket: negligible cost.
                    1
                },
                describe: __patches_ffi_describe::<$module_type>,
                prepare: __patches_ffi_prepare::<$module_type>,
                update_validated_parameters: __patches_ffi_update_validated_parameters,
                update_parameters: __patches_ffi_update_parameters,
                process: __patches_ffi_process,
                set_ports: __patches_ffi_set_ports,
                periodic_update: __patches_ffi_periodic_update,
                descriptor: __patches_ffi_descriptor,
                instance_id: __patches_ffi_instance_id,
                drop: __patches_ffi_drop::<$module_type>,
                free_bytes: __patches_ffi_free_bytes,
            }
        }

        unsafe extern "C" fn __patches_ffi_describe<T: $crate::__reexport::Module>(
            shape: $crate::types::FfiModuleShape,
        ) -> $crate::types::FfiBytes {
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                let shape: $crate::__reexport::ModuleShape = shape.into();
                let desc = T::describe(&shape);
                let json = $crate::json::serialize_module_descriptor(&desc);
                $crate::types::FfiBytes::from_vec(json)
            }));
            match result {
                Ok(bytes) => bytes,
                Err(_) => {
                    let err = b"plugin panicked in describe".to_vec();
                    $crate::types::FfiBytes::from_vec(err)
                }
            }
        }

        unsafe extern "C" fn __patches_ffi_prepare<T: $crate::__reexport::Module>(
            descriptor_json: *const u8,
            descriptor_json_len: usize,
            env: $crate::types::FfiAudioEnvironment,
            instance_id: u64,
        ) -> *mut ::std::ffi::c_void {
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                let json_slice = unsafe { ::std::slice::from_raw_parts(descriptor_json, descriptor_json_len) };
                let descriptor = $crate::json::deserialize_module_descriptor(json_slice)
                    .expect("invalid descriptor JSON in prepare");
                let audio_env: $crate::__reexport::AudioEnvironment = env.into();
                let iid = $crate::__reexport::InstanceId::from_raw(instance_id);
                let module = T::prepare(&audio_env, descriptor, iid);
                let boxed = Box::new(module);
                Box::into_raw(boxed) as *mut ::std::ffi::c_void
            }));
            match result {
                Ok(ptr) => ptr,
                Err(_) => ::std::ptr::null_mut(),
            }
        }

        unsafe extern "C" fn __patches_ffi_update_validated_parameters(
            handle: *mut ::std::ffi::c_void,
            params_json: *const u8,
            params_json_len: usize,
        ) {
            let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                let module = unsafe { &mut *(handle as *mut $module_type) };
                let json_slice = unsafe { ::std::slice::from_raw_parts(params_json, params_json_len) };
                let mut params = $crate::json::deserialize_parameter_map(json_slice)
                    .expect("invalid params JSON in update_validated_parameters");
                $crate::__reexport::Module::update_validated_parameters(module, &mut params);
            }));
        }

        unsafe extern "C" fn __patches_ffi_update_parameters(
            handle: *mut ::std::ffi::c_void,
            params_json: *const u8,
            params_json_len: usize,
            error_out: *mut $crate::types::FfiBytes,
        ) -> i32 {
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                let module = unsafe { &mut *(handle as *mut $module_type) };
                let json_slice = unsafe { ::std::slice::from_raw_parts(params_json, params_json_len) };
                let params = $crate::json::deserialize_parameter_map(json_slice)
                    .expect("invalid params JSON in update_parameters");
                $crate::__reexport::Module::update_parameters(module, &params)
            }));
            match result {
                Ok(Ok(())) => 0,
                Ok(Err(e)) => {
                    let msg = format!("{e}");
                    unsafe { *error_out = $crate::types::FfiBytes::from_vec(msg.into_bytes()) };
                    1
                }
                Err(_) => {
                    let msg = b"plugin panicked in update_parameters".to_vec();
                    unsafe { *error_out = $crate::types::FfiBytes::from_vec(msg) };
                    1
                }
            }
        }

        unsafe extern "C" fn __patches_ffi_process(
            handle: *mut ::std::ffi::c_void,
            pool_ptr: *mut [$crate::__reexport::CableValue; 2],
            pool_len: usize,
            write_index: usize,
        ) {
            let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                let module = unsafe { &mut *(handle as *mut $module_type) };
                let pool_slice = unsafe { ::std::slice::from_raw_parts_mut(pool_ptr, pool_len) };
                let mut pool = $crate::__reexport::CablePool::new(pool_slice, write_index);
                $crate::__reexport::Module::process(module, &mut pool);
            }));
            // On panic: module produces silence (cables unchanged). This is
            // the safest behavior — no undefined state in the output cables.
        }

        unsafe extern "C" fn __patches_ffi_set_ports(
            handle: *mut ::std::ffi::c_void,
            inputs: *const $crate::types::FfiInputPort,
            inputs_len: usize,
            outputs: *const $crate::types::FfiOutputPort,
            outputs_len: usize,
        ) {
            let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                let module = unsafe { &mut *(handle as *mut $module_type) };
                let ffi_inputs = unsafe { ::std::slice::from_raw_parts(inputs, inputs_len) };
                let ffi_outputs = unsafe { ::std::slice::from_raw_parts(outputs, outputs_len) };
                let inputs: Vec<$crate::__reexport::InputPort> = ffi_inputs.iter().map(|p| (*p).into()).collect();
                let outputs: Vec<$crate::__reexport::OutputPort> = ffi_outputs.iter().map(|p| (*p).into()).collect();
                $crate::__reexport::Module::set_ports(module, &inputs, &outputs);
            }));
        }

        unsafe extern "C" fn __patches_ffi_periodic_update(
            handle: *mut ::std::ffi::c_void,
            pool_ptr: *const [$crate::__reexport::CableValue; 2],
            pool_len: usize,
            write_index: usize,
        ) -> i32 {
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                let module = unsafe { &mut *(handle as *mut $module_type) };
                if let Some(periodic) = $crate::__reexport::Module::as_periodic(module) {
                    // Reconstruct a read-only CablePool. We cast const to mut for the
                    // CablePool constructor, but the periodic_update contract is read-only.
                    let pool_slice = unsafe {
                        ::std::slice::from_raw_parts_mut(pool_ptr as *mut _, pool_len)
                    };
                    let pool = $crate::__reexport::CablePool::new(pool_slice, write_index);
                    periodic.periodic_update(&pool);
                    1
                } else {
                    0
                }
            }));
            result.unwrap_or(0)
        }

        unsafe extern "C" fn __patches_ffi_descriptor(
            handle: *mut ::std::ffi::c_void,
        ) -> $crate::types::FfiBytes {
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                let module = unsafe { &*(handle as *const $module_type) };
                let desc = $crate::__reexport::Module::descriptor(module);
                let json = $crate::json::serialize_module_descriptor(desc);
                $crate::types::FfiBytes::from_vec(json)
            }));
            match result {
                Ok(bytes) => bytes,
                Err(_) => $crate::types::FfiBytes::from_vec(
                    b"plugin panicked in descriptor".to_vec()
                ),
            }
        }

        unsafe extern "C" fn __patches_ffi_instance_id(
            handle: *mut ::std::ffi::c_void,
        ) -> u64 {
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                let module = unsafe { &*(handle as *const $module_type) };
                $crate::__reexport::Module::instance_id(module).as_u64()
            }));
            result.unwrap_or(u64::MAX)
        }

        unsafe extern "C" fn __patches_ffi_drop<T>(
            handle: *mut ::std::ffi::c_void,
        ) {
            let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                let _ = unsafe { Box::from_raw(handle as *mut T) };
            }));
        }

        unsafe extern "C" fn __patches_ffi_free_bytes(
            bytes: $crate::types::FfiBytes,
        ) {
            let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                if !bytes.ptr.is_null() {
                    let _ = unsafe { bytes.reclaim() };
                }
            }));
        }
    };
}
