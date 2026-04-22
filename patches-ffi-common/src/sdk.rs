//! Plugin-side SDK: zero-alloc decoders and the `export_plugin!` macro.
//!
//! ADR 0045 Spike 7 Phase C (E105). A plugin crate drops the hand-written
//! `extern "C"` glue, implements [`patches_core::Module`], and invokes
//! [`export_plugin!`] to emit the eight ABI entry points required by the
//! host vtable.

use patches_core::param_frame::{ParamView, ParamViewIndex};

use crate::port_frame::{PortLayout, PortView};

/// Errors raised by `decode_param_frame` / `decode_port_frame` in release
/// builds. Debug builds panic instead (Spike 5 pack-guard style).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    ParamFrameLenMismatch { expected: usize, actual: usize },
    PortFrameLenMismatch { expected: usize, actual: usize },
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParamFrameLenMismatch { expected, actual } => write!(
                f,
                "param-frame length mismatch: expected {expected}, got {actual}"
            ),
            Self::PortFrameLenMismatch { expected, actual } => write!(
                f,
                "port-frame length mismatch: expected {expected}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Reconstruct a [`ParamView`] from host-supplied wire bytes and a prepared
/// index. Debug-panics on length mismatch; release returns `Err`.
#[inline]
pub fn decode_param_frame<'a>(
    bytes: &'a [u8],
    index: &'a ParamViewIndex,
) -> Result<ParamView<'a>, DecodeError> {
    let expected = ParamView::wire_size_for(index);
    if bytes.len() != expected {
        debug_assert!(
            false,
            "decode_param_frame: length mismatch expected={expected} actual={}",
            bytes.len()
        );
        return Err(DecodeError::ParamFrameLenMismatch {
            expected,
            actual: bytes.len(),
        });
    }
    Ok(ParamView::from_wire_bytes(index, bytes))
}

/// Reconstruct a [`PortView`] from host-supplied wire bytes and a prepared
/// layout. Debug-panics on length mismatch; release returns `Err`.
#[inline]
pub fn decode_port_frame<'a>(
    bytes: &'a [u8],
    layout: &'a PortLayout,
) -> Result<PortView<'a>, DecodeError> {
    if bytes.len() != layout.total_size {
        debug_assert!(
            false,
            "decode_port_frame: length mismatch expected={} actual={}",
            layout.total_size,
            bytes.len()
        );
        return Err(DecodeError::PortFrameLenMismatch {
            expected: layout.total_size,
            actual: bytes.len(),
        });
    }
    Ok(PortView::new(layout, bytes))
}

/// Plugin-instance wrapper: holds the user's `Module` plus the prepared
/// [`ParamViewIndex`] and [`PortLayout`] used to decode audio-thread frames.
///
/// Public so `export_plugin!` can name it from outside this crate.
pub struct PluginInstance<M: patches_core::Module> {
    pub module: M,
    pub param_index: ParamViewIndex,
    pub port_layout: PortLayout,
    pub input_buf: Vec<patches_core::InputPort>,
    pub output_buf: Vec<patches_core::OutputPort>,
}

