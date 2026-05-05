fn main() {
    let src_dir = "tree-sitter-patches/src";

    println!("cargo:rerun-if-changed={src_dir}/parser.c");
    println!("cargo:rerun-if-changed={src_dir}/grammar.json");
    println!("cargo:rerun-if-changed={src_dir}/node-types.json");

    cc::Build::new()
        .include(src_dir)
        .file(format!("{src_dir}/parser.c"))
        .warnings(false)
        .cargo_warnings(false)
        .compile("tree_sitter_patches");
}
