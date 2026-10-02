//! Closed vocabularies: enumerations whose every value has exactly one spelling.
//!
//! The spelling is written next to the value and is the only one: it is what
//! `as_str` and `Display` give, what `parse` accepts and what serialisation writes and
//! reads. So no default casing of Rust's decides a stored string, and a stored string
//! that is not in the vocabulary is refused rather than guessed at.

/// Declares a closed vocabulary.
macro_rules! vocabulary {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$value_meta:meta])* $value:ident => $spelling:literal, )+
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $( $(#[$value_meta])* $value, )+
        }

        impl $name {
            /// Every value, in the specification's order.
            pub const ALL: &'static [Self] = &[$(Self::$value,)+];

            /// The value's spelling.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$value => $spelling,)+
                }
            }

            /// The value spelled exactly `spelling`, if there is one.
            pub fn parse(spelling: &str) -> Option<Self> {
                match spelling {
                    $($spelling => Some(Self::$value),)+
                    _ => None,
                }
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let spelling = <::std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
                Self::parse(&spelling).ok_or_else(|| {
                    ::serde::de::Error::custom(format_args!(
                        "{:?} is not a {}",
                        spelling,
                        stringify!($name)
                    ))
                })
            }
        }
    };
}

pub(crate) use vocabulary;