/// Emit the eight ABI symbols the new `FfiPluginVTable` expects plus
/// `patches_plugin_init` and `patches_plugin_descriptor_hash_<name>`.
///
/// ```ignore
/// patches_ffi_common::export_plugin!(MyModule, describe_fn, "my_module");
/// ```
///
/// `$module`: a [`patches_core::Module`]-implementing type.
/// `$descriptor_fn`: `fn(&ModuleShape) -> ModuleDescriptor` — identical to
/// the module's own `Module::describe`, surfaced as a free function so the
/// host's per-module descriptor-hash symbol can call it at load time.
/// `$name`: bare module name (string literal) used to build the
/// `patches_plugin_descriptor_hash_<name>` symbol read by the host loader.
#[macro_export]
macro_rules! export_plugin {
    ($module:ty, $descriptor_fn:path, $name:literal) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_describe(
            shape: $crate::types::FfiModuleShape,
        ) -> $crate::types::FfiBytes {
            let core_shape: ::patches_core::ModuleShape = shape.into();
            let desc = $descriptor_fn(&core_shape);
            $crate::types::FfiBytes::from_vec(
                $crate::json::serialize_module_descriptor(&desc),
            )
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_prepare(
            descriptor_json: *const u8,
            descriptor_json_len: usize,
            env: $crate::types::FfiAudioEnvironment,
            instance_id: u64,
        ) -> *mut ::std::ffi::c_void {
            // SAFETY: host passes a valid descriptor-JSON byte slice for
            // the duration of the call (ADR 0045 §4).
            let slice = unsafe {
                ::std::slice::from_raw_parts(descriptor_json, descriptor_json_len)
            };
            let descriptor = match $crate::json::deserialize_module_descriptor(slice)
            {
                Ok(d) => d,
                Err(_) => return ::std::ptr::null_mut(),
            };
            let layout = $crate::param_layout::compute_layout(&descriptor);
            let param_index =
                $crate::param_frame::ParamViewIndex::from_layout(&layout);
            let port_layout = $crate::port_frame::PortLayout::new(
                descriptor.inputs.len() as u32,
                descriptor.outputs.len() as u32,
            );
            let input_buf = ::std::vec::Vec::with_capacity(descriptor.inputs.len());
            let output_buf =
                ::std::vec::Vec::with_capacity(descriptor.outputs.len());
            let audio_env: ::patches_core::AudioEnvironment = env.into();
            let id = ::patches_core::modules::InstanceId::from_raw(instance_id);
            let module = <$module as ::patches_core::Module>::prepare(
                &audio_env,
                descriptor,
                id,
            );
            let instance = ::std::boxed::Box::new($crate::sdk::PluginInstance::<
                $module,
            > {
                module,
                param_index,
                port_layout,
                input_buf,
                output_buf,
            });
            ::std::boxed::Box::into_raw(instance) as *mut ::std::ffi::c_void
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_update_validated_parameters(
            handle: $crate::abi::Handle,
            bytes: *const u8,
            len: usize,
            _env: *const $crate::abi::HostEnv,
        ) {
            // SAFETY: handle is a live `Box<PluginInstance<M>>` raw pointer.
            let inst = unsafe {
                &mut *(handle as *mut $crate::sdk::PluginInstance<$module>)
            };
            // SAFETY: host promises `bytes/len` is a valid slice for the
            // duration of the call.
            let slice = unsafe { ::std::slice::from_raw_parts(bytes, len) };
            if let ::std::result::Result::Ok(view) =
                $crate::sdk::decode_param_frame(slice, &inst.param_index)
            {
                ::patches_core::Module::update_validated_parameters(
                    &mut inst.module,
                    &view,
                );
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_set_ports(
            handle: $crate::abi::Handle,
            bytes: *const u8,
            len: usize,
            _env: *const $crate::abi::HostEnv,
        ) {
            // SAFETY: handle is a live `Box<PluginInstance<M>>` raw pointer.
            let inst = unsafe {
                &mut *(handle as *mut $crate::sdk::PluginInstance<$module>)
            };
            // SAFETY: valid slice for the call.
            let slice = unsafe { ::std::slice::from_raw_parts(bytes, len) };
            let view =
                match $crate::sdk::decode_port_frame(slice, &inst.port_layout) {
                    ::std::result::Result::Ok(v) => v,
                    ::std::result::Result::Err(_) => return,
                };
            inst.input_buf.clear();
            inst.output_buf.clear();
            for i in 0..view.input_count() {
                inst.input_buf.push(view.input(i).into());
            }
            for i in 0..view.output_count() {
                inst.output_buf.push(view.output(i).into());
            }
            ::patches_core::Module::set_ports(
                &mut inst.module,
                &inst.input_buf,
                &inst.output_buf,
            );
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_process(
            handle: *mut ::std::ffi::c_void,
            pool_ptr: *mut [::patches_core::cables::CableValue; 2],
            pool_len: usize,
            write_index: usize,
        ) {
            // SAFETY: handle is a live `Box<PluginInstance<M>>` raw pointer;
            // host supplies a valid cable-pool slice exclusively borrowed for
            // this call.
            let inst = unsafe {
                &mut *(handle as *mut $crate::sdk::PluginInstance<$module>)
            };
            let slice = unsafe {
                ::std::slice::from_raw_parts_mut(pool_ptr, pool_len)
            };
            let mut pool =
                ::patches_core::cable_pool::CablePool::new(slice, write_index);
            ::patches_core::Module::process(&mut inst.module, &mut pool);
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_periodic_update(
            handle: *mut ::std::ffi::c_void,
            pool_ptr: *const [::patches_core::cables::CableValue; 2],
            pool_len: usize,
            write_index: usize,
        ) -> i32 {
            // SAFETY: see __patches_process. Read-only access only; we cast
            // via mut-slice and immediately re-borrow immutably through
            // `&pool`.
            let inst = unsafe {
                &mut *(handle as *mut $crate::sdk::PluginInstance<$module>)
            };
            let slice = unsafe {
                ::std::slice::from_raw_parts_mut(
                    pool_ptr as *mut [::patches_core::cables::CableValue; 2],
                    pool_len,
                )
            };
            let pool =
                ::patches_core::cable_pool::CablePool::new(slice, write_index);
            match ::patches_core::Module::as_periodic(&mut inst.module) {
                ::std::option::Option::Some(p) => {
                    p.periodic_update(&pool);
                    1
                }
                ::std::option::Option::None => 0,
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_drop(handle: *mut ::std::ffi::c_void) {
            if handle.is_null() {
                return;
            }
            // SAFETY: handle came from `Box::into_raw` in __patches_prepare.
            let _ = unsafe {
                ::std::boxed::Box::from_raw(
                    handle as *mut $crate::sdk::PluginInstance<$module>,
                )
            };
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_free_bytes(bytes: $crate::types::FfiBytes) {
            // SAFETY: bytes came from `FfiBytes::from_vec` on this side.
            let _ = unsafe { bytes.reclaim() };
        }

        const __PATCHES_VTABLE: $crate::types::FfiPluginVTable =
            $crate::types::FfiPluginVTable {
                abi_version: $crate::types::ABI_VERSION,
                module_version: 0,
                supports_periodic: 0,
                describe: __patches_describe,
                prepare: __patches_prepare,
                update_validated_parameters: __patches_update_validated_parameters,
                process: __patches_process,
                set_ports: __patches_set_ports,
                periodic_update: __patches_periodic_update,
                drop: __patches_drop,
                free_bytes: __patches_free_bytes,
            };

        static __PATCHES_VTABLES: [$crate::types::FfiPluginVTable; 1] =
            [__PATCHES_VTABLE];

        #[unsafe(no_mangle)]
        pub extern "C" fn patches_plugin_init() -> $crate::types::FfiPluginManifest {
            $crate::types::FfiPluginManifest {
                abi_version: $crate::types::ABI_VERSION,
                count: 1,
                vtables: __PATCHES_VTABLES.as_ptr(),
            }
        }

        #[unsafe(export_name = concat!("patches_plugin_descriptor_hash_", $name))]
        pub extern "C" fn __patches_plugin_descriptor_hash() -> u64 {
            let shape = ::patches_core::ModuleShape::default();
            let desc = $descriptor_fn(&shape);
            $crate::descriptor_hash(&desc)
        }
    };
}

/// Same as [`export_plugin!`] but the exported descriptor-hash symbol
/// returns a caller-supplied value instead of the computed hash. For
/// test fixtures that intentionally drift from the host's expectation
/// (E107 ticket 0622).
#[macro_export]
macro_rules! export_plugin_with_hash_override {
    ($module:ty, $descriptor_fn:path, $name:literal, $hash:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_describe(
            shape: $crate::types::FfiModuleShape,
        ) -> $crate::types::FfiBytes {
            let core_shape: ::patches_core::ModuleShape = shape.into();
            let desc = $descriptor_fn(&core_shape);
            $crate::types::FfiBytes::from_vec(
                $crate::json::serialize_module_descriptor(&desc),
            )
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_prepare(
            _descriptor_json: *const u8,
            _descriptor_json_len: usize,
            _env: $crate::types::FfiAudioEnvironment,
            _instance_id: u64,
        ) -> *mut ::std::ffi::c_void {
            ::std::ptr::null_mut()
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_update_validated_parameters(
            _h: $crate::abi::Handle,
            _b: *const u8,
            _l: usize,
            _e: *const $crate::abi::HostEnv,
        ) {
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_set_ports(
            _h: $crate::abi::Handle,
            _b: *const u8,
            _l: usize,
            _e: *const $crate::abi::HostEnv,
        ) {
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_process(
            _h: *mut ::std::ffi::c_void,
            _p: *mut [::patches_core::cables::CableValue; 2],
            _l: usize,
            _w: usize,
        ) {
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_periodic_update(
            _h: *mut ::std::ffi::c_void,
            _p: *const [::patches_core::cables::CableValue; 2],
            _l: usize,
            _w: usize,
        ) -> i32 {
            0
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_drop(_h: *mut ::std::ffi::c_void) {}

        #[unsafe(no_mangle)]
        pub extern "C" fn __patches_free_bytes(bytes: $crate::types::FfiBytes) {
            let _ = unsafe { bytes.reclaim() };
        }

        const __PATCHES_VTABLE: $crate::types::FfiPluginVTable =
            $crate::types::FfiPluginVTable {
                abi_version: $crate::types::ABI_VERSION,
                module_version: 0,
                supports_periodic: 0,
                describe: __patches_describe,
                prepare: __patches_prepare,
                update_validated_parameters: __patches_update_validated_parameters,
                process: __patches_process,
                set_ports: __patches_set_ports,
                periodic_update: __patches_periodic_update,
                drop: __patches_drop,
                free_bytes: __patches_free_bytes,
            };

        static __PATCHES_VTABLES: [$crate::types::FfiPluginVTable; 1] =
            [__PATCHES_VTABLE];

        #[unsafe(no_mangle)]
        pub extern "C" fn patches_plugin_init() -> $crate::types::FfiPluginManifest {
            $crate::types::FfiPluginManifest {
                abi_version: $crate::types::ABI_VERSION,
                count: 1,
                vtables: __PATCHES_VTABLES.as_ptr(),
            }
        }

        #[unsafe(export_name = concat!("patches_plugin_descriptor_hash_", $name))]
        pub extern "C" fn __patches_plugin_descriptor_hash() -> u64 {
            // Silence unused warning in plugins that never need $module.
            let _ = ::std::marker::PhantomData::<$module>;
            $hash
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::cables::CableValue;
    use patches_core::modules::{InstanceId, ModuleDescriptor, ModuleShape, ParameterMap};
    use patches_core::param_frame::{pack_into, ParamFrame};
    use patches_core::param_layout::{compute_layout, defaults_from_descriptor};
    use patches_core::{
        AudioEnvironment, InputPort, Module, MonoInput, MonoOutput, OutputPort,
    };

    use crate::abi::{Handle, HostEnv};
    use crate::port_frame::{pack_ports_into, PortFrame};

    fn test_descriptor() -> ModuleDescriptor {
        ModuleDescriptor::new("decode_smoke", ModuleShape::default())
            .float_param("gain", 0.0, 1.0, 0.5)
            .int_param("mode", -10, 10, 2)
            .bool_param("bypass", false)
            .file_param("sample", &["wav"])
            .mono_in("in")
            .mono_out("out")
    }

    // ── Decoder round-trips ─────────────────────────────────────────────

    #[test]
    fn decode_param_frame_round_trip() {
        let desc = test_descriptor();
        let layout = compute_layout(&desc);
        let defaults = defaults_from_descriptor(&desc);
        let index = ParamViewIndex::from_layout(&layout);

        let mut params = ParameterMap::new();
        params.insert_param(
            "gain",
            0,
            patches_core::modules::ParameterValue::Float(0.75),
        );
        params.insert_param(
            "mode",
            0,
            patches_core::modules::ParameterValue::Int(7),
        );
        params.insert_param(
            "bypass",
            0,
            patches_core::modules::ParameterValue::Bool(true),
        );

        let mut frame = ParamFrame::with_layout(&layout);
        pack_into(&layout, &defaults, &params, &mut frame).unwrap();
        frame.buffer_slots_mut()[0] = 0xDEAD_BEEF;

        let bytes = frame.storage_bytes();
        let view = decode_param_frame(bytes, &index).unwrap();
        assert_eq!(view.fetch_float_static("gain", 0), 0.75);
        assert_eq!(view.fetch_int_static("mode", 0), 7);
        assert!(view.fetch_bool_static("bypass", 0));
        let bid = view.fetch_buffer_static("sample", 0).unwrap();
        assert_eq!(bid.as_u64(), 0xDEAD_BEEF);
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn decode_param_frame_rejects_short_release() {
        let desc = test_descriptor();
        let layout = compute_layout(&desc);
        let index = ParamViewIndex::from_layout(&layout);
        let err = decode_param_frame(&[0u8; 1], &index).unwrap_err();
        assert!(matches!(err, DecodeError::ParamFrameLenMismatch { .. }));
    }

    #[test]
    fn decode_port_frame_round_trip() {
        let layout = PortLayout::new(1, 1);
        let mut frame = PortFrame::with_layout(layout);
        let inputs = vec![InputPort::Mono(MonoInput {
            cable_idx: 3,
            scale: 0.5,
            connected: true,
        })];
        let outputs = vec![OutputPort::Mono(MonoOutput {
            cable_idx: 7,
            connected: true,
        })];
        pack_ports_into(5, &inputs, &outputs, &mut frame).unwrap();
        let view = decode_port_frame(frame.bytes(), &layout).unwrap();
        assert_eq!(view.header().idx, 5);
        assert_eq!(view.input(0).cable_idx, 3);
        assert_eq!(view.output(0).cable_idx, 7);
    }

    // ── export_plugin! smoke test ───────────────────────────────────────

    struct SmokeModule {
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    }

    thread_local! {
        static DROP_COUNT: ::std::cell::Cell<u32> = const { ::std::cell::Cell::new(0) };
        static LAST: ::std::cell::RefCell<Option<(f32, i64, bool, u64)>> =
            const { ::std::cell::RefCell::new(None) };
        static LAST_PORTS: ::std::cell::RefCell<Option<(MonoInput, MonoOutput)>> =
            const { ::std::cell::RefCell::new(None) };
    }

    impl Drop for SmokeModule {
        fn drop(&mut self) {
            DROP_COUNT.with(|c| c.set(c.get() + 1));
        }
    }

    fn smoke_descriptor(_shape: &ModuleShape) -> ModuleDescriptor {
        test_descriptor()
    }

    impl Module for SmokeModule {
        fn describe(shape: &ModuleShape) -> ModuleDescriptor {
            smoke_descriptor(shape)
        }
        fn prepare(
            _env: &AudioEnvironment,
            descriptor: ModuleDescriptor,
            instance_id: InstanceId,
        ) -> Self {
            Self { descriptor, instance_id }
        }
        fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
            let g = p.fetch_float_static("gain", 0);
            let m = p.fetch_int_static("mode", 0);
            let b = p.fetch_bool_static("bypass", 0);
            let s = p
                .fetch_buffer_static("sample", 0)
                .map(|x| x.as_u64())
                .unwrap_or(0);
            LAST.with(|l| *l.borrow_mut() = Some((g, m, b, s)));
        }
        fn descriptor(&self) -> &ModuleDescriptor {
            &self.descriptor
        }
        fn instance_id(&self) -> InstanceId {
            self.instance_id
        }
        fn process(&mut self, _pool: &mut patches_core::cable_pool::CablePool<'_>) {}
        fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
            if let (Some(InputPort::Mono(i)), Some(OutputPort::Mono(o))) =
                (inputs.first(), outputs.first())
            {
                LAST_PORTS.with(|l| *l.borrow_mut() = Some((*i, *o)));
            }
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    // One invocation per crate — `#[no_mangle]` fns would collide otherwise.
    crate::export_plugin!(SmokeModule, smoke_descriptor, "decode_smoke");

    extern "C" fn noop_release(_: u64) {}

    fn host_env() -> HostEnv {
        HostEnv {
            float_buffer_release: noop_release,
            song_data_release: noop_release,
        }
    }

    #[test]
    fn macro_smoke_round_trip() {
        use crate::types::{FfiAudioEnvironment, FfiModuleShape, FfiPluginManifest};

        // describe → ModuleDescriptor
        let shape = FfiModuleShape::from(&ModuleShape::default());
        let bytes = __patches_describe(shape);
        let desc = crate::json::deserialize_module_descriptor(unsafe {
            bytes.as_slice()
        })
        .unwrap();
        assert_eq!(desc.module_name, "decode_smoke");
        __patches_free_bytes(bytes);

        // prepare
        let env = AudioEnvironment {
            sample_rate: 48000.0,
            poly_voices: 1,
            periodic_update_interval: 64,
            hosted: false,
        };
        let ffi_env = FfiAudioEnvironment::from(&env);
        let json_bytes = crate::json::serialize_module_descriptor(&desc);
        let handle = __patches_prepare(
            json_bytes.as_ptr(),
            json_bytes.len(),
            ffi_env,
            99,
        );
        assert!(!handle.is_null());

        // Pack a parameter frame and dispatch.
        let layout = compute_layout(&desc);
        let defaults = defaults_from_descriptor(&desc);
        let mut params = ParameterMap::new();
        params.insert_param(
            "gain",
            0,
            patches_core::modules::ParameterValue::Float(0.125),
        );
        params.insert_param(
            "mode",
            0,
            patches_core::modules::ParameterValue::Int(-4),
        );
        params.insert_param(
            "bypass",
            0,
            patches_core::modules::ParameterValue::Bool(true),
        );
        let mut frame = ParamFrame::with_layout(&layout);
        pack_into(&layout, &defaults, &params, &mut frame).unwrap();
        frame.buffer_slots_mut()[0] = 0xCAFE_F00D;
        let b = frame.storage_bytes();
        let env_v = host_env();
        __patches_update_validated_parameters(
            handle as Handle,
            b.as_ptr(),
            b.len(),
            &env_v,
        );

        LAST.with(|l| {
            let v = l.borrow().unwrap();
            assert_eq!(v.0, 0.125);
            assert_eq!(v.1, -4);
            assert!(v.2);
            assert_eq!(v.3, 0xCAFE_F00D);
        });

        // set_ports
        let port_layout = PortLayout::new(1, 1);
        let mut port_frame = PortFrame::with_layout(port_layout);
        let inputs = vec![InputPort::Mono(MonoInput {
            cable_idx: 11,
            scale: 1.0,
            connected: true,
        })];
        let outputs = vec![OutputPort::Mono(MonoOutput {
            cable_idx: 22,
            connected: true,
        })];
        pack_ports_into(0, &inputs, &outputs, &mut port_frame).unwrap();
        let pb = port_frame.bytes();
        __patches_set_ports(handle as Handle, pb.as_ptr(), pb.len(), &env_v);
        LAST_PORTS.with(|l| {
            let (i, o) = l.borrow().unwrap();
            assert_eq!(i.cable_idx, 11);
            assert_eq!(o.cable_idx, 22);
        });

        // process: trivial no-op, prove the entry point doesn't panic.
        let mut pool_mem: Vec<[CableValue; 2]> =
            vec![[CableValue::Mono(0.0); 2]; 4];
        __patches_process(handle, pool_mem.as_mut_ptr(), pool_mem.len(), 0);

        // destroy
        let before = DROP_COUNT.with(|c| c.get());
        __patches_drop(handle);
        let after = DROP_COUNT.with(|c| c.get());
        assert_eq!(after, before + 1, "SmokeModule Drop must fire");

        // manifest sanity
        let m: FfiPluginManifest = patches_plugin_init();
        assert_eq!(m.count, 1);
        let slice = unsafe { std::slice::from_raw_parts(m.vtables, m.count) };
        assert_eq!(slice[0].abi_version, crate::types::ABI_VERSION);

        // descriptor-hash symbol matches host computation.
        let hash = __patches_plugin_descriptor_hash();
        assert_eq!(hash, crate::descriptor_hash(&desc));
    }
}
