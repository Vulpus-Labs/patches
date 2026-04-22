use super::mono::{MonoInput, MonoOutput};
use super::poly::{PolyInput, PolyOutput};

/// Heterogeneous input-port wrapper used by the planner to deliver ports to
/// `Module::set_ports` without boxing.
///
/// `Trigger` / `PolyTrigger` variants reuse the `MonoInput` / `PolyInput`
/// structs because buffer layout is identical; the variant tag alone carries
/// the cable-kind distinction (ADR 0047).
#[derive(Clone, Debug, PartialEq)]
pub enum InputPort {
    Mono(MonoInput),
    Poly(PolyInput),
    Trigger(MonoInput),
    PolyTrigger(PolyInput),
}

impl InputPort {
    pub fn as_mono(&self) -> Option<MonoInput> {
        match self {
            InputPort::Mono(p) => Some(*p),
            InputPort::Poly(_) | InputPort::Trigger(_) | InputPort::PolyTrigger(_) => None,
        }
    }

    pub fn expect_mono(&self) -> MonoInput {
        self.as_mono().expect("expected mono input port")
    }

    pub fn as_poly(&self) -> Option<PolyInput> {
        match self {
            InputPort::Poly(p) => Some(*p),
            InputPort::Mono(_) | InputPort::Trigger(_) | InputPort::PolyTrigger(_) => None,
        }
    }

    pub fn expect_poly(&self) -> PolyInput {
        self.as_poly().expect("expected poly input port")
    }

    pub fn as_trigger(&self) -> Option<MonoInput> {
        match self {
            InputPort::Trigger(p) => Some(*p),
            _ => None,
        }
    }

    pub fn expect_trigger(&self) -> MonoInput {
        self.as_trigger().expect("expected trigger input port")
    }

    pub fn as_poly_trigger(&self) -> Option<PolyInput> {
        match self {
            InputPort::PolyTrigger(p) => Some(*p),
            _ => None,
        }
    }

    pub fn expect_poly_trigger(&self) -> PolyInput {
        self.as_poly_trigger().expect("expected poly-trigger input port")
    }
}

/// Heterogeneous output-port wrapper used by the planner to deliver ports to
/// `Module::set_ports` without boxing.
#[derive(Clone, Debug, PartialEq)]
pub enum OutputPort {
    Mono(MonoOutput),
    Poly(PolyOutput),
    Trigger(MonoOutput),
    PolyTrigger(PolyOutput),
}

impl OutputPort {
    pub fn as_mono(&self) -> Option<MonoOutput> {
        match self {
            OutputPort::Mono(p) => Some(*p),
            OutputPort::Poly(_) | OutputPort::Trigger(_) | OutputPort::PolyTrigger(_) => None,
        }
    }

    pub fn expect_mono(&self) -> MonoOutput {
        self.as_mono().expect("expected mono output port")
    }

    pub fn as_poly(&self) -> Option<PolyOutput> {
        match self {
            OutputPort::Poly(p) => Some(*p),
            OutputPort::Mono(_) | OutputPort::Trigger(_) | OutputPort::PolyTrigger(_) => None,
        }
    }

    pub fn expect_poly(&self) -> PolyOutput {
        self.as_poly().expect("expected poly output port")
    }

    pub fn as_trigger(&self) -> Option<MonoOutput> {
        match self {
            OutputPort::Trigger(p) => Some(*p),
            _ => None,
        }
    }

    pub fn expect_trigger(&self) -> MonoOutput {
        self.as_trigger().expect("expected trigger output port")
    }

    pub fn as_poly_trigger(&self) -> Option<PolyOutput> {
        match self {
            OutputPort::PolyTrigger(p) => Some(*p),
            _ => None,
        }
    }

    pub fn expect_poly_trigger(&self) -> PolyOutput {
        self.as_poly_trigger().expect("expected poly-trigger output port")
    }
}
