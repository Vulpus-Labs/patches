//! Shared tree-sitter cursor-context classifier.
//!
//! Both hover and completions need to know: at this byte offset, is the
//! cursor sitting on a module-type reference, a port label, a module
//! instance name, etc.? The logic had drifted into two idioms — hover's
//! ancestor chain of `try_hover_*` predicates, and completions' cursor
//! loop with `cursor.kind()` match arms — making a sixth handler author
//! choose between them.
//!
//! This module exposes [`classify_cursor`] returning a [`CursorContext`]
//! enum. Handlers match on the enum and dispatch; the tree walking is
//! shared.

use crate::lsp_util::{find_ancestor, first_named_child_of_kind};
use tree_sitter::{Node, Tree};

/// A classification of what the cursor is on, carrying the relevant
/// tree-sitter nodes so handlers don't have to re-walk to find them.
///
/// Field set is intentionally rich so both hover (needs per-node info for
/// detail rendering) and completions (needs enclosing decl for scoped
/// lookups) can consume the same classification. Fields that individual
/// handlers don't use today are retained for future handlers.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum CursorContext<'tree> {
    /// Type name in `module <name> : <Type>`. `node` is the type identifier.
    ModuleType {
        node: Node<'tree>,
        module_decl: Node<'tree>,
    },
    /// Port label in a port reference (`osc.<port>[...]`). `label_node`
    /// spans just the port label (ident or param_ref); `port_ref_node`
    /// is the enclosing port_ref.
    PortRef {
        label_node: Node<'tree>,
        port_ref_node: Node<'tree>,
    },
    /// Module instance name in `module <name> : Type`. `node` is the
    /// name identifier.
    ModuleName {
        node: Node<'tree>,
        module_decl: Node<'tree>,
    },
    /// Inside a `{ ... }` param block on a module declaration.
    ParamBlock {
        param_block: Node<'tree>,
        module_decl: Node<'tree>,
    },
    /// Inside a `( ... )` shape block on a module declaration.
    ShapeBlock {
        shape_block: Node<'tree>,
        module_decl: Node<'tree>,
    },
    /// Could not classify (cursor on whitespace, inside a comment, or on
    /// a node kind the helper doesn't recognise).
    Unknown,
}

/// Classify the cursor at `byte_offset` into a [`CursorContext`].
///
/// Tries `byte_offset` and, if the cursor is just past a token, also
/// `byte_offset - 1` so clicking at the trailing edge of an identifier
/// still resolves to that identifier.
pub(crate) fn classify_cursor(tree: &Tree, byte_offset: usize) -> CursorContext<'_> {
    let root = tree.root_node();
    let candidates: &[usize] = if byte_offset > 0 {
        &[byte_offset, byte_offset - 1]
    } else {
        &[byte_offset]
    };

    for &off in candidates {
        if let Some(node) = root.descendant_for_byte_range(off, off) {
            if let Some(ctx) = classify_node(node) {
                return ctx;
            }
        }
    }
    CursorContext::Unknown
}

fn classify_node(node: Node<'_>) -> Option<CursorContext<'_>> {
    // Module type name: `module v : Type`.
    if let Some(parent) = node.parent() {
        if parent.kind() == "module_decl" {
            if let Some(type_node) = parent.child_by_field_name("type") {
                if type_node.id() == node.id() {
                    return Some(CursorContext::ModuleType {
                        node,
                        module_decl: parent,
                    });
                }
            }
            if let Some(name_node) = parent.child_by_field_name("name") {
                if name_node.id() == node.id() {
                    return Some(CursorContext::ModuleName {
                        node,
                        module_decl: parent,
                    });
                }
            }
        }
    }

    // Port label inside a port_ref.
    if let Some(port_ref_node) = if node.kind() == "port_ref" {
        Some(node)
    } else {
        node.parent().filter(|p| p.kind() == "port_ref").or_else(|| {
            node.parent()
                .filter(|p| p.kind() == "port_label")
                .and_then(|p| p.parent())
        }).or_else(|| find_ancestor(node, "port_ref"))
    } {
        if let Some(port_label_node) = first_named_child_of_kind(port_ref_node, "port_label") {
            if node.start_byte() >= port_label_node.start_byte()
                && node.end_byte() <= port_label_node.end_byte()
            {
                return Some(CursorContext::PortRef {
                    label_node: port_label_node,
                    port_ref_node,
                });
            }
        }
    }

    // Inside a param or shape block.
    if let Some(module_decl) = find_ancestor(node, "module_decl") {
        if let Some(param_block) = find_ancestor(node, "param_block") {
            return Some(CursorContext::ParamBlock {
                param_block,
                module_decl,
            });
        }
        if let Some(shape_block) = find_ancestor(node, "shape_block") {
            return Some(CursorContext::ShapeBlock {
                shape_block,
                module_decl,
            });
        }
    }

    None
}
