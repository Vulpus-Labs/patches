//! Modulator module group (ADR 0076).
//!
//! Envelopes, LFOs, sample-and-hold, glide, FM operators, quantisers,
//! tuners, and ring modulation. Mono / poly variants live as sibling
//! files; deeper unification (one module parameterised over axis) is
//! out of scope.

pub mod adsr;
pub mod glide;
pub mod lfo;
pub mod op;
pub mod poly_adsr;
pub mod poly_lfo;
pub mod poly_op;
pub mod poly_quant;
pub mod poly_sah;
pub mod poly_tuner;
pub mod quant;
pub mod ring_mod;
pub mod sah;
pub mod tuner;

pub use adsr::Adsr;
pub use glide::Glide;
pub use lfo::Lfo;
pub use op::Op;
pub use poly_adsr::PolyAdsr;
pub use poly_lfo::PolyLfo;
pub use poly_op::PolyOp;
pub use poly_quant::PolyQuant;
pub use poly_sah::PolySah;
pub use poly_tuner::PolyTuner;
pub use quant::Quant;
pub use ring_mod::RingMod;
pub use sah::Sah;
pub use tuner::Tuner;
