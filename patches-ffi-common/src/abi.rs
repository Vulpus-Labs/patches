//! New C ABI surface for ADR 0045 Spike 7 (pure definitions — no callers yet).
//!
//! These typedefs supersede the JSON-shaped audio-thread entry points in
//! [`crate::types::FfiPluginVTable`]. Phase B (E104) wires the host loader to
//! these; Phase C (E105) wires plugin-side glue via macro.
//!
//! The audio-thread ABI is three functions plus a per-instance [`HostEnv`]
//! vtable carrying release callbacks, one per payload id type. See ADR 0045 §6.

use std::ffi::c_void;

use patches_core::cables::CableValue;

/// Opaque plugin-instance pointer. A plugin's `prepare` returns one; every
/// audio-thread entry point takes it as its first argument.
pub type Handle = *mut c_void;

/// Packed parameter frame entry: `update_validated_parameters`.
///
/// Delivers a complete snapshot of the module's parameters in the wire
/// format dictated by the module's `ParamLayout`. Pointers are valid only
/// for the duration of the call; scalars must be copied if kept.
pub type UpdateValidatedParametersFn = unsafe extern "C" fn(
    handle: Handle,
    bytes: *const u8,
    len: usize,
    env: *const HostEnv,
);

/// Packed port frame entry: `set_ports`. Same lifetime rules as
/// [`UpdateValidatedParametersFn`]; no allocation or blocking permitted.
pub type SetPortsFn = unsafe extern "C" fn(
    handle: Handle,
    bytes: *const u8,
    len: usize,
    env: *const HostEnv,
);

/// Audio-thread process entry: one tick of sample work.
///
/// `cables` points to the host's cable-value pool; `cable_count` is its
/// length; `write_index` is the current rotating write slot (ADR 0001).
pub type ProcessFn = unsafe extern "C" fn(
    handle: Handle,
    cables: *mut [CableValue; 2],
    cable_count: usize,
    write_index: u32,
);

/// Per-instance host environment vtable supplied alongside every frame-bearing
/// call. Stateless from the plugin's view: a single layout shared by all
/// instances of a given runtime.
///
/// `#[repr(C)]` with fixed field order — see unit test for layout stability
/// guarantees.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostEnv {
    /// Release one `SongDataId`. Kept even though the tracker path does not
    /// cross FFI today (ADR 0045 §6), so the vtable shape is stable as
    /// future payload types land.
    pub song_data_release: extern "C" fn(u64),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    extern "C" fn noop_release(_: u64) {}

    #[test]
    fn host_env_layout_is_stable() {
        assert_eq!(size_of::<HostEnv>(), size_of::<usize>());
        assert_eq!(align_of::<HostEnv>(), align_of::<usize>());
        assert_eq!(offset_of!(HostEnv, song_data_release), 0);
    }

    #[test]
    fn host_env_constructible() {
        let env = HostEnv {
            song_data_release: noop_release,
        };
        (env.song_data_release)(0);
    }

    #[test]
    fn handle_is_pointer_sized() {
        assert_eq!(size_of::<Handle>(), size_of::<*mut c_void>());
    }
}
