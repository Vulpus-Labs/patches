use crate::ast::{Ident, PatternBlock, PatternChannel, SongBlock, SongCellRef, SongRow};
use crate::lsp_util::first_named_child_of_kind;
use super::{named_children_of_kind, node_text, span_of, build_ident};
use super::diagnostics::{Diagnostic, walk_errors};

pub(super) fn build_pattern_block(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> PatternBlock {
    walk_errors(node, diags);

    let name = node.child_by_field_name("name").map(|n| build_ident(n, source));

    let channels = named_children_of_kind(node, "channel_row")
        .into_iter()
        .map(|n| build_pattern_channel(n, source, diags))
        .collect();

    PatternBlock {
        name,
        channels,
        span: span_of(node),
    }
}

fn build_pattern_channel(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> PatternChannel {
    walk_errors(node, diags);

    let label = node.child_by_field_name("label").map(|n| build_ident(n, source));

    // Count step nodes (including those after continuation `|`)
    let step_count = named_children_of_kind(node, "step").len();

    PatternChannel {
        label,
        step_count,
        span: span_of(node),
    }
}

pub(super) fn build_song_block(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> SongBlock {
    walk_errors(node, diags);

    let name = node.child_by_field_name("name").map(|n| build_ident(n, source));

    // Lanes live in a `song_lanes` child: "(" ident ("," ident)* ")"
    let mut channel_names: Vec<Ident> = Vec::new();
    if let Some(lanes_node) = first_named_child_of_kind(node, "song_lanes") {
        for id in named_children_of_kind(lanes_node, "ident") {
            channel_names.push(build_ident(id, source));
        }
    }

    // Walk song items; flatten all song_cells found inside (sections and
    // play bodies) into a single row of cells — the LSP uses this for
    // navigation only, so we don't reconstruct row boundaries here.
    let mut cells: Vec<SongCellRef> = Vec::new();
    let mut is_loop_point = false;
    for item in named_children_of_kind(node, "song_item") {
        collect_song_item_cells(item, source, &mut cells, &mut is_loop_point);
    }
    let rows = if cells.is_empty() {
        Vec::new()
    } else {
        vec![SongRow {
            cells,
            is_loop_point,
            span: span_of(node),
        }]
    };

    SongBlock {
        name,
        channel_names,
        rows,
        span: span_of(node),
    }
}

fn collect_song_cell(cell_node: tree_sitter::Node, source: &str, out: &mut Vec<SongCellRef>) {
    let text = node_text(cell_node, source);
    if text == "_" {
        out.push(SongCellRef {
            name: None,
            is_silence: true,
            span: span_of(cell_node),
        });
    } else if let Some(id) = first_named_child_of_kind(cell_node, "ident") {
        out.push(SongCellRef {
            name: Some(build_ident(id, source)),
            is_silence: false,
            span: span_of(cell_node),
        });
    }
}

fn collect_song_item_cells(
    item: tree_sitter::Node,
    source: &str,
    cells: &mut Vec<SongCellRef>,
    is_loop_point: &mut bool,
) {
    // A song_item wraps exactly one child: section_def, pattern_block,
    // play_stmt, or loop_marker. Walk into each kind and accumulate cells.
    let mut cursor = item.walk();
    for child in item.named_children(&mut cursor) {
        match child.kind() {
            "loop_marker" => *is_loop_point = true,
            "section_def" | "play_stmt" => collect_cells_recursive(child, source, cells),
            "pattern_block" => {} // inline pattern — skipped here
            _ => {}
        }
    }
}

fn collect_cells_recursive(node: tree_sitter::Node, source: &str, cells: &mut Vec<SongCellRef>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "song_cell" => collect_song_cell(child, source, cells),
            "ident" if matches!(node.kind(), "play_atom") => {
                // Section ref inside a play expression: record as a cell so
                // the reference is visible to go-to-definition.
                cells.push(SongCellRef {
                    name: Some(build_ident(child, source)),
                    is_silence: false,
                    span: span_of(child),
                });
            }
            _ => collect_cells_recursive(child, source, cells),
        }
    }
}
