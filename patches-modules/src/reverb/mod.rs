//! Reverb module group (ADR 0076).
//!
//! Currently houses the FDN-8 reverb; future reverb variants share
//! this directory.

pub mod fdn_reverb;

pub use fdn_reverb::FdnReverb;
