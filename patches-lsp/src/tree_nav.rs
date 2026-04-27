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

use crate::lsp_util::{find_ancestor, first_named_child_of_kind, node_text};
use tree_sitter::{Node, Tree};

// ─── Tree-sitter field-extraction helpers ────────────────────────────────
//
// These helpers localise the `child_by_field_name(...) + node_text` pattern
// that would otherwise recur across every feature handler, along with the
// handful of module-type literals LSP behaviour depends on. A grammar field
// rename or module rename now edits one site.

/// Type name of the `MasterSequencer` module. LSP special-cases it in a
/// couple of places (the `song:` completion slot and a song-name hover on
/// its param block); centralise the literal so a module rename is one edit.
pub(crate) const MASTER_SEQUENCER: &str = "MasterSequencer";

/// Source text of a `module_decl`'s `type` field (e.g. `Osc`, `voice`,
/// `MasterSequencer`), or `None` when the decl's type slot is empty.
pub(crate) fn module_type_name<'a>(
    module_decl: Node<'_>,
    source: &'a str,
) -> Option<&'a str> {
    module_decl
        .child_by_field_name("type")
        .map(|n| node_text(n, source))
}

/// Source text of a `module_decl`'s `name` field, or `None` when the decl's
/// name slot is empty.
pub(crate) fn module_instance_name<'a>(
    module_decl: Node<'_>,
    source: &'a str,
) -> Option<&'a str> {
    module_decl
        .child_by_field_name("name")
        .map(|n| node_text(n, source))
}

/// True when the `module_decl`'s declared module type is
/// [`MASTER_SEQUENCER`].
pub(crate) fn is_master_sequencer(module_decl: Node<'_>, source: &str) -> bool {
    module_type_name(module_decl, source) == Some(MASTER_SEQUENCER)
}

/// Source text of a `template`'s `name` field.
pub(crate) fn template_name<'a>(template_node: Node<'_>, source: &'a str) -> Option<&'a str> {
    template_node
        .child_by_field_name("name")
        .map(|n| node_text(n, source))
}

/// A classification of what the cursor is on, carrying the relevant
/// tree-sitter nodes so handlers don't have to re-walk to find them.
///
/// Field set is intentionally rich so both hover (needs per-node info for
/// detail rendering) and completions (needs enclosing decl for scoped
/// lookups) can consume the same classification. Fields that individual
/// handlers don't use today are retained for future handlers.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CursorContext<'tree> {
    /// Type name in `module <name> : <Type>`. `node` is the type identifier.
    ModuleType {
        node: Node<'tree>,
        #[allow(dead_code)]
        module_decl: Node<'tree>,
    },
    /// Cursor sits in the type slot of `module <name> : ` with no type
    /// identifier parsed yet (empty or trailing whitespace). Completions
    /// offers module-type names; hover ignores this variant.
    ModuleTypeSlot {
        #[allow(dead_code)]
        module_decl: Node<'tree>,
    },
    /// Port label in a port reference (`osc.<port>[...]`). `label_node`
    /// spans just the port label (ident or param_ref); `port_ref_node`
    /// is the enclosing port_ref.
    PortRef {
        #[allow(dead_code)]
        label_node: Node<'tree>,
        port_ref_node: Node<'tree>,
    },
    /// Module instance name in `module <name> : Type`. `node` is the
    /// name identifier.
    ModuleName {
        node: Node<'tree>,
        #[allow(dead_code)]
        module_decl: Node<'tree>,
    },
    /// Inside a `{ ... }` param block on a module declaration.
    ParamBlock {
        #[allow(dead_code)]
        param_block: Node<'tree>,
        module_decl: Node<'tree>,
    },
    /// Inside a `( ... )` shape block on a module declaration.
    ShapeBlock {
        #[allow(dead_code)]
        shape_block: Node<'tree>,
        #[allow(dead_code)]
        module_decl: Node<'tree>,
    },
    /// Cursor sits on a tap component name (`meter`, `osc`, ...).
    TapType { node: Node<'tree> },
    /// Cursor sits on the tap name (the first ident inside `~...(...)`).
    TapName { node: Node<'tree> },
    /// Inside a song/section/pattern-row structure where pattern names are
    /// the relevant completion set.
    SongRow {
        #[allow(dead_code)]
        node: Node<'tree>,
    },
    /// Could not classify (cursor on whitespace, inside a comment, or on
    /// a node kind the helper doesn't recognise).
    Unknown,
}

/// Tree-sitter node kinds that mean "cursor is inside a song/play/section
/// structure where pattern-name completion is wanted".
const SONG_KINDS: &[&str] = &[
    "song_row",
    "song_cell",
    "row_elem",
    "inline_block",
    "named_inline",
    "section_def",
];

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
            if let Some(ctx) = classify_node(node, byte_offset) {
                return ctx;
            }
        }
    }
    CursorContext::Unknown
}

