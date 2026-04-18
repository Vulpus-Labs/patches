//! Integration tests for the analysis module. Split by category from the
//! original 785-line `tests.rs` per ticket 0533. Shared fixtures and
//! harness helpers live here; behaviour-specific tests live in sibling
//! submodules.

#![allow(unused_imports)]

pub(super) use super::*;

pub(super) use super::deps::resolve_dependencies;
pub(super) use super::scan::shallow_scan;
pub(super) use crate::ast_builder::build_ast;
pub(super) use crate::navigation::SymbolKind;
pub(super) use crate::parser::language;
pub(super) use patches_modules::default_registry;

pub(super) fn parse(source: &str) -> ast::File {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    let (file, _) = build_ast(&tree, source);
    file
}

pub(super) fn analyse_source(source: &str) -> SemanticModel {
    let file = parse(source);
    let registry = default_registry();
    analyse(&file, &registry)
}

mod deps;
mod descriptors;
mod navigation;
mod scan;
mod tracker;
mod validation;
