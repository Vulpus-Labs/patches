# Writing a native plugin

This chapter walks through building a native module plugin in Rust
with the `patches-sdk` crate. By the end you'll have a `.dylib` /
`.so` / `.dll` that the player and the CLAP plugin both load, and
the new module type will be usable from any `.patches` file.

We'll use the in-tree `test-plugins/gain/` as the worked example —
about eighty lines of Rust for a working unity-gain module. The
same pattern scales to anything you can express in the `Module`
trait.

If you want to write the bindings without the Rust SDK (for a non-
Rust SDK, say), [Native plugin ABI](extending-abi.md) is the
reference for the underlying C surface.

## What you get

A native plugin can ship one or more module types. Each module type
appears in `.patches` exactly like a built-in:

```patches
module g : Gain { gain: 0.5 }
```

There is no separate registration step in the patch file. As long
as the host has been pointed at a directory containing your bundle
(see *Loading the plugin* below), the module type is discovered at
host startup.

## Project layout

A plugin crate is a regular `cdylib`. The minimal `Cargo.toml`:

```toml
[package]
name = "my-gain-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
patches-sdk = "0.7"
# Optional, if you want DSP kernels (filters, ADSR, oversampling):
# patches-dsp = "0.6"
```

`patches-sdk` is the only required dependency. It re-exports the
`Module` trait, descriptors, ports, the `module_params!` and
`export_plugin!` macros, and the audio-thread types. Anything
reachable from the SDK's public API is supported; if you find
yourself depending on `patches-core` directly, file an issue —
the SDK gets deliberate extensions, not unannounced direct
dependencies on the foundation crates.

`patches-dsp` is intentionally *not* re-exported. If you need
filters, biquads, ADSR cores, oversampling, or similar, depend on
it explicitly.

## The smallest module

`test-plugins/gain/src/lib.rs`, in full:

```rust
use patches_sdk::cable_pool::CablePool;
use patches_sdk::cables::{InputPort, MonoInput, MonoOutput, OutputPort};
use patches_sdk::module_params;
use patches_sdk::modules::descriptor_template::{
    CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate,
};
use patches_sdk::modules::{InstanceId, ModuleDescriptor};
use patches_sdk::param_frame::ParamView;
use patches_sdk::ParameterKind;
use patches_sdk::{AudioEnvironment, Module};
use patches_sdk::{StructuralParams, BuildError};

module_params! {
    Gain {
        gain: Float,
    }
}

pub struct Gain {
    descriptor: ModuleDescriptor,
    instance_id: InstanceId,
    gain: f32,
    input: MonoInput,
    output: MonoOutput,
}

impl Module for Gain {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Gain",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::mono("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::mono("out")],
            per_axis_outputs: &[],
            realtime_params: &[ParameterTemplate {
                name: params::gain.as_str(),
                kind: ParameterKind::Float { min: 0.0, max: 2.0, default: 1.0 },
            }],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        Ok(Self {
            descriptor,
            instance_id,
            gain: 1.0,
            input: MonoInput::default(),
            output: MonoOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.gain = p.get(params::gain);
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let input_val = pool.read_mono(&self.input);
        pool.write_mono(&self.output, input_val * self.gain);
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.input  = MonoInput::from_ports(inputs, 0);
        self.output = MonoOutput::from_ports(outputs, 0);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

patches_sdk::export_plugin!(Gain, "Gain");
```

Eighty-eight lines. Let's read it.

### `module_params!`

```rust
module_params! {
    Gain {
        gain: Float,
    }
}
```

Generates a `params` submodule with typed parameter handles
(`params::gain`). The names you list here will be the parameter
names visible in `.patches` files (`{ gain: 0.5 }`).

### `template()`

```rust
fn template() -> ModuleDescriptorTemplate { ... }
```

A const `ModuleDescriptorTemplate` describing the module's name,
ports, and parameters. The fields here lower 1:1 to the JSON
described in [Native plugin ABI](extending-abi.md):

- **`name`** — the type name as seen in `.patches`. `Gain` here →
  `module g : Gain` in a patch.
- **`axes`** — shape axes. `CountAxis::CHANNELS` enables a
  `channels` shape argument. Modules with no shape variation just
  use `&[CountAxis::CHANNELS]` and ignore it.
- **`global_inputs` / `global_outputs`** — ports declared once,
  regardless of shape.
- **`per_axis_inputs` / `per_axis_outputs`** — ports expanded one
  per axis value (e.g. one input per channel for a multi-channel
  mixer).
- **`realtime_params`** — audio-thread-mutable parameters.
- **`structural_params`** — control-thread-only parameters; values
  are sealed at `prepare()` time and don't appear in
  `update_validated_parameters`.

`PortTemplate::mono("in")` is shorthand for a mono audio port.
`PortTemplate::poly`, `PortTemplate::stereo`, and the `_trigger`
variants exist for the other kinds.

### `prepare()`

Called once per instance, on the control thread, before the audio
thread sees the module. Allocate scratch buffers here; no
allocation is permitted on the audio thread.

The `_structural` parameter carries the resolved values of any
structural parameters (file paths, song references, etc.). For a
module with no structural params, the SDK passes an empty view.

