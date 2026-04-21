//! `export_modules!` and `export_module!` macros.
//!
//! `export_modules!(T1, T2, ...)` generates the FFI entry point
//! (`patches_plugin_init`) plus generic `extern "C"` wrappers that dispatch
//! through the `Module` trait. The entry returns an `FfiPluginManifest`
//! pointing at a plugin-static `[FfiPluginVTable; N]`, one vtable per module
//! type. `export_module!(T)` is preserved as a thin shim calling
//! `export_modules!(T)`.
//!
//! Every generated `extern "C"` function is wrapped in `catch_unwind` to
//! prevent UB from unwinding across the ABI boundary.

/// Generate the FFI entry point and vtables for one or more module types.
#[macro_export]
macro_rules! export_modules {
    ( $( $module_type:ty $( = $version:expr )? ),+ $(,)? ) => {
        #[doc(hidden)]
        mod __patches_ffi_generated {
            use super::*;

            pub unsafe extern "C" fn describe<T: $crate::__reexport::Module>(
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
                    Err(_) => $crate::types::FfiBytes::from_vec(
                        b"plugin panicked in describe".to_vec(),
                    ),
                }
            }

            pub unsafe extern "C" fn prepare<T: $crate::__reexport::Module>(
                descriptor_json: *const u8,
                descriptor_json_len: usize,
                env: $crate::types::FfiAudioEnvironment,
                instance_id: u64,
            ) -> *mut ::std::ffi::c_void {
                let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    let json_slice = unsafe {
                        ::std::slice::from_raw_parts(descriptor_json, descriptor_json_len)
                    };
                    let descriptor = $crate::json::deserialize_module_descriptor(json_slice)
                        .expect("invalid descriptor JSON in prepare");
                    let audio_env: $crate::__reexport::AudioEnvironment = env.into();
                    let iid = $crate::__reexport::InstanceId::from_raw(instance_id);
                    let module = T::prepare(&audio_env, descriptor, iid);
                    Box::into_raw(Box::new(module)) as *mut ::std::ffi::c_void
                }));
                result.unwrap_or(::std::ptr::null_mut())
            }

            pub unsafe extern "C" fn update_validated_parameters<T: $crate::__reexport::Module>(
                handle: *mut ::std::ffi::c_void,
                params_json: *const u8,
                params_json_len: usize,
            ) {
                let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    // ADR 0045 Spike 5 bridge: rebuild ParamView on the plugin side
                    // from the JSON ParameterMap until Spike 7 replaces this path.
                    let module = unsafe { &mut *(handle as *mut T) };
                    let json_slice = unsafe {
                        ::std::slice::from_raw_parts(params_json, params_json_len)
                    };
                    let map = $crate::json::deserialize_parameter_map(json_slice)
                        .expect("invalid params JSON in update_validated_parameters");
                    let descriptor = $crate::__reexport::Module::descriptor(module);
                    let layout = $crate::__reexport::compute_layout(descriptor);
                    let index = $crate::__reexport::ParamViewIndex::from_layout(&layout);
                    let mut frame = $crate::__reexport::ParamFrame::with_layout(&layout);
                    let defaults = $crate::__reexport::defaults_from_descriptor(descriptor);
                    $crate::__reexport::pack_into(&layout, &defaults, &map, &mut frame)
                        .expect("pack_into failed in update_validated_parameters");
                    let view = $crate::__reexport::ParamView::new(&index, &frame);
                    $crate::__reexport::Module::update_validated_parameters(module, &view);
                }));
            }

            pub unsafe extern "C" fn update_parameters<T: $crate::__reexport::Module>(
                handle: *mut ::std::ffi::c_void,
                params_json: *const u8,
                params_json_len: usize,
                error_out: *mut $crate::types::FfiBytes,
            ) -> i32 {
                let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    let module = unsafe { &mut *(handle as *mut T) };
                    let json_slice = unsafe {
                        ::std::slice::from_raw_parts(params_json, params_json_len)
                    };
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

            pub unsafe extern "C" fn process<T: $crate::__reexport::Module>(
                handle: *mut ::std::ffi::c_void,
                pool_ptr: *mut [$crate::__reexport::CableValue; 2],
                pool_len: usize,
                write_index: usize,
            ) {
                let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    let module = unsafe { &mut *(handle as *mut T) };
                    let pool_slice = unsafe {
                        ::std::slice::from_raw_parts_mut(pool_ptr, pool_len)
                    };
                    let mut pool = $crate::__reexport::CablePool::new(pool_slice, write_index);
                    $crate::__reexport::Module::process(module, &mut pool);
                }));
            }

            pub unsafe extern "C" fn set_ports<T: $crate::__reexport::Module>(
                handle: *mut ::std::ffi::c_void,
                inputs: *const $crate::types::FfiInputPort,
                inputs_len: usize,
                outputs: *const $crate::types::FfiOutputPort,
                outputs_len: usize,
            ) {
                let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    let module = unsafe { &mut *(handle as *mut T) };
                    let ffi_inputs = unsafe {
                        ::std::slice::from_raw_parts(inputs, inputs_len)
                    };
                    let ffi_outputs = unsafe {
                        ::std::slice::from_raw_parts(outputs, outputs_len)
                    };
                    let inputs: Vec<$crate::__reexport::InputPort> =
                        ffi_inputs.iter().map(|p| (*p).into()).collect();
                    let outputs: Vec<$crate::__reexport::OutputPort> =
                        ffi_outputs.iter().map(|p| (*p).into()).collect();
                    $crate::__reexport::Module::set_ports(module, &inputs, &outputs);
                }));
            }

            pub unsafe extern "C" fn periodic_update<T: $crate::__reexport::Module>(
                handle: *mut ::std::ffi::c_void,
                pool_ptr: *const [$crate::__reexport::CableValue; 2],
                pool_len: usize,
                write_index: usize,
            ) -> i32 {
                let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    let module = unsafe { &mut *(handle as *mut T) };
                    if let Some(periodic) = $crate::__reexport::Module::as_periodic(module) {
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

            pub unsafe extern "C" fn descriptor<T: $crate::__reexport::Module>(
                handle: *mut ::std::ffi::c_void,
            ) -> $crate::types::FfiBytes {
                let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    let module = unsafe { &*(handle as *const T) };
                    let desc = $crate::__reexport::Module::descriptor(module);
                    let json = $crate::json::serialize_module_descriptor(desc);
                    $crate::types::FfiBytes::from_vec(json)
                }));
                match result {
                    Ok(bytes) => bytes,
                    Err(_) => $crate::types::FfiBytes::from_vec(
                        b"plugin panicked in descriptor".to_vec(),
                    ),
                }
            }

            pub unsafe extern "C" fn instance_id<T: $crate::__reexport::Module>(
                handle: *mut ::std::ffi::c_void,
            ) -> u64 {
                let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    let module = unsafe { &*(handle as *const T) };
                    $crate::__reexport::Module::instance_id(module).as_u64()
                }));
                result.unwrap_or(u64::MAX)
            }

            pub unsafe extern "C" fn drop_module<T>(
                handle: *mut ::std::ffi::c_void,
            ) {
                let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    let _ = unsafe { Box::from_raw(handle as *mut T) };
                }));
            }

            pub unsafe extern "C" fn free_bytes(
                bytes: $crate::types::FfiBytes,
            ) {
                let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    if !bytes.ptr.is_null() {
                        let _ = unsafe { bytes.reclaim() };
                    }
                }));
            }

            pub fn make_vtable<T: $crate::__reexport::Module>(module_version: u32)
                -> $crate::types::FfiPluginVTable
            {
                $crate::types::FfiPluginVTable {
                    abi_version: $crate::types::ABI_VERSION,
                    module_version,
                    supports_periodic: 1,
                    describe: describe::<T>,
                    prepare: prepare::<T>,
                    update_validated_parameters: update_validated_parameters::<T>,
                    update_parameters: update_parameters::<T>,
                    process: process::<T>,
                    set_ports: set_ports::<T>,
                    periodic_update: periodic_update::<T>,
                    descriptor: descriptor::<T>,
                    instance_id: instance_id::<T>,
                    drop: drop_module::<T>,
                    free_bytes,
                }
            }
        }

        #[no_mangle]
        pub extern "C" fn patches_plugin_init() -> $crate::types::FfiPluginManifest {
            use ::std::sync::OnceLock;
            static VTABLES: OnceLock<Vec<$crate::types::FfiPluginVTable>> = OnceLock::new();
            let v = VTABLES.get_or_init(|| {
                vec![
                    $( __patches_ffi_generated::make_vtable::<$module_type>(
                        $crate::__module_version!( $($version)? )
                    ) ),+
                ]
            });
            $crate::types::FfiPluginManifest {
                abi_version: $crate::types::ABI_VERSION,
                count: v.len(),
                vtables: v.as_ptr(),
            }
        }
    };
}

/// Single-module compatibility shim. Prefer `export_modules!` for new code.
#[macro_export]
macro_rules! export_module {
    ($module_type:ty $( = $version:expr )?) => {
        $crate::export_modules!($module_type $( = $version )?);
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __module_version {
    () => { 0u32 };
    ($v:expr) => { ($v) as u32 };
}
