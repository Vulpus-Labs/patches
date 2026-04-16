//! Textual backward-scan fallback for incomplete input where tree-sitter has
//! no usable node at the cursor (e.g. `osc.` with cursor just past the dot).

/// Context determined by scanning backward from cursor position.
pub(super) enum BackwardContext {
    ModuleColon,
    ModuleTypeName,
    Dot(String),
    DollarDot,
    /// `module_name.port_name[` — complete with shape aliases
    PortIndex(String),
    /// Inside a song block row (after `|`)
    SongRow,
}

/// Scan backward from the cursor to determine context when tree-sitter nodes
/// don't give enough information (e.g. in incomplete input).
pub(super) fn scan_backward_for_context(source: &str, byte_offset: usize) -> Option<BackwardContext> {
    let before = &source[..byte_offset];
    let trimmed = before.trim_end();

    // Check for `module.port[` — complete with shape aliases for the module.
    if let Some(before_bracket) = trimmed.strip_suffix('[') {
        let before_bracket = before_bracket.trim_end();
        if let Some(dot_pos) = before_bracket.rfind('.') {
            let before_dot = &before_bracket[..dot_pos];
            let module_name = before_dot
                .rsplit(|c: char| c.is_whitespace() || c == '{' || c == '}')
                .next()?;
            if !module_name.is_empty()
                && module_name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                return Some(BackwardContext::PortIndex(module_name.to_string()));
            }
        }
    }

    if trimmed.ends_with("$.") {
        return Some(BackwardContext::DollarDot);
    }

    if let Some(before_dot) = trimmed.strip_suffix('.') {
        let before_dot = before_dot.trim_end();
        let module_name = before_dot
            .rsplit(|c: char| c.is_whitespace() || c == '{' || c == '}')
            .next()?;
        if !module_name.is_empty()
            && module_name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Some(BackwardContext::Dot(module_name.to_string()));
        }
    }

    // Check for `module <name> : |` pattern.
    if let Some(before_colon) = trimmed.strip_suffix(':') {
        let before_colon = before_colon.trim_end();
        let parts: Vec<&str> = before_colon.rsplitn(3, char::is_whitespace).collect();
        if parts.len() >= 2 && (parts[1] == "module" || parts.get(2) == Some(&"module")) {
            return Some(BackwardContext::ModuleColon);
        }
    }

    // Check for `module <name> : <PartialTypeName>` — cursor is mid-type-name.
    let word_start = trimmed
        .rfind(|c: char| c.is_whitespace())
        .map(|i| i + 1)
        .unwrap_or(0);
    let before_word = trimmed[..word_start].trim_end();
    if let Some(before_colon) = before_word.strip_suffix(':') {
        let before_colon = before_colon.trim_end();
        let parts: Vec<&str> = before_colon.rsplitn(3, char::is_whitespace).collect();
        if parts.len() >= 2 && (parts[1] == "module" || parts.get(2) == Some(&"module")) {
            return Some(BackwardContext::ModuleTypeName);
        }
    }

    // Check if inside a song block row: last non-whitespace before cursor ends
    // with `|` or we're after `| ` at start of a song row
    if (trimmed.ends_with('|')
        || (trimmed.rfind('|').is_some() && is_inside_song_block(source, byte_offset)))
        && is_inside_song_block(source, byte_offset)
    {
        return Some(BackwardContext::SongRow);
    }

    None
}

/// Heuristic: check if the cursor is inside a song block by scanning backward
/// for `song <name> {` without a closing `}`.
fn is_inside_song_block(source: &str, byte_offset: usize) -> bool {
    let before = &source[..byte_offset];
    // Find the last `song ` keyword
    if let Some(song_pos) = before.rfind("song ") {
        let after_song = &before[song_pos..];
        let open_braces = after_song.matches('{').count();
        let close_braces = after_song.matches('}').count();
        open_braces > close_braces
    } else {
        false
    }
}