fn classify_node(node: Node<'_>, byte_offset: usize) -> Option<CursorContext<'_>> {
    // Tap-target tokens (ADR 0054 §1). Tested before port_ref since a
    // tap_target sits in the same syntactic slot as a port_ref endpoint.
    if let Some(tap_ctx) = classify_tap_node(node) {
        return Some(tap_ctx);
    }

    // Module type / name identifier: `module v : Type`.
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

    // Inside a module_decl: param block, shape block, or empty type slot.
    // Uses `ancestor_or_self` so the cursor landing on `param_block` /
    // `shape_block` / `module_decl` itself (common for whitespace positions)
    // still classifies correctly.
    if let Some(module_decl) = ancestor_or_self(node, "module_decl") {
        if let Some(param_block) = ancestor_or_self(node, "param_block") {
            return Some(CursorContext::ParamBlock {
                param_block,
                module_decl,
            });
        }
        if let Some(shape_block) = ancestor_or_self(node, "shape_block") {
            return Some(CursorContext::ShapeBlock {
                shape_block,
                module_decl,
            });
        }
        if is_in_type_slot(byte_offset, module_decl) {
            return Some(CursorContext::ModuleTypeSlot { module_decl });
        }
    }

    // Song block: pattern-name completion target.
    let mut cur = node;
    loop {
        if SONG_KINDS.contains(&cur.kind()) {
            return Some(CursorContext::SongRow { node: cur });
        }
        cur = match cur.parent() {
            Some(p) => p,
            None => break,
        };
    }

    None
}

/// Tap-target sub-tokens (`tap_type`, `tap_param_key`, `tap_name`).
///
/// The cursor often lands on the inner `ident` rather than on the
/// wrapper rule, so we accept either: walk up at most one parent to
/// reach the wrapper.
fn classify_tap_node(node: Node<'_>) -> Option<CursorContext<'_>> {
    if node.kind() == "tap_type" {
        return Some(CursorContext::TapType { node });
    }
    if let Some(parent) = node.parent() {
        match parent.kind() {
            "tap_type" => return Some(CursorContext::TapType { node: parent }),
            "tap_name" => return Some(CursorContext::TapName { node: parent }),
            _ => {}
        }
    }
    None
}

/// Find `kind` on `node` itself or any of its ancestors. Handy for cursor
/// positions that descend to the container (whitespace inside `{ … }`)
/// rather than to a child.
fn ancestor_or_self<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    if node.kind() == kind {
        Some(node)
    } else {
        find_ancestor(node, kind)
    }
}

/// True when `byte_offset` is past the `:` of a `module <name> : …` decl but
/// not yet inside its shape or param block, i.e. the type slot is empty or
/// the cursor is within a partial type identifier.
pub(crate) fn is_in_type_slot(byte_offset: usize, module_decl: Node<'_>) -> bool {
    let mut child_cursor = module_decl.walk();
    let mut found_colon = false;
    for child in module_decl.children(&mut child_cursor) {
        if child.kind() == ":" && child.end_byte() <= byte_offset {
            found_colon = true;
        }
    }
    if !found_colon {
        return false;
    }

    let at_type = module_decl
        .child_by_field_name("type")
        .is_none_or(|t| byte_offset <= t.end_byte());
    if at_type {
        return true;
    }

    let no_shape = first_named_child_of_kind(module_decl, "shape_block")
        .is_none_or(|s| byte_offset < s.start_byte());
    let no_params = first_named_child_of_kind(module_decl, "param_block")
        .is_none_or(|p| byte_offset < p.start_byte());
    no_shape && no_params
}
