use super::mono::{MonoInput, MonoOutput};
use super::poly::{PolyInput, PolyOutput};
use super::stereo::{StereoInput, StereoOutput};
use crate::invariant::ExpectInvariant;

/// Heterogeneous input-port wrapper used by the planner to deliver ports to
/// `Module::set_ports` without boxing.
///
/// Only arity (`Mono` vs. `Poly`) is encoded here; trigger / MIDI / transport
/// semantics are carried by the port's layout tag in the `ModuleDescriptor`
/// and enforced at graph-connection time (ADR 0047 / ADR 0033).
#[derive(Clone, Debug, PartialEq)]
pub enum InputPort {
    Mono(MonoInput),
    Poly(PolyInput),
    Stereo(StereoInput),
}

impl InputPort {
    pub fn as_mono(&self) -> Option<MonoInput> {
        match self {
            InputPort::Mono(p) => Some(*p),
            _ => None,
        }
    }

    /// Unwrap as [`MonoInput`].
    ///
    /// # Panics
    ///
    /// Panics if the variant is not `Mono`. The planner's connection-time
    /// type checking (ADR 0047 / ADR 0033) is the structural guarantee:
    /// once a `ModuleGraph` has been built, every `InputPort` delivered to
    /// `Module::set_ports` matches the descriptor's declared kind, so this
    /// is infallible at audio-thread call sites.
    pub fn expect_mono(&self) -> MonoInput {
        self.as_mono().expect_invariant("port kind guaranteed by planner connection-time type check (ADR 0047/0033)")
    }

    pub fn as_poly(&self) -> Option<PolyInput> {
        match self {
            InputPort::Poly(p) => Some(*p),
            _ => None,
        }
    }

    /// Unwrap as [`PolyInput`].
    ///
    /// # Panics
    ///
    /// Panics if the variant is not `Poly`. See [`expect_mono`](Self::expect_mono)
    /// for the planner-time type-check guarantee that makes this infallible
    /// in module `set_ports` implementations.
    pub fn expect_poly(&self) -> PolyInput {
        self.as_poly().expect_invariant("port kind guaranteed by planner connection-time type check (ADR 0047/0033)")
    }

    pub fn as_stereo(&self) -> Option<StereoInput> {
        match self {
            InputPort::Stereo(p) => Some(*p),
            _ => None,
        }
    }

    /// Unwrap as [`StereoInput`].
    ///
    /// # Panics
    ///
    /// Panics if the variant is not `Stereo`. See [`expect_mono`](Self::expect_mono)
    /// for the planner-time type-check guarantee that makes this infallible
    /// in module `set_ports` implementations.
    pub fn expect_stereo(&self) -> StereoInput {
        self.as_stereo().expect_invariant("port kind guaranteed by planner connection-time type check (ADR 0047/0033)")
    }

    /// Alias for [`expect_mono`](Self::expect_mono): trigger ports are mono
    /// cables with a `MonoLayout::Trigger` layout tag. Retained for
    /// module-side readability.
    ///
    /// # Panics
    ///
    /// Panics if the variant is not `Mono`. See [`expect_mono`](Self::expect_mono).
    pub fn expect_trigger(&self) -> MonoInput {
        self.expect_mono()
    }

    /// Alias for [`expect_poly`](Self::expect_poly): poly-trigger ports are
    /// poly cables with a `PolyLayout::Trigger` layout tag.
    ///
    /// # Panics
    ///
    /// Panics if the variant is not `Poly`. See [`expect_mono`](Self::expect_mono).
    pub fn expect_poly_trigger(&self) -> PolyInput {
        self.expect_poly()
    }
}

/// Heterogeneous output-port wrapper used by the planner to deliver ports to
/// `Module::set_ports` without boxing.
#[derive(Clone, Debug, PartialEq)]
pub enum OutputPort {
    Mono(MonoOutput),
    Poly(PolyOutput),
    Stereo(StereoOutput),
}

impl OutputPort {
    pub fn as_mono(&self) -> Option<MonoOutput> {
        match self {
            OutputPort::Mono(p) => Some(*p),
            _ => None,
        }
    }

    /// Unwrap as [`MonoOutput`].
    ///
    /// # Panics
    ///
    /// Panics if the variant is not `Mono`. The planner's connection-time
    /// type checking (ADR 0047 / ADR 0033) makes this infallible in module
    /// `set_ports` implementations.
    pub fn expect_mono(&self) -> MonoOutput {
        self.as_mono().expect_invariant("port kind guaranteed by planner connection-time type check (ADR 0047/0033)")
    }

    pub fn as_poly(&self) -> Option<PolyOutput> {
        match self {
            OutputPort::Poly(p) => Some(*p),
            _ => None,
        }
    }

    /// Unwrap as [`PolyOutput`].
    ///
    /// # Panics
    ///
    /// Panics if the variant is not `Poly`. See [`expect_mono`](Self::expect_mono).
    pub fn expect_poly(&self) -> PolyOutput {
        self.as_poly().expect_invariant("port kind guaranteed by planner connection-time type check (ADR 0047/0033)")
    }

    pub fn as_stereo(&self) -> Option<StereoOutput> {
        match self {
            OutputPort::Stereo(p) => Some(*p),
            _ => None,
        }
    }

    /// Unwrap as [`StereoOutput`].
    ///
    /// # Panics
    ///
    /// Panics if the variant is not `Stereo`. See [`expect_mono`](Self::expect_mono).
    pub fn expect_stereo(&self) -> StereoOutput {
        self.as_stereo().expect_invariant("port kind guaranteed by planner connection-time type check (ADR 0047/0033)")
    }

    /// Alias for [`expect_mono`](Self::expect_mono): trigger ports are mono
    /// cables with a `MonoLayout::Trigger` layout tag.
    ///
    /// # Panics
    ///
    /// Panics if the variant is not `Mono`. See [`expect_mono`](Self::expect_mono).
    pub fn expect_trigger(&self) -> MonoOutput {
        self.expect_mono()
    }

    /// Alias for [`expect_poly`](Self::expect_poly): poly-trigger ports are
    /// poly cables with a `PolyLayout::Trigger` layout tag.
    ///
    /// # Panics
    ///
    /// Panics if the variant is not `Poly`. See [`expect_mono`](Self::expect_mono).
    pub fn expect_poly_trigger(&self) -> PolyOutput {
        self.expect_poly()
    }
}
