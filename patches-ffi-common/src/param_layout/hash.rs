//! Canonical byte encoding + 64-bit digest for a `ModuleDescriptor`.
//!
//! Used at plugin load time (ADR 0045 Spike 7) to detect descriptor drift
//! between host and plugin. Must be byte-identical across runs, machines, and
//! compiler versions: no `Hash` derives, no `HashMap` iteration, no
//! platform-dependent size/alignment leaks.
//!
//! FNV-1a 64-bit is used for the digest. This is not a crypto hash, but the
//! threat model is accidental drift, not adversarial collisions — if that
//! bar ever rises, the algorithm can be swapped without changing the
//! canonical byte encoding feeding it.

use patches_core::modules::module_descriptor::{
    ModuleDescriptor, ParameterDescriptor, ParameterKind, PortDescriptor,
};

use super::{param_kind_tag, port_kind_tag};

const FNV_OFFSET_64: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME_64: u64 = 0x0000_0100_0000_01b3;

struct Fnv1a64 {
    state: u64,
}

impl Fnv1a64 {
    fn new() -> Self {
        Self { state: FNV_OFFSET_64 }
    }

    fn write_u8(&mut self, b: u8) {
        self.state ^= b as u64;
        self.state = self.state.wrapping_mul(FNV_PRIME_64);
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u8(b);
        }
    }

    fn write_u32(&mut self, v: u32) {
        self.write(&v.to_le_bytes());
    }

    fn write_str(&mut self, s: &str) {
        self.write_u32(s.len() as u32);
        self.write(s.as_bytes());
    }

    fn finish(self) -> u64 {
        self.state
    }
}

pub(crate) fn descriptor_hash(d: &ModuleDescriptor) -> u64 {
    let mut h = Fnv1a64::new();

    // Module name first so two modules sharing parameter/port shape still hash
    // distinctly. Not strictly required for drift detection (host/plugin pair
    // a named module with a named module) but cheap insurance.
    h.write_str(d.module_name);

    // Parameters in canonical order.
    let mut params: Vec<&ParameterDescriptor> = d.parameters.iter().collect();
    params.sort_by(|a, b| a.name.cmp(b.name).then_with(|| a.index.cmp(&b.index)));
    h.write_u32(params.len() as u32);
    for p in params {
        h.write_str(p.name);
        h.write_u32(p.index as u32);
        h.write_u8(param_kind_tag(&p.parameter_type));
        encode_kind_payload(&mut h, &p.parameter_type);
    }

    // Ports in declared order (the declared order *is* the slice index passed
    // to `Module::process`; reordering here would break the contract with the
    // module impl, so this one is not canonicalised by name).
    encode_ports(&mut h, &d.inputs);
    encode_ports(&mut h, &d.outputs);

    h.finish()
}

fn encode_kind_payload(h: &mut Fnv1a64, kind: &ParameterKind) {
    match kind {
        ParameterKind::Enum { variants, .. } => {
            h.write_u32(variants.len() as u32);
            for v in *variants {
                h.write_str(v);
            }
        }
        ParameterKind::File { extensions } => {
            h.write_u32(extensions.len() as u32);
            for e in *extensions {
                h.write_str(e);
            }
        }
        ParameterKind::Float { .. }
        | ParameterKind::Int { .. }
        | ParameterKind::Bool { .. }
        | ParameterKind::SongName => {
            // Range and default are not part of shape: they affect clamping
            // behaviour, not wire layout. Tune one without forcing a hash
            // bump and a host/plugin refusal-to-load.
        }
    }
}

fn encode_ports(h: &mut Fnv1a64, ports: &[PortDescriptor]) {
    h.write_u32(ports.len() as u32);
    for p in ports {
        h.write_str(p.name);
        h.write_u32(p.index as u32);
        h.write_u8(port_kind_tag(&p.kind));
    }
}
