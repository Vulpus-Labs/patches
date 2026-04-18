//! Module-type resolution, shape validation, and parameter validation delegation.

use std::collections::HashMap;

use patches_core::{ModuleDescriptor, ParameterMap, Provenance, QName};
use patches_registry::Registry;
use patches_dsl::flat::FlatModule;

use super::errors::{BindError, BindErrorCode};

/// A [`FlatModule`] paired with its resolved [`ModuleDescriptor`] and
/// validated parameter map.
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    pub id: QName,
    pub type_name: String,
    pub descriptor: ModuleDescriptor,
    pub params: ParameterMap,
    pub port_aliases: Vec<(u32, String)>,
    pub provenance: Provenance,
}

/// A [`FlatModule`] that could not be fully bound against the registry.
///
/// The raw flat fields are preserved so feature handlers (hover, completions)
/// can still offer user-visible diagnostics and partial information against
/// whatever *did* parse. `reason` classifies the first failure encountered
/// on this module; additional failures are recorded in [`super::BoundPatch::errors`].
#[derive(Debug, Clone)]
pub struct UnresolvedModule {
    pub id: QName,
    pub type_name: String,
    pub shape: Vec<(String, patches_dsl::ast::Scalar)>,
    pub params: Vec<(String, patches_dsl::ast::Value)>,
    pub port_aliases: Vec<(u32, String)>,
    pub provenance: Provenance,
    pub reason: BindErrorCode,
}

/// One module in a [`super::BoundPatch`]: either fully resolved against the
/// registry, or retained unresolved so downstream code can still walk the
/// graph.
#[derive(Debug, Clone)]
pub enum BoundModule {
    Resolved(ResolvedModule),
    Unresolved(UnresolvedModule),
}

impl BoundModule {
    pub fn id(&self) -> &QName {
        match self {
            Self::Resolved(m) => &m.id,
            Self::Unresolved(m) => &m.id,
        }
    }

    pub fn type_name(&self) -> &str {
        match self {
            Self::Resolved(m) => &m.type_name,
            Self::Unresolved(m) => &m.type_name,
        }
    }

    pub fn provenance(&self) -> &Provenance {
        match self {
            Self::Resolved(m) => &m.provenance,
            Self::Unresolved(m) => &m.provenance,
        }
    }

    pub fn as_resolved(&self) -> Option<&ResolvedModule> {
        match self {
            Self::Resolved(m) => Some(m),
            Self::Unresolved(_) => None,
        }
    }
}

pub(super) fn bind_module(
    fm: &FlatModule,
    registry: &Registry,
    base_dir: Option<&std::path::Path>,
    song_name_to_index: &HashMap<String, usize>,
    errors: &mut Vec<BindError>,
) -> BoundModule {
    let shape = crate::shape_from_args(&fm.shape);

    let descriptor = match registry.describe(&fm.type_name, &shape) {
        Ok(d) => d,
        Err(e) => {
            // Disambiguate unknown-type vs shape rejection by looking at the
            // error payload. `Registry::describe` returns `BuildError::Custom`
            // (or a specific variant) — we keep the message as-is and pick
            // the narrower code when the type isn't registered.
            let code = if registry.module_names().any(|n| n == fm.type_name) {
                BindErrorCode::InvalidShape
            } else {
                BindErrorCode::UnknownModuleType
            };
            errors.push(BindError::new(code, fm.provenance.clone(), e.to_string()));
            return mark_unresolved(fm, code);
        }
    };

    let params = match crate::convert_params(&fm.params, &descriptor, base_dir, song_name_to_index) {
        Ok(p) => p,
        Err(err) => {
            let code = err.bind_code();
            errors.push(BindError::new(code, fm.provenance.clone(), err.into_message()));
            return mark_unresolved(fm, code);
        }
    };

    if let Err(e) = patches_core::validate_parameters(&params, &descriptor) {
        errors.push(BindError::new(
            BindErrorCode::ParameterConversion,
            fm.provenance.clone(),
            e.to_string(),
        ));
        return mark_unresolved(fm, BindErrorCode::ParameterConversion);
    }

    BoundModule::Resolved(ResolvedModule {
        id: fm.id.clone(),
        type_name: fm.type_name.clone(),
        descriptor,
        params,
        port_aliases: fm.port_aliases.clone(),
        provenance: fm.provenance.clone(),
    })
}

/// Build an `Unresolved` [`BoundModule`] tagged with `code`, preserving the
/// raw flat fields so downstream consumers (hover, completions) can still
/// surface partial information. Extracted from three identical inline
/// blocks in [`bind_module`] — ticket 0445.
fn mark_unresolved(fm: &FlatModule, code: BindErrorCode) -> BoundModule {
    BoundModule::Unresolved(UnresolvedModule {
        id: fm.id.clone(),
        type_name: fm.type_name.clone(),
        shape: fm.shape.clone(),
        params: fm.params.clone(),
        port_aliases: fm.port_aliases.clone(),
        provenance: fm.provenance.clone(),
        reason: code,
    })
}
