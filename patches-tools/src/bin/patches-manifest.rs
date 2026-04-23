//! `patches-manifest` — list every module in the registry with its
//! `ModuleDescriptor` at `channels = 1` and, if different, `channels = 2`.
//! Optionally scans FFI plugin bundles.
//!
//! Usage:
//!   patches-manifest [--module-path DIR|FILE]...

use std::path::PathBuf;
use std::process;

use patches_core::cables::{CableKind, MonoLayout, PolyLayout};
use patches_core::{ModuleDescriptor, ModuleShape, ParameterKind, PortDescriptor};

fn kind_str(p: &PortDescriptor) -> &'static str {
    match (p.kind.clone(), p.mono_layout, p.poly_layout) {
        (CableKind::Mono, MonoLayout::Trigger, _) => "trigger",
        (CableKind::Mono, MonoLayout::Audio, _) => "mono",
        (CableKind::Poly, _, PolyLayout::Trigger) => "poly_trigger",
        (CableKind::Poly, _, PolyLayout::Midi) => "midi",
        (CableKind::Poly, _, PolyLayout::Transport) => "transport",
        (CableKind::Poly, _, PolyLayout::Audio) => "poly",
    }
}

fn param_str(p: &ParameterKind) -> String {
    match p {
        ParameterKind::Float { min, max, default } => {
            format!("float [{min}..{max}] = {default}")
        }
        ParameterKind::Int { min, max, default } => {
            format!("int   [{min}..{max}] = {default}")
        }
        ParameterKind::Bool { default } => format!("bool  = {default}"),
        ParameterKind::Enum { variants, default } => {
            format!("enum  {{{}}} = {default}", variants.join(", "))
        }
        ParameterKind::File { extensions } => {
            format!("file  (extensions: {})", extensions.join(", "))
        }
        ParameterKind::SongName => "song_name".to_string(),
    }
}

/// Render a port/param line with the given index suffix if >0 or if part
/// of an indexed group.
fn render_port(name: &str, index: usize, indexed: bool, port: &PortDescriptor) -> String {
    let label = if indexed {
        format!("{name}[{index}]")
    } else {
        name.to_string()
    };
    format!("    {:<24} ({})", label, kind_str(port))
}

fn render_param(name: &str, index: usize, indexed: bool, kind: &ParameterKind) -> String {
    let label = if indexed {
        format!("{name}[{index}]")
    } else {
        name.to_string()
    };
    format!("    {:<24} {}", label, param_str(kind))
}

/// Group ports/params by name so indexed groups (`in[0]..in[N]`) print as
/// `in[i]` once, not N times.
fn print_descriptor(desc: &ModuleDescriptor) {
    println!(
        "  shape: channels={}, length={}, high_quality={}",
        desc.shape.channels, desc.shape.length, desc.shape.high_quality
    );

    if !desc.inputs.is_empty() {
        println!("  inputs:");
        print_port_group(&desc.inputs, /*is_input=*/ true);
    }
    if !desc.outputs.is_empty() {
        println!("  outputs:");
        print_port_group(&desc.outputs, /*is_input=*/ false);
    }
    if !desc.parameters.is_empty() {
        println!("  parameters:");
        print_param_group(&desc.parameters);
    }
}

fn print_port_group(ports: &[patches_core::PortDescriptor], _is_input: bool) {
    // Detect indexed groups: name repeats with monotonically increasing index.
    let mut i = 0;
    while i < ports.len() {
        let name = ports[i].name;
        let mut j = i + 1;
        while j < ports.len() && ports[j].name == name {
            j += 1;
        }
        let group = &ports[i..j];
        if group.len() == 1 && group[0].index == 0 {
            println!("{}", render_port(name, 0, false, &group[0]));
        } else {
            // Indexed: show range.
            let first = &group[0];
            let last = &group[group.len() - 1];
            println!(
                "    {:<24} ({})  [i = {}..{}]",
                format!("{name}[i]"),
                kind_str(first),
                first.index,
                last.index
            );
        }
        i = j;
    }
}

