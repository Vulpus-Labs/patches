//! Drums bundle: a single FFI plugin dylib exposing the eight drum module
//! types from `patches-modules` through one manifest. See ADR 0039 / E088.

use patches_modules::{
    ClapDrum, ClosedHiHat, Claves, Cymbal, Kick, OpenHiHat, Snare, Tom,
};

patches_ffi::export_modules!(
    Kick,
    Snare,
    ClapDrum,
    ClosedHiHat,
    OpenHiHat,
    Tom,
    Claves,
    Cymbal,
);
