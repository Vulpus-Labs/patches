//! End-to-end smoke test: exercise the binary against an example patch.

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_patches-svg")
}

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn renders_example_patch_to_stdout() {
    let input = workspace_root().join("examples/pad.patches");
    assert!(input.exists(), "example patch missing: {input:?}");

    let out = Command::new(bin())
        .arg(&input)
        .output()
        .expect("run patches-svg");
    assert!(
        out.status.success(),
        "cli failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf8 stdout");
    assert!(stdout.starts_with("<svg"), "unexpected prefix");
    assert!(stdout.contains("</svg>"), "missing closing tag");
}

#[test]
fn missing_input_exits_nonzero() {
    let out = Command::new(bin())
        .arg("/does/not/exist.patches")
        .output()
        .expect("run patches-svg");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("patches-svg"));
}
