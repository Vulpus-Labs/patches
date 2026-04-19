/// Declare a `#[repr(u32)]` enum whose discriminants are the variant indices
/// used by [`ParameterValue::Enum`](super::parameter_map::ParameterValue::Enum).
///
/// Each variant is paired with the snake_case name used in the
/// [`ParameterKind::Enum`](super::module_descriptor::ParameterKind::Enum)
/// descriptor — the macro does not infer casing, the name is given
/// explicitly.
///
/// Generates:
/// - `#[repr(u32)]` enum with explicit discriminants `0`, `1`, …
/// - `pub const VARIANTS: &'static [&'static str]` in declaration order
/// - `TryFrom<u32>` returning `Result<Self, u32>` (error is the out-of-range
///   value)
/// - `From<Self> for ParameterValue` building `ParameterValue::Enum(idx)`
/// - `IntoParameterValue` impl so the variant can appear directly in the
///   `test_support::params!` macro
///
/// # Example
///
/// ```ignore
/// use patches_core::params_enum;
///
/// params_enum! {
///     pub enum LfoMode {
///         Bipolar => "bipolar",
///         UnipolarPositive => "unipolar_positive",
///         UnipolarNegative => "unipolar_negative",
///     }
/// }
/// ```
#[macro_export]
macro_rules! params_enum {
    (
        $(#[$attr:meta])*
        $vis:vis enum $Name:ident {
            $( $Variant:ident => $name:literal ),+ $(,)?
        }
    ) => {
        $(#[$attr])*
        #[repr(u32)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis enum $Name {
            $( $Variant ),+
        }

        impl $Name {
            pub const VARIANTS: &'static [&'static str] = &[ $( $name ),+ ];

            /// Return the snake_case variant name for this discriminant.
            pub const fn as_str(self) -> &'static str {
                Self::VARIANTS[self as usize]
            }
        }

        impl ::core::convert::TryFrom<u32> for $Name {
            type Error = u32;
            fn try_from(value: u32) -> ::core::result::Result<Self, u32> {
                let arr = [ $( $Name::$Variant ),+ ];
                if (value as usize) < arr.len() {
                    Ok(arr[value as usize])
                } else {
                    Err(value)
                }
            }
        }

        impl ::core::convert::From<$Name> for $crate::modules::ParameterValue {
            fn from(v: $Name) -> Self {
                $crate::modules::ParameterValue::Enum(v as u32)
            }
        }

    };
}

#[cfg(test)]
mod tests {
    use super::super::ParameterValue;

    params_enum! {
        pub enum Demo {
            Alpha => "alpha",
            Beta => "beta",
            Gamma => "gamma",
        }
    }

    #[test]
    fn discriminants_start_at_zero_and_increase() {
        assert_eq!(Demo::Alpha as u32, 0);
        assert_eq!(Demo::Beta as u32, 1);
        assert_eq!(Demo::Gamma as u32, 2);
    }

    #[test]
    fn variants_list_matches_declaration_order() {
        assert_eq!(Demo::VARIANTS, &["alpha", "beta", "gamma"]);
    }

    #[test]
    fn try_from_round_trip() {
        for (i, v) in [Demo::Alpha, Demo::Beta, Demo::Gamma].iter().enumerate() {
            assert_eq!(Demo::try_from(i as u32).unwrap(), *v);
        }
    }

    #[test]
    fn try_from_out_of_range_returns_err_with_value() {
        assert_eq!(Demo::try_from(3), Err(3));
        assert_eq!(Demo::try_from(99), Err(99));
    }

    #[test]
    fn into_parameter_value() {
        let v: ParameterValue = Demo::Beta.into();
        assert_eq!(v, ParameterValue::Enum(1));
    }

    #[test]
    fn match_is_exhaustive() {
        // Compile-time check: exhaustive match with no wildcard.
        fn label(d: Demo) -> &'static str {
            match d {
                Demo::Alpha => "a",
                Demo::Beta => "b",
                Demo::Gamma => "g",
            }
        }
        assert_eq!(label(Demo::Alpha), "a");
    }
}
