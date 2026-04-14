use std::fmt;

use crate::provenance::Provenance;

/// Errors produced by module construction and parameter validation.
///
/// Carries an optional [`Provenance`] origin pointing back at the DSL source
/// (FlatModule / FlatConnection) that triggered the failure. Module
/// implementations have no provenance handle, so they construct errors with
/// `origin: None`; the caller (typically `patches-interpreter`) attaches the
/// provenance via [`BuildError::with_origin`].
#[derive(Debug)]
pub enum BuildError {
    UnknownModule {
        name: String,
        origin: Option<Provenance>,
    },

    InvalidShape {
        module: &'static str,
        reason: String,
        origin: Option<Provenance>,
    },

    MissingParameter {
        module: &'static str,
        parameter: &'static str,
        origin: Option<Provenance>,
    },

    InvalidParameterType {
        module: &'static str,
        parameter: &'static str,
        expected: &'static str,
        found: &'static str,
        origin: Option<Provenance>,
    },

    ParameterOutOfRange {
        module: &'static str,
        parameter: &'static str,
        min: f32,
        max: f32,
        found: f32,
        origin: Option<Provenance>,
    },

    Custom {
        module: &'static str,
        message: String,
        origin: Option<Provenance>,
    },
}

impl BuildError {
    /// Borrow the origin provenance, if any.
    pub fn origin(&self) -> Option<&Provenance> {
        match self {
            BuildError::UnknownModule { origin, .. }
            | BuildError::InvalidShape { origin, .. }
            | BuildError::MissingParameter { origin, .. }
            | BuildError::InvalidParameterType { origin, .. }
            | BuildError::ParameterOutOfRange { origin, .. }
            | BuildError::Custom { origin, .. } => origin.as_ref(),
        }
    }

    /// Set the origin provenance, returning the error for chaining.
    pub fn with_origin(mut self, provenance: Provenance) -> Self {
        match &mut self {
            BuildError::UnknownModule { origin, .. }
            | BuildError::InvalidShape { origin, .. }
            | BuildError::MissingParameter { origin, .. }
            | BuildError::InvalidParameterType { origin, .. }
            | BuildError::ParameterOutOfRange { origin, .. }
            | BuildError::Custom { origin, .. } => *origin = Some(provenance),
        }
        self
    }
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Display omits the provenance chain; rendering is the responsibility
        // of the caller (see ticket 0414).
        match self {
            BuildError::UnknownModule { name, .. } =>
                write!(f, "unknown module '{name}'"),

            BuildError::InvalidShape { module, reason, .. } =>
                write!(f, "invalid shape for module '{module}': {reason}"),

            BuildError::MissingParameter { module, parameter, .. } =>
                write!(f, "module '{module}' missing parameter '{parameter}'"),

            BuildError::InvalidParameterType {
                module, parameter, expected, found, ..
            } =>
                write!(
                    f,
                    "module '{module}' parameter '{parameter}' expected {expected}, found {found}"
                ),

            BuildError::ParameterOutOfRange {
                module, parameter, min, max, found, ..
            } =>
                write!(
                    f,
                    "module '{module}' parameter '{parameter}' out of range [{min}, {max}], found {found}"
                ),

            BuildError::Custom { module, message, .. } =>
                write!(f, "module '{module}': {message}"),
        }
    }
}
