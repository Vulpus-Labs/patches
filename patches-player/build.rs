//! Build-time splash image preprocessor.
//!
//! Looks for `assets/splash.png` (or `.jpg`) in the crate root. If
//! present, decodes, resizes to fit `MAX_W × MAX_H` pixels (preserving
//! aspect), and emits a generated `splash.rs` with the pixel grid as
//! a const `[u8; 3 * w * h]` plus `WIDTH` / `HEIGHT` consts.
//!
//! At runtime the image is rendered using Braille characters (one
//! cell = 2×4 sub-pixels), so the bake target is sized at 2×
//! horizontal and 4× vertical cell counts. Width must be even and
//! height must be a multiple of 4 so the Braille packing is exact.
//!
//! If no splash image is found, an empty placeholder is emitted; the
//! runtime treats `WIDTH == 0` as "no splash, skip".

use std::env;
use std::fs;
use std::path::PathBuf;

// Bake target = 2 × cells_w × 4 × cells_h. At 200 cols × 50 rows of
// terminal cells this is 400 × 200 sub-pixels.
const MAX_W: u32 = 400;
const MAX_H: u32 = 200;

fn main() {
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let candidates = [
        crate_dir.join("assets/splash.png"),
        crate_dir.join("assets/splash.jpg"),
        crate_dir.join("assets/splash.jpeg"),
    ];
    let chosen = candidates.iter().find(|p| p.exists());

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let out_path = out_dir.join("splash.rs");

    if let Some(path) = chosen {
        println!("cargo:rerun-if-changed={}", path.display());
        let img = image::open(path).expect("decode splash image");
        let resized = img.resize(MAX_W, MAX_H, image::imageops::FilterType::Lanczos3);
        let rgb = resized.to_rgb8();
        let (w, h) = rgb.dimensions();
        // Trim to a width multiple of 2 and height multiple of 4 so the
        // Braille 2×4 sub-pixel packing is exact.
        let w_aligned = w - (w % 2);
        let h_aligned = h - (h % 4);
        let mut bytes: Vec<u8> = Vec::with_capacity((3 * w_aligned * h_aligned) as usize);
        for y in 0..h_aligned {
            for x in 0..w_aligned {
                let p = rgb.get_pixel(x, y);
                bytes.push(p.0[0]);
                bytes.push(p.0[1]);
                bytes.push(p.0[2]);
            }
        }
        let body = format!(
            "pub const WIDTH: usize = {w};\n\
             pub const HEIGHT: usize = {h_aligned};\n\
             pub const PIXELS: &[u8] = &{bytes:?};\n",
            w = w_aligned,
            h_aligned = h_aligned,
            bytes = bytes,
        );
        fs::write(&out_path, body).unwrap();
    } else {
        let body = "pub const WIDTH: usize = 0;\n\
                    pub const HEIGHT: usize = 0;\n\
                    pub const PIXELS: &[u8] = &[];\n";
        fs::write(&out_path, body).unwrap();
    }

    // Also rerun if the assets dir changes existence.
    let assets = crate_dir.join("assets");
    println!("cargo:rerun-if-changed={}", assets.display());

    embed_windows_icon(&crate_dir);
}

// Embed the workspace icon into patch_player.exe as resource index 0.
// Only runs when target = Windows AND host = Windows: cross-compiling
// from macOS/Linux would need an RC toolchain (llvm-rc / mingw windres);
// release builds happen on the Windows CI runner where this is native.
#[cfg(target_os = "windows")]
fn embed_windows_icon(crate_dir: &PathBuf) {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    let icon = crate_dir.join("../assets/patches.ico");
    if !icon.exists() {
        return;
    }
    println!("cargo:rerun-if-changed={}", icon.display());
    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon.to_str().expect("icon path is valid UTF-8"));
    res.compile().expect("embed windows icon");
}

#[cfg(not(target_os = "windows"))]
fn embed_windows_icon(_crate_dir: &PathBuf) {}
