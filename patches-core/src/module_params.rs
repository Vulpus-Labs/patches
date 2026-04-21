//! The `module_params!` macro (ADR 0046).
//!
//! Single declaration site per module emitting typed name consts. Expected
//! usage:
//!
//! ```ignore
//! module_params! {
//!     Delay {
//!         dry_wet:  Float,
//!         delay_ms: IntArray,
//!         gain:     FloatArray,
//!         mode:     Enum<FilterMode>,
//!     }
//! }
//! ```
//!
//! Emits a sibling `pub mod params` holding typed `*ParamName` /
//! `*ParamArray` consts. The module name in the macro header names the
//! *owner* (for documentation only); the emitted `params` module is
//! addressed as `params::DRY_WET`, etc.
//!
//! The macro does **not** own `describe`. Channel count comes from
//! `ModuleShape` at instance build time; hand-written `describe(shape)`
//! stays.

#[macro_export]
macro_rules! module_params {
    (
        $Owner:ident {
            $( $field:ident : $kind:tt $(< $ety:ty >)? ),+ $(,)?
        }
    ) => {
        pub mod params {
            #![allow(non_upper_case_globals, unused_imports)]
            use super::*;
            use $crate::params::*;

            $(
                $crate::__module_params_one!($field : $kind $(<$ety>)? );
            )+
        }
    };
}

// Internal dispatch: map each kind token to the typed-name const.
// Scalar uppercase const name is derived via paste? Avoid extra deps: use
// stringify + a helper? Simplest: user-provided field name is already
// lowercase snake_case; we emit the const as the UPPER_SNAKE form. Instead
// of computing case, we emit the const with the *same* identifier (lower
// case, which is valid for a const but triggers non_upper_case_globals —
// silenced above).
#[macro_export]
#[doc(hidden)]
macro_rules! __module_params_one {
    ($field:ident : Float) => {
        pub const $field: FloatParamName = FloatParamName::new(stringify!($field));
    };
    ($field:ident : FloatArray) => {
        pub const $field: FloatParamArray = FloatParamArray::new(stringify!($field));
    };
    ($field:ident : Int) => {
        pub const $field: IntParamName = IntParamName::new(stringify!($field));
    };
    ($field:ident : IntArray) => {
        pub const $field: IntParamArray = IntParamArray::new(stringify!($field));
    };
    ($field:ident : Bool) => {
        pub const $field: BoolParamName = BoolParamName::new(stringify!($field));
    };
    ($field:ident : BoolArray) => {
        pub const $field: BoolParamArray = BoolParamArray::new(stringify!($field));
    };
    ($field:ident : Buffer) => {
        pub const $field: BufferParamName = BufferParamName::new(stringify!($field));
    };
    ($field:ident : SongName) => {
        pub const $field: SongNameParamName = SongNameParamName::new(stringify!($field));
    };
    ($field:ident : BufferArray) => {
        pub const $field: BufferParamArray = BufferParamArray::new(stringify!($field));
    };
    ($field:ident : Enum<$E:ty>) => {
        pub const $field: EnumParamName<$E> = EnumParamName::<$E>::new(stringify!($field));
    };
    ($field:ident : EnumArray<$E:ty>) => {
        pub const $field: EnumParamArray<$E> = EnumParamArray::<$E>::new(stringify!($field));
    };
}

#[cfg(test)]
mod tests {
    use crate as patches_core;

    patches_core::params_enum! {
        pub enum Mode {
            A => "a",
            B => "b",
        }
    }

    patches_core::module_params! {
        Demo {
            dry_wet: Float,
            gain:    FloatArray,
            steps:   IntArray,
            active:  Bool,
            ir:      Buffer,
            shape:   Enum<Mode>,
        }
    }

    #[test]
    fn consts_carry_names() {
        assert_eq!(params::dry_wet.as_str(), "dry_wet");
        assert_eq!(params::gain.as_str(), "gain");
        assert_eq!(params::steps.as_str(), "steps");
        assert_eq!(params::active.as_str(), "active");
        assert_eq!(params::ir.as_str(), "ir");
        assert_eq!(params::shape.as_str(), "shape");
    }

    #[test]
    fn array_at_produces_indexed_key() {
        let k = params::gain.at(3);
        assert_eq!(k.name, "gain");
        assert_eq!(k.index, 3);
    }
}
