use super::mono::{MonoInput, MonoOutput};
use super::poly::{PolyInput, PolyOutput};
use super::stereo::{StereoInput, StereoOutput};

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

    pub fn expect_mono(&self) -> MonoInput {
        self.as_mono().expect("expected mono input port")
    }

    pub fn as_poly(&self) -> Option<PolyInput> {
        match self {
            InputPort::Poly(p) => Some(*p),
            _ => None,
        }
    }

    pub fn expect_poly(&self) -> PolyInput {
        self.as_poly().expect("expected poly input port")
    }

    pub fn as_stereo(&self) -> Option<StereoInput> {
        match self {
            InputPort::Stereo(p) => Some(*p),
            _ => None,
        }
    }

    pub fn expect_stereo(&self) -> StereoInput {
        self.as_stereo().expect("expected stereo input port")
    }

    /// Alias for [`expect_mono`](Self::expect_mono): trigger ports are mono
    /// cables with a `MonoLayout::Trigger` layout tag. Retained for
    /// module-side readability.
    pub fn expect_trigger(&self) -> MonoInput {
        self.expect_mono()
    }

    /// Alias for [`expect_poly`](Self::expect_poly): poly-trigger ports are
    /// poly cables with a `PolyLayout::Trigger` layout tag.
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

    pub fn expect_mono(&self) -> MonoOutput {
        self.as_mono().expect("expected mono output port")
    }

    pub fn as_poly(&self) -> Option<PolyOutput> {
        match self {
            OutputPort::Poly(p) => Some(*p),
            _ => None,
        }
    }

    pub fn expect_poly(&self) -> PolyOutput {
        self.as_poly().expect("expected poly output port")
    }

    pub fn as_stereo(&self) -> Option<StereoOutput> {
        match self {
            OutputPort::Stereo(p) => Some(*p),
            _ => None,
        }
    }

    pub fn expect_stereo(&self) -> StereoOutput {
        self.as_stereo().expect("expected stereo output port")
    }

    /// Alias for [`expect_mono`](Self::expect_mono): trigger ports are mono
    /// cables with a `MonoLayout::Trigger` layout tag.
    pub fn expect_trigger(&self) -> MonoOutput {
        self.expect_mono()
    }

    /// Alias for [`expect_poly`](Self::expect_poly): poly-trigger ports are
    /// poly cables with a `PolyLayout::Trigger` layout tag.
    pub fn expect_poly_trigger(&self) -> PolyOutput {
        self.expect_poly()
    }
}
