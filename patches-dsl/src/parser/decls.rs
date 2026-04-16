//! Pest-tree walkers for declaration-level nodes: file, patch, template,
//! module_decl, param_decl, port_group_decl, section_def, and the
//! include directive.

use pest::iterators::Pair;

use crate::ast::{
    File, IncludeDirective, IncludeFile, ModuleDecl, ParamDecl, ParamEntry, ParamType, Patch,
    PatternDef, PortGroupDecl, SectionDef, SongDef, Span, Template,
};

use super::error::ParseError;
use super::expressions::{
    build_ident, build_param_entry, build_param_ref_name, build_scalar, build_shape_arg,
    build_statements,
};
use super::steps_songs::{build_pattern_block, build_song_block};
use super::{current_source, span_of, Rule};

pub(super) fn build_include_directive(pair: Pair<'_, Rule>) -> IncludeDirective {
    let span = span_of(&pair);
    let string_pair = pair.into_inner().next().unwrap(); // grammar: include_directive = { "include" ~ string_lit }
    let raw = string_pair.as_str();
    let path = raw[1..raw.len() - 1].to_owned(); // strip surrounding quotes
    IncludeDirective { path, span }
}

/// Shared state accumulated while building either a [`File`] or [`IncludeFile`].
struct FileItems {
    includes: Vec<IncludeDirective>,
    templates: Vec<Template>,
    patterns: Vec<PatternDef>,
    songs: Vec<SongDef>,
    sections: Vec<SectionDef>,
    patch: Option<Patch>,
    span: Span,
}

/// Walk the inner pairs of a file or include_file rule and collect all items.
fn build_file_items(pair: Pair<'_, Rule>) -> Result<FileItems, ParseError> {
    let span = span_of(&pair);
    let mut items = FileItems {
        includes: Vec::new(),
        templates: Vec::new(),
        patterns: Vec::new(),
        songs: Vec::new(),
        sections: Vec::new(),
        patch: None,
        span,
    };

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::include_directive => items.includes.push(build_include_directive(inner)),
            Rule::template => items.templates.push(build_template(inner)?),
            Rule::pattern_block => items.patterns.push(build_pattern_block(inner)?),
            Rule::section_def => items.sections.push(build_section_def(inner)?),
            Rule::song_block => items.songs.push(build_song_block(inner)?),
            Rule::patch => items.patch = Some(build_patch(inner)?),
            Rule::EOI => {}
            _ => unreachable!("unexpected rule: {:?}", inner.as_rule()),
        }
    }

    Ok(items)
}

pub(super) fn build_file(pair: Pair<'_, Rule>) -> Result<File, ParseError> {
    let items = build_file_items(pair)?;
    Ok(File {
        includes: items.includes,
        templates: items.templates,
        patterns: items.patterns,
        songs: items.songs,
        sections: items.sections,
        patch: items.patch.unwrap(), // grammar: file = SOI ~ ... ~ patch ~ EOI
        span: items.span,
    })
}

pub(super) fn build_include_file(pair: Pair<'_, Rule>) -> Result<IncludeFile, ParseError> {
    let items = build_file_items(pair)?;
    Ok(IncludeFile {
        includes: items.includes,
        templates: items.templates,
        patterns: items.patterns,
        songs: items.songs,
        sections: items.sections,
        span: items.span,
    })
}

pub(super) fn build_module_decl(pair: Pair<'_, Rule>) -> Result<ModuleDecl, ParseError> {
    // pair.as_rule() == Rule::module_decl
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());
    let type_name = build_ident(it.next().unwrap());
    // Narrow span to `name : type_name` — tight enough that diagnostics like
    // BN0001 UnknownModuleType land on the offending tokens rather than the
    // whole declaration (which pest widens across trailing whitespace when
    // optional shape/param blocks are absent).
    let span = Span::new(current_source(), name.span.start, type_name.span.end);
    let mut shape = Vec::new();
    let mut params = Vec::new();

    for next in it {
        match next.as_rule() {
            Rule::shape_block => {
                shape = next.into_inner().map(build_shape_arg).collect::<Result<_, _>>()?;
            }
            Rule::param_block => {
                // Each item is either a param_ref (shorthand) or param_entry (key: value).
                params = next
                    .into_inner()
                    .map(|item| match item.as_rule() {
                        Rule::param_ref => {
                            Ok(ParamEntry::Shorthand(build_param_ref_name(item)))
                        }
                        Rule::param_entry => build_param_entry(item),
                        _ => unreachable!(
                            "unexpected rule in param_block: {:?}",
                            item.as_rule()
                        ),
                    })
                    .collect::<Result<_, _>>()?;
            }
            _ => unreachable!("unexpected rule in module_decl: {:?}", next.as_rule()),
        }
    }

    Ok(ModuleDecl { name, type_name, shape, params, span })
}

