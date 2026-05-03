//! Helpers shared by `patches-tools` binaries and their tests.

use patches_manifest::{
    GeneratorInfo, ModuleManifest, ModuleManifestEntry, OwnedTemplate, SCHEMA_VERSION,
};
use patches_registry::Registry;

/// Build a [`ModuleManifest`] from every registered module in `registry`.
/// Modules are emitted in deterministic (sorted-name) order so the JSON
/// output is byte-stable for snapshot tests.
///
/// The bundled snapshot at `patches-manifest/data/module-manifest.json`
/// covers `default_registry()` only — i.e. in-tree modules. FFI plugin
/// templates extend the registry at runtime and are merged into the LSP
/// view at startup (ADR 0066 §3); they are intentionally absent from the
/// checked-in file.
pub fn build_manifest(registry: &Registry, generator: GeneratorInfo) -> ModuleManifest {
    let mut names: Vec<&str> = registry.module_names().collect();
    names.sort_unstable();
    let modules = names
        .into_iter()
        .map(|name| {
            let template = registry
                .template(name)
                .expect("registry.template() must succeed for a name from module_names()");
            ModuleManifestEntry {
                name: name.to_string(),
                template: OwnedTemplate::from(template),
            }
        })
        .collect();
    ModuleManifest {
        schema_version: SCHEMA_VERSION,
        generator,
        modules,
    }
}

/// Generator info with empty git_rev and timestamp. Use for the bundled
/// snapshot manifest so the file is byte-stable across builds.
pub fn deterministic_generator() -> GeneratorInfo {
    GeneratorInfo {
        tool: "patches-manifest".to_string(),
        git_rev: String::new(),
        timestamp: String::new(),
    }
}

/// Pretty-print a manifest as JSON with a trailing newline.
pub fn manifest_to_json(manifest: &ModuleManifest) -> String {
    let mut s = serde_json::to_string_pretty(manifest)
        .expect("ModuleManifest serialization is infallible");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::ModuleShape;
    use patches_manifest::static_registry::registry_from_manifest;

    /// Bundled manifest path, relative to this crate's manifest dir.
    const SNAPSHOT_PATH: &str = "../patches-manifest/data/module-manifest.json";

    fn manifest_text() -> String {
        let registry = patches_modules::default_registry();
        let manifest = build_manifest(&registry, deterministic_generator());
        manifest_to_json(&manifest)
    }

    /// Scope: in-tree `default_registry()` only. Plugin-supplied modules
    /// extend the registry at runtime and are not part of this snapshot.
    #[test]
    fn bundled_manifest_is_up_to_date() {
        let want = manifest_text();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SNAPSHOT_PATH);
        let have = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "failed to read bundled manifest at {}: {e}\n\
                 regenerate with: cargo run -p patches-tools --bin patches-manifest -- --json --deterministic --output {}",
                path.display(),
                path.display(),
            )
        });
        if want != have {
            panic!(
                "bundled manifest is stale at {}\n\
                 regenerate with: cargo run -p patches-tools --bin patches-manifest -- --json --deterministic --output {}",
                path.display(),
                path.display(),
            );
        }
    }

    /// Round-trip check: serialize default_registry → JSON → manifest →
    /// describe-only registry, then assert each module's descriptor
    /// matches `default_registry`'s for channels=1,2. Catches regressions
    /// in the owned-mirror or interning helpers.
    #[test]
    fn manifest_registry_descriptors_match_default_registry() {
        let registry_modules = patches_modules::default_registry();
        let manifest = build_manifest(&registry_modules, deterministic_generator());
        let registry_manifest = registry_from_manifest(&manifest);

        let mut names: Vec<String> = registry_modules
            .module_names()
            .map(str::to_string)
            .collect();
        names.sort();

        for channels in [1usize, 2usize] {
            let shape = ModuleShape { channels };
            for name in &names {
                let from_modules = registry_modules.describe(name, &shape);
                let from_manifest = registry_manifest.describe(name, &shape);
                match (from_modules, from_manifest) {
                    (Ok(a), Ok(b)) => assert_descriptors_equal(name, channels, &a, &b),
                    (Err(_), Err(_)) => {}
                    (Ok(_), Err(e)) => {
                        panic!("module {name} ch={channels}: manifest registry rejected: {e}")
                    }
                    (Err(e), Ok(_)) => {
                        panic!("module {name} ch={channels}: modules registry rejected: {e}")
                    }
                }
            }
        }
    }

    fn assert_descriptors_equal(
        name: &str,
        channels: usize,
        a: &patches_core::ModuleDescriptor,
        b: &patches_core::ModuleDescriptor,
    ) {
        assert_eq!(a.module_name, b.module_name, "{name} ch={channels}: name");
        assert_eq!(a.shape, b.shape, "{name} ch={channels}: shape");
        assert_eq!(a.inputs.len(), b.inputs.len(), "{name} ch={channels}: input count");
        for (x, y) in a.inputs.iter().zip(&b.inputs) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.index, y.index);
            assert_eq!(x.kind, y.kind);
            assert_eq!(x.mono_layout, y.mono_layout);
            assert_eq!(x.poly_layout, y.poly_layout);
        }
        assert_eq!(a.outputs.len(), b.outputs.len(), "{name} ch={channels}: output count");
        for (x, y) in a.outputs.iter().zip(&b.outputs) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.index, y.index);
            assert_eq!(x.kind, y.kind);
        }
        assert_eq!(
            a.realtime_params.len(),
            b.realtime_params.len(),
            "{name} ch={channels}: realtime_param count",
        );
        for (x, y) in a.realtime_params.iter().zip(&b.realtime_params) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.index, y.index);
        }
        assert_eq!(
            a.structural_params.len(),
            b.structural_params.len(),
            "{name} ch={channels}: structural_param count",
        );
        for (x, y) in a.structural_params.iter().zip(&b.structural_params) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.index, y.index);
        }
    }
}