fn print_param_group(params: &[patches_core::ParameterDescriptor]) {
    let mut i = 0;
    while i < params.len() {
        let name = params[i].name;
        let mut j = i + 1;
        while j < params.len() && params[j].name == name {
            j += 1;
        }
        let group = &params[i..j];
        if group.len() == 1 && group[0].index == 0 {
            println!(
                "{}",
                render_param(name, 0, false, &group[0].parameter_type)
            );
        } else {
            let first = &group[0];
            let last = &group[group.len() - 1];
            println!(
                "    {:<24} {}  [i = {}..{}]",
                format!("{name}[i]"),
                param_str(&first.parameter_type),
                first.index,
                last.index
            );
        }
        i = j;
    }
}

/// Two descriptors are "structurally equivalent" for manifest purposes if
/// their port and parameter lists (name, index, kind/type) are identical.
/// Shape.channels is allowed to differ — that's the axis we're varying.
fn descriptors_equivalent(a: &ModuleDescriptor, b: &ModuleDescriptor) -> bool {
    if a.inputs.len() != b.inputs.len()
        || a.outputs.len() != b.outputs.len()
        || a.parameters.len() != b.parameters.len()
    {
        return false;
    }
    for (x, y) in a.inputs.iter().zip(&b.inputs) {
        if x.name != y.name || x.index != y.index || x.kind != y.kind {
            return false;
        }
    }
    for (x, y) in a.outputs.iter().zip(&b.outputs) {
        if x.name != y.name || x.index != y.index || x.kind != y.kind {
            return false;
        }
    }
    for (x, y) in a.parameters.iter().zip(&b.parameters) {
        if x.name != y.name || x.index != y.index {
            return false;
        }
    }
    true
}

fn print_usage() {
    eprintln!("usage: patches-manifest [--module-path DIR|FILE]...");
}

fn main() {
    let mut module_paths: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--module-path" => match args.next() {
                Some(p) => module_paths.push(PathBuf::from(p)),
                None => {
                    eprintln!("error: --module-path requires an argument");
                    process::exit(2);
                }
            },
            "-h" | "--help" => {
                print_usage();
                return;
            }
            other => {
                eprintln!("error: unexpected argument: {other}");
                print_usage();
                process::exit(2);
            }
        }
    }

    let mut registry = patches_modules::default_registry();
    if !module_paths.is_empty() {
        let scanner = patches_ffi::PluginScanner::new(module_paths);
        let report = scanner.scan(&mut registry);
        for (p, e) in &report.errors {
            eprintln!("plugin scan error: {}: {e}", p.display());
        }
    }

    let mut names: Vec<String> = registry.module_names().map(str::to_string).collect();
    names.sort();

    for name in &names {
        let one = registry.describe(
            name,
            &ModuleShape { channels: 1, length: 0, high_quality: false },
        );
        let two = registry.describe(
            name,
            &ModuleShape { channels: 2, length: 0, high_quality: false },
        );

        println!("## {name}");
        match (&one, &two) {
            (Ok(d1), Ok(d2)) => {
                if descriptors_equivalent(d1, d2) {
                    // Shape-invariant: show once.
                    print_descriptor(d1);
                } else {
                    println!();
                    println!("  --- channels = 1 ---");
                    print_descriptor(d1);
                    println!();
                    println!("  --- channels = 2 ---");
                    print_descriptor(d2);
                }
            }
            (Ok(d1), Err(e2)) => {
                print_descriptor(d1);
                println!("  (channels=2 unsupported: {e2})");
            }
            (Err(e1), Ok(d2)) => {
                println!("  (channels=1 unsupported: {e1})");
                print_descriptor(d2);
            }
            (Err(e1), Err(_)) => {
                println!("  (describe failed: {e1})");
            }
        }
        println!();
    }
}
