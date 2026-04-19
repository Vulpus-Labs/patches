use std::ffi::c_void;

use patches_core::cables::CableValue;

// ── ABI version ──────────────────────────────────────────────────────────────

/// Increment this when the vtable layout or any repr(C) type changes.
pub const ABI_VERSION: u32 = 3;

// ── FfiBytes ─────────────────────────────────────────────────────────────────

/// An owned byte buffer allocated by the plugin side.
///
/// The host reads `ptr` / `len`, then calls `vtable.free_bytes` to let the
/// plugin deallocate. This avoids cross-allocator double-free issues.
///
/// A null `ptr` with `len == 0` represents an empty / absent buffer.
#[repr(C)]
pub struct FfiBytes {
    pub ptr: *mut u8,
    pub len: usize,
}

impl FfiBytes {
    /// Create an `FfiBytes` that owns the contents of `v`.
    ///
    /// The caller is responsible for eventually freeing this via
    /// [`FfiBytes::reclaim`] or the vtable's `free_bytes`.
    pub fn from_vec(v: Vec<u8>) -> Self {
        let mut v = std::mem::ManuallyDrop::new(v);
        Self { ptr: v.as_mut_ptr(), len: v.len() }
    }

    /// An empty (null) buffer.
    pub fn empty() -> Self {
        Self { ptr: std::ptr::null_mut(), len: 0 }
    }

    /// Read the contents as a byte slice without taking ownership.
    ///
    /// # Safety
    /// The pointer must be valid and the buffer must not have been freed.
    pub unsafe fn as_slice(&self) -> &[u8] {
        if self.ptr.is_null() {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }
    }

    /// Reclaim the buffer as a `Vec<u8>` so it can be dropped normally.
    ///
    /// # Safety
    /// Must only be called once, and only by the same allocator that produced
    /// the buffer (i.e. on the plugin side via `free_bytes`).
    pub unsafe fn reclaim(self) -> Vec<u8> {
        if self.ptr.is_null() {
            Vec::new()
        } else {
            unsafe { Vec::from_raw_parts(self.ptr, self.len, self.len) }
        }
    }
}

// ── FfiAudioEnvironment ──────────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FfiAudioEnvironment {
    pub sample_rate: f32,
    pub poly_voices: usize,
    pub periodic_update_interval: u32,
}

impl From<&patches_core::AudioEnvironment> for FfiAudioEnvironment {
    fn from(env: &patches_core::AudioEnvironment) -> Self {
        Self {
            sample_rate: env.sample_rate,
            poly_voices: env.poly_voices,
            periodic_update_interval: env.periodic_update_interval,
        }
    }
}

impl From<FfiAudioEnvironment> for patches_core::AudioEnvironment {
    fn from(ffi: FfiAudioEnvironment) -> Self {
        Self {
            sample_rate: ffi.sample_rate,
            poly_voices: ffi.poly_voices,
            periodic_update_interval: ffi.periodic_update_interval,
            hosted: false,
        }
    }
}

// ── FfiModuleShape ───────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FfiModuleShape {
    pub channels: usize,
    pub length: usize,
    pub high_quality: u8,
}

impl From<&patches_core::ModuleShape> for FfiModuleShape {
    fn from(shape: &patches_core::ModuleShape) -> Self {
        Self {
            channels: shape.channels,
            length: shape.length,
            high_quality: shape.high_quality as u8,
        }
    }
}

impl From<FfiModuleShape> for patches_core::ModuleShape {
    fn from(ffi: FfiModuleShape) -> Self {
        Self {
            channels: ffi.channels,
            length: ffi.length,
            high_quality: ffi.high_quality != 0,
        }
    }
}

// ── FfiInputPort / FfiOutputPort ─────────────────────────────────────────────

