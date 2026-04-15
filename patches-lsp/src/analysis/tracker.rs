//! Phase 4b: validate tracker references — undefined patterns in songs,
//! undefined songs in MasterSequencer params, channel count mismatches.
//!
//! Split from `validate` so the body-validation phase 4 stays focused on
//! connections/parameters and the tracker phase has a single home. Both
//! phases consume the resolved [`DeclarationMap`] and emit diagnostics
//! directly; neither mutates the model.

use super::types::DeclarationMap;
use crate::ast;
use crate::ast_builder::Diagnostic;

/// Validate tracker references: undefined patterns in songs and channel
/// count consistency across columns. MasterSequencer-side song-name
/// validation is a separate pass below because it requires the full AST
/// to read parameter values.
pub(crate) fn analyse_tracker(decl_map: &DeclarationMap) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Check song blocks: every pattern name referenced must exist
    for song in decl_map.songs.values() {
        for row in &song.rows {
            for cell in row {
                if cell.is_silence {
                    continue;
                }
                if let Some(pattern_name) = &cell.pattern_name {
                    if !decl_map.patterns.contains_key(pattern_name) {
                        diagnostics.push(Diagnostic {
                            span: cell.span,
                            message: format!("undefined pattern '{pattern_name}'"),
                            kind: crate::ast_builder::DiagnosticKind::UndefinedPattern,
                            replacements: Vec::new(),
                        });
                    }
                }
            }
        }

        // Check channel count consistency: patterns in the same column should
        // have the same channel count
        let num_cols = song.channel_names.len();
        for col in 0..num_cols {
            let mut first_count: Option<(usize, &str)> = None;
            for row in &song.rows {
                if col >= row.len() {
                    continue;
                }
                let cell = &row[col];
                if cell.is_silence {
                    continue;
                }
                if let Some(pattern_name) = &cell.pattern_name {
                    if let Some(pat_info) = decl_map.patterns.get(pattern_name) {
                        match first_count {
                            None => first_count = Some((pat_info.channel_count, pattern_name)),
                            Some((expected, _)) if pat_info.channel_count != expected => {
                                diagnostics.push(Diagnostic {
                                    span: cell.span,
                                    message: format!(
                                        "pattern '{}' has {} channels, expected {} in this column",
                                        pattern_name, pat_info.channel_count, expected
                                    ),
                                    kind: crate::ast_builder::DiagnosticKind::ChannelCountMismatch,
                                    replacements: Vec::new(),
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    diagnostics
}

/// Validate MasterSequencer song parameter references against known songs.
/// This needs the full AST to read parameter values.
pub(crate) fn analyse_tracker_modules(
    file: &ast::File,
    decl_map: &DeclarationMap,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let bodies: Vec<(&[ast::Statement], &str)> = {
        let mut v = Vec::new();
        if let Some(patch) = &file.patch {
            v.push((patch.body.as_slice(), ""));
        }
        for t in &file.templates {
            let scope = t.name.as_ref().map_or("", |id| id.name.as_str());
            v.push((t.body.as_slice(), scope));
        }
        v
    };

    for (body, _scope) in bodies {
        for stmt in body {
            if let ast::Statement::Module(m) = stmt {
                let type_name = match &m.type_name {
                    Some(id) => &id.name,
                    None => continue,
                };
                if type_name != "MasterSequencer" {
                    continue;
                }

                // Find the "song" parameter
                for param in &m.params {
                    if let ast::ParamEntry::KeyValue {
                        name: Some(pname),
                        value: Some(value),
                        span,
                        ..
                    } = param
                    {
                        if pname.name != "song" {
                            continue;
                        }
                        let song_name = match value {
                            ast::Value::Scalar(ast::Scalar::Str(s)) => s.as_str(),
                            _ => continue,
                        };
                        if !decl_map.songs.contains_key(song_name) {
                            diagnostics.push(Diagnostic {
                                span: *span,
                                message: format!("undefined song '{song_name}'"),
                                kind: crate::ast_builder::DiagnosticKind::UndefinedSong,
                                replacements: Vec::new(),
                            });
                        }
                        // Channel-alignment between `song` and the MasterSequencer
                        // `channels` shape arg depends on resolved shape; the pest
                        // pipeline reports it post-expansion. Tree-sitter fallback
                        // stays name-level only per ADR 0038 stage 4c.
                    }
                }
            }
        }
    }

    diagnostics
}