fn build_param_decl(pair: Pair<'_, Rule>) -> Result<ParamDecl, ParseError> {
    // pair.as_rule() == Rule::param_decl
    // Grammar: param_decl = { ident ~ ("[" ~ ident ~ "]")? ~ ":" ~ type_name ~ ("=" ~ scalar)? }
    let span = span_of(&pair);
    let mut it = pair.into_inner();

    let name = build_ident(it.next().unwrap());

    // Next pair is either an ident (arity annotation), type_name, or scalar.
    // We need to distinguish: if it's an ident AND followed by type_name, it's the arity ident.
    // Collect remaining pairs to inspect them positionally.
    let remaining: Vec<_> = it.collect();
    let mut pos = 0;

    // Check if remaining[0] is an ident (arity) or type_name
    let arity = if remaining.len() > pos
        && matches!(remaining[pos].as_rule(), Rule::ident)
    {
        let arity_name = remaining[pos].as_str().to_owned();
        pos += 1;
        Some(arity_name)
    } else {
        None
    };

    // type_name
    let ty = match remaining[pos].as_str() {
        "float" => ParamType::Float,
        "int" => ParamType::Int,
        "bool" => ParamType::Bool,
        "str" => ParamType::Str,
        "pattern" => ParamType::Pattern,
        "song" => ParamType::Song,
        other => unreachable!("unexpected type_name: {other}"),
    };
    pos += 1;

    // optional default scalar
    let default = if pos < remaining.len() {
        Some(build_scalar(remaining[pos].clone())?)
    } else {
        None
    };

    Ok(ParamDecl { name, arity, ty, default, span })
}

/// Build a [`PortGroupDecl`] from a `port_group_decl` pair.
///
/// Grammar: `port_group_decl = { ident ~ ("[" ~ ident ~ "]")? }`
fn build_port_group_decl(pair: Pair<'_, Rule>) -> PortGroupDecl {
    let span = span_of(&pair);
    let mut it = pair.into_inner();

    let name = build_ident(it.next().unwrap());

    // Optional arity ident
    let arity = it.next().map(|arity_pair| arity_pair.as_str().to_owned());

    PortGroupDecl { name, arity, span }
}

pub(super) fn build_template(pair: Pair<'_, Rule>) -> Result<Template, ParseError> {
    // pair.as_rule() == Rule::template
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());

    let mut params = Vec::new();
    let mut in_ports = Vec::new();
    let mut out_ports = Vec::new();
    let mut body = Vec::new();

    for next in it {
        match next.as_rule() {
            Rule::param_decls => {
                params = next.into_inner().map(build_param_decl).collect::<Result<_, _>>()?;
            }
            Rule::port_decls => {
                // in_decl = { "in:" ~ comma_port_decls }  (optional)
                // out_decl = { "out:" ~ comma_port_decls }
                for decl in next.into_inner() {
                    match decl.as_rule() {
                        Rule::in_decl => {
                            let ci = decl.into_inner().next().unwrap();
                            in_ports = ci.into_inner().map(build_port_group_decl).collect();
                        }
                        Rule::out_decl => {
                            let ci = decl.into_inner().next().unwrap();
                            out_ports = ci.into_inner().map(build_port_group_decl).collect();
                        }
                        _ => unreachable!("unexpected rule in port_decls: {:?}", decl.as_rule()),
                    }
                }
            }
            Rule::statement => body.extend(build_statements(next)?),
            _ => unreachable!("unexpected rule in template: {:?}", next.as_rule()),
        }
    }

    Ok(Template { name, params, in_ports, out_ports, body, span })
}

pub(super) fn build_patch(pair: Pair<'_, Rule>) -> Result<Patch, ParseError> {
    // pair.as_rule() == Rule::patch
    let span = span_of(&pair);
    let body = pair
        .into_inner()
        .map(build_statements)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    Ok(Patch { body, span })
}

pub(super) fn build_section_def(pair: Pair<'_, Rule>) -> Result<SectionDef, ParseError> {
    // section_def = { "section" ~ ident ~ "{" ~ row_seq ~ "}" }
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());
    let body = super::steps_songs::build_row_seq(it.next().unwrap())?;
    Ok(SectionDef { name, body, span })
}