/// Tag values for port kind discrimination across the C ABI.
pub const PORT_TAG_MONO: u8 = 0;
pub const PORT_TAG_POLY: u8 = 1;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FfiInputPort {
    pub tag: u8,
    pub cable_idx: usize,
    pub scale: f32,
    pub connected: u8,
}

impl From<&patches_core::InputPort> for FfiInputPort {
    fn from(port: &patches_core::InputPort) -> Self {
        match port {
            patches_core::InputPort::Mono(m) => Self {
                tag: PORT_TAG_MONO,
                cable_idx: m.cable_idx,
                scale: m.scale,
                connected: m.connected as u8,
            },
            patches_core::InputPort::Poly(p) => Self {
                tag: PORT_TAG_POLY,
                cable_idx: p.cable_idx,
                scale: p.scale,
                connected: p.connected as u8,
            },
        }
    }
}

impl From<FfiInputPort> for patches_core::InputPort {
    fn from(ffi: FfiInputPort) -> Self {
        if ffi.tag == PORT_TAG_POLY {
            patches_core::InputPort::Poly(patches_core::PolyInput {
                cable_idx: ffi.cable_idx,
                scale: ffi.scale,
                connected: ffi.connected != 0,
            })
        } else {
            patches_core::InputPort::Mono(patches_core::MonoInput {
                cable_idx: ffi.cable_idx,
                scale: ffi.scale,
                connected: ffi.connected != 0,
            })
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FfiOutputPort {
    pub tag: u8,
    pub cable_idx: usize,
    pub connected: u8,
}

impl From<&patches_core::OutputPort> for FfiOutputPort {
    fn from(port: &patches_core::OutputPort) -> Self {
        match port {
            patches_core::OutputPort::Mono(m) => Self {
                tag: PORT_TAG_MONO,
                cable_idx: m.cable_idx,
                connected: m.connected as u8,
            },
            patches_core::OutputPort::Poly(p) => Self {
                tag: PORT_TAG_POLY,
                cable_idx: p.cable_idx,
                connected: p.connected as u8,
            },
        }
    }
}

impl From<FfiOutputPort> for patches_core::OutputPort {
    fn from(ffi: FfiOutputPort) -> Self {
        if ffi.tag == PORT_TAG_POLY {
            patches_core::OutputPort::Poly(patches_core::PolyOutput {
                cable_idx: ffi.cable_idx,
                connected: ffi.connected != 0,
            })
        } else {
            patches_core::OutputPort::Mono(patches_core::MonoOutput {
                cable_idx: ffi.cable_idx,
                connected: ffi.connected != 0,
            })
        }
    }
}

// ── FfiPluginVTable ──────────────────────────────────────────────────────────

/// The C ABI contract between host and plugin.
///
/// A plugin exports `patches_plugin_init() -> FfiPluginManifest` which the host
/// calls once at load time. The manifest points at an array of vtables; each
/// vtable is one module type.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FfiPluginVTable {
    pub abi_version: u32,
    /// Per-module semver-packed version: `(major<<16)|(minor<<8)|patch`.
    /// Registry prefers higher versions across rescans.
    pub module_version: u32,
    pub supports_periodic: i32,

    pub describe: unsafe extern "C" fn(shape: FfiModuleShape) -> FfiBytes,

    pub prepare: unsafe extern "C" fn(
        descriptor_json: *const u8,
        descriptor_json_len: usize,
        env: FfiAudioEnvironment,
        instance_id: u64,
    ) -> *mut c_void,

    pub update_validated_parameters: unsafe extern "C" fn(
        handle: *mut c_void,
        params_json: *const u8,
        params_json_len: usize,
    ),

    pub update_parameters: unsafe extern "C" fn(
        handle: *mut c_void,
        params_json: *const u8,
        params_json_len: usize,
        error_out: *mut FfiBytes,
    ) -> i32,

    pub process: unsafe extern "C" fn(
        handle: *mut c_void,
        pool_ptr: *mut [CableValue; 2],
        pool_len: usize,
        write_index: usize,
    ),

    pub set_ports: unsafe extern "C" fn(
        handle: *mut c_void,
        inputs: *const FfiInputPort,
        inputs_len: usize,
        outputs: *const FfiOutputPort,
        outputs_len: usize,
    ),

    pub periodic_update: unsafe extern "C" fn(
        handle: *mut c_void,
        pool_ptr: *const [CableValue; 2],
        pool_len: usize,
        write_index: usize,
    ) -> i32,

    pub descriptor: unsafe extern "C" fn(handle: *mut c_void) -> FfiBytes,

    pub instance_id: unsafe extern "C" fn(handle: *mut c_void) -> u64,

    pub drop: unsafe extern "C" fn(handle: *mut c_void),

    pub free_bytes: unsafe extern "C" fn(bytes: FfiBytes),
}

// ── FfiPluginManifest ────────────────────────────────────────────────────────

/// Multi-module plugin bundle manifest. See ADR 0039.
///
/// `vtables` points to a plugin-static array of length `count`; the host reads
/// the entries at load time (cloning each into its own `DylibModuleBuilder`)
/// and does not retain the pointer.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FfiPluginManifest {
    pub abi_version: u32,
    pub count: usize,
    pub vtables: *const FfiPluginVTable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_environment_round_trip() {
        let orig = patches_core::AudioEnvironment {
            sample_rate: 48000.0,
            poly_voices: 8,
            periodic_update_interval: 64,
            hosted: false,
        };
        let ffi: FfiAudioEnvironment = (&orig).into();
        let back: patches_core::AudioEnvironment = ffi.into();
        assert_eq!(back.sample_rate, orig.sample_rate);
        assert_eq!(back.poly_voices, orig.poly_voices);
        assert_eq!(back.periodic_update_interval, orig.periodic_update_interval);
    }

    #[test]
    fn module_shape_round_trip() {
        let orig = patches_core::ModuleShape { channels: 4, length: 16, high_quality: true };
        let ffi: FfiModuleShape = (&orig).into();
        let back: patches_core::ModuleShape = ffi.into();
        assert_eq!(back.channels, 4);
        assert_eq!(back.length, 16);
        assert!(back.high_quality);
    }

    #[test]
    fn input_port_mono_round_trip() {
        let orig = patches_core::InputPort::Mono(patches_core::MonoInput {
            cable_idx: 42,
            scale: 0.75,
            connected: true,
        });
        let ffi: FfiInputPort = (&orig).into();
        let back: patches_core::InputPort = ffi.into();
        let m = back.expect_mono();
        assert_eq!(m.cable_idx, 42);
        assert_eq!(m.scale, 0.75);
        assert!(m.connected);
    }

    #[test]
    fn input_port_poly_round_trip() {
        let orig = patches_core::InputPort::Poly(patches_core::PolyInput {
            cable_idx: 7,
            scale: 0.5,
            connected: false,
        });
        let ffi: FfiInputPort = (&orig).into();
        let back: patches_core::InputPort = ffi.into();
        let p = back.expect_poly();
        assert_eq!(p.cable_idx, 7);
        assert_eq!(p.scale, 0.5);
        assert!(!p.connected);
    }

    #[test]
    fn output_port_mono_round_trip() {
        let orig = patches_core::OutputPort::Mono(patches_core::MonoOutput {
            cable_idx: 3,
            connected: true,
        });
        let ffi: FfiOutputPort = (&orig).into();
        let back: patches_core::OutputPort = ffi.into();
        let m = back.expect_mono();
        assert_eq!(m.cable_idx, 3);
        assert!(m.connected);
    }

    #[test]
    fn output_port_poly_round_trip() {
        let orig = patches_core::OutputPort::Poly(patches_core::PolyOutput {
            cable_idx: 10,
            connected: false,
        });
        let ffi: FfiOutputPort = (&orig).into();
        let back: patches_core::OutputPort = ffi.into();
        let p = back.expect_poly();
        assert_eq!(p.cable_idx, 10);
        assert!(!p.connected);
    }

    #[test]
    fn ffi_bytes_from_vec_and_reclaim() {
        let data = vec![1u8, 2, 3, 4, 5];
        let ffi = FfiBytes::from_vec(data.clone());
        assert_eq!(ffi.len, 5);
        let reclaimed = unsafe { ffi.reclaim() };
        assert_eq!(reclaimed, data);
    }

    #[test]
    fn ffi_bytes_empty() {
        let ffi = FfiBytes::empty();
        assert!(ffi.ptr.is_null());
        assert_eq!(ffi.len, 0);
        let slice = unsafe { ffi.as_slice() };
        assert!(slice.is_empty());
    }

    unsafe extern "C" fn stub_describe(_s: FfiModuleShape) -> FfiBytes { FfiBytes::empty() }
    unsafe extern "C" fn stub_prepare(
        _p: *const u8, _l: usize, _e: FfiAudioEnvironment, _i: u64,
    ) -> *mut c_void { std::ptr::null_mut() }
    unsafe extern "C" fn stub_uvp(_h: *mut c_void, _p: *const u8, _l: usize) {}
    unsafe extern "C" fn stub_up(
        _h: *mut c_void, _p: *const u8, _l: usize, _e: *mut FfiBytes,
    ) -> i32 { 0 }
    unsafe extern "C" fn stub_process(
        _h: *mut c_void, _p: *mut [CableValue; 2], _l: usize, _w: usize,
    ) {}
    unsafe extern "C" fn stub_set_ports(
        _h: *mut c_void, _i: *const FfiInputPort, _il: usize,
        _o: *const FfiOutputPort, _ol: usize,
    ) {}
    unsafe extern "C" fn stub_periodic(
        _h: *mut c_void, _p: *const [CableValue; 2], _l: usize, _w: usize,
    ) -> i32 { 0 }
    unsafe extern "C" fn stub_descriptor(_h: *mut c_void) -> FfiBytes { FfiBytes::empty() }
    unsafe extern "C" fn stub_iid(_h: *mut c_void) -> u64 { 0 }
    unsafe extern "C" fn stub_drop(_h: *mut c_void) {}
    unsafe extern "C" fn stub_free_bytes(_b: FfiBytes) {}

    fn stub_vtable() -> FfiPluginVTable {
        FfiPluginVTable {
            abi_version: ABI_VERSION,
            module_version: 0,
            supports_periodic: 0,
            describe: stub_describe,
            prepare: stub_prepare,
            update_validated_parameters: stub_uvp,
            update_parameters: stub_up,
            process: stub_process,
            set_ports: stub_set_ports,
            periodic_update: stub_periodic,
            descriptor: stub_descriptor,
            instance_id: stub_iid,
            drop: stub_drop,
            free_bytes: stub_free_bytes,
        }
    }

    #[test]
    fn manifest_round_trip_leaked_array() {
        let vtables: Vec<FfiPluginVTable> = vec![stub_vtable(), stub_vtable()];
        let leaked: &'static [FfiPluginVTable] = Vec::leak(vtables);
        let manifest = FfiPluginManifest {
            abi_version: ABI_VERSION,
            count: leaked.len(),
            vtables: leaked.as_ptr(),
        };
        assert_eq!(manifest.abi_version, ABI_VERSION);
        assert_eq!(manifest.count, 2);
        let slice = unsafe { std::slice::from_raw_parts(manifest.vtables, manifest.count) };
        assert_eq!(slice.len(), 2);
        assert_eq!(slice[0].abi_version, ABI_VERSION);
        assert_eq!(slice[1].abi_version, ABI_VERSION);
        assert!(std::ptr::eq(slice.as_ptr(), leaked.as_ptr()));
    }
}
