fn main() {
    let src_dir = "tree-sitter-patches/src";

    cc::Build::new()
        .include(src_dir)
        .file(format!("{src_dir}/parser.c"))
        .warnings(false)
        .cargo_warnings(false)
        .compile("tree_sitter_patches");
}