Return `Ok(self)` to commit the instance; `Err(BuildError::...)`
to surface a load-time error in the host's diagnostics.

### `update_validated_parameters()`

Audio-thread. The host pushes a packed parameter frame whenever a
patch parameter changes (DSL parameter change on reload, knob
turn in the plugin GUI, host automation). Copy the values into
your module state.

```rust
fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
    self.gain = p.get(params::gain);
}
```

`ParamView::get` is type-checked against the `module_params!`
declaration. If you ask for a parameter that doesn't exist, you
get a compile error.

### `process()`

Audio-thread. Called once per sample. Read inputs from the cable
pool, write outputs back. **No allocation, no blocking, no I/O,
no syscalls.**

```rust
fn process(&mut self, pool: &mut CablePool<'_>) {
    let input_val = pool.read_mono(&self.input);
    pool.write_mono(&self.output, input_val * self.gain);
}
```

For poly ports use `read_poly` / `write_poly`; for stereo,
`read_stereo` / `write_stereo`.

### `set_ports()`

Audio-thread. Called when the host's planner connects (or
reconnects) cables to this instance. Cache the port handles you
were given:

```rust
fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
    self.input  = MonoInput::from_ports(inputs, 0);
    self.output = MonoOutput::from_ports(outputs, 0);
}
```

The slice indices match the order of port declarations in
`template()`. `MonoInput::from_ports(inputs, 0)` reads the first
input.

### `export_plugin!`

```rust
patches_sdk::export_plugin!(Gain, "Gain");
```

Macro that emits the eight C ABI entry points, the
`patches_plugin_init` symbol the host loads, and a descriptor-hash
symbol. The string argument is the module's user-facing name
(matching `template().name`).

For multi-module bundles use `export_modules!` instead:

```rust
patches_sdk::export_modules!(
    (Gain, "Gain"),
    (Compress, "Compress"),
    (Reverb, "Reverb"),
);
```

## Building

```bash
cargo build --release -p my-gain-plugin
```

The result is in `target/release/`:

| OS      | Filename                    |
| ------- | --------------------------- |
| macOS   | `libmy_gain_plugin.dylib`   |
| Linux   | `libmy_gain_plugin.so`      |
| Windows | `my_gain_plugin.dll`        |

This is the loadable bundle. The Patches host doesn't care about
the filename — it loads anything with `.dylib` / `.so` / `.dll`
extension in a scanned directory, calls `patches_plugin_init`, and
registers whatever module types it finds inside.

(Some hosts use a `.pxm` extension as a Patches-specific marker.
The library extension still works; `.pxm` is just a convention so
your shell file manager doesn't mix module bundles with unrelated
dylibs.)

## Loading the plugin

The host can find your bundle in four ways. They're tried in this
priority order:

1. **`PATCHES_PLUGIN_PATH` env var.** A colon-separated list of
   directories or specific bundle files.
2. **CLI / GUI per-session list.** The player's `--module-path
   <DIR|FILE>` flag (repeatable). The CLAP plugin's
   *Module-paths panel* (Add directory… / Add dylib…). These live
   for the duration of one session unless you also save them via:
3. **`GlobalConfig::bundle_dirs` from `settings.toml`.** The
   persisted host-scoped bundle dir list (`patches-plugin-common`'s
   global config). Add via the player's TUI "Add bundle directory"
   action or the CLAP GUI's *Add directory…* button; persists
   across sessions.
4. **Default data directory.** `<ProjectDirs.data_dir()>/bundles/`,
   if it exists. Never auto-created.

A workspace-developer convenience also kicks in: bundles built into
`target/<profile>/` of the current Patches workspace are
auto-scanned without any configuration — see `stdlib_scanner` in
`patches-ffi/src/scanner.rs`.

In practice for a one-off bundle:

```bash
patch_player --module-path target/release my_patch.patches
```

For permanent install, copy the dylib to a stable location and add
the directory via the global config from inside the player (TUI:
"Add bundle directory") or the CLAP plugin GUI.

## Audio-thread rules

The same constraints that apply to in-tree modules apply across the
FFI:

- **No allocation** in `process`, `update_validated_parameters`,
  or `set_ports`. Pre-allocate in `prepare`.
- **No blocking** — no mutexes, no I/O, no syscalls.
- **No unbounded loops** — the audio block is a few hundred
  samples; spending milliseconds in one tick will glitch.

The host's CPU monitor will flag instances that take too long; the
tick-boundary panic catcher halts the engine cleanly if your code
panics. (Your crate must use `panic = "unwind"` — the default —
not `panic = "abort"`, or the catch_unwind machinery in the host
can't recover.)

## What to read next

- [Native plugin ABI](extending-abi.md) — the C surface, JSON
  schemas, and wire formats. Read this if you're building tooling
  around the FFI or implementing an SDK in another language.
- [Implementing modules](implementing-modules.md) — the same
  `Module` trait, but for in-tree Rust modules. Useful for
  cross-reference; the contract is identical apart from the
  packaging.
- `test-plugins/conv-reverb/` — a slightly more involved example
  with a structural param (file path).
