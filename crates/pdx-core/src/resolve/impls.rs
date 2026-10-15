//! A Rust trait impl's header, as Stage 3 reads it (issue 117).
//!
//! Extraction records each trait impl's header (issue 109): what must hold for the impl
//! to apply to a type, as entries of a one-letter kind, `:` and a text, in the grammar
//! `engine/src/pdxe_core.h` gives above `PDXEImplTrait`. The typed pass proves an impl
//! applies before it answers with the impl's method; this module reads the same entries,
//! with the same meaning, so that a trusted stage of Stage 3 never takes a trait impl's
//! member for a call's target on less evidence than the typed pass would.
//!
//! An impl is proven to apply here in two ways only, and anything else is unknown, which
//! is never proven:
//!
//! - it requires nothing: its self type is a path whose type arguments are distinct
//!   parameters of the impl (or one parameter itself), and every bound is relaxed
//!   (`?Sized`) or a lifetime's, which decides no impl;
//! - the impl the call is made from (for a call on `self` or `Self`, over the same type)
//!   requires what it requires: every trait bound, its parameter renamed by the self
//!   types' positions, is one of the calling impl's own predicates, word for word. The
//!   calling impl's predicates hold wherever its methods run.
//!
//! No bound is ever shown false, for the reasons issue 109 gives: an unproven impl is
//! unknown, never rejected.

use std::collections::BTreeMap;

/// The self type's form (`s:`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfForm {
    /// A path whose type arguments are distinct parameters of the impl, or none.
    Plain,
    /// One parameter of the impl itself.
    Blanket,
    /// Anything else: concrete arguments, a reference, a tuple, ...
    Other,
}

/// One bound of a predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    /// A trait the subject must implement.
    Trait {
        /// The trait's path as written.
        path: String,
        /// Its generic arguments as written, in order.
        args: Vec<String>,
    },
    /// `?Sized`: no requirement.
    Relaxed,
    /// An outlives requirement: selection erases lifetimes, so it decides no impl.
    Lifetime,
    /// Higher-ranked, `Fn(..)` sugar, parenthesised: never proven.
    Other,
}

/// One predicate: a subject and its bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Predicate {
    /// The subject as written (a parameter, a projection, a path, a lifetime).
    pub subject: String,
    /// The parameter it bounds, an index of [`ImplHeader::params`]; `None` for any
    /// other subject.
    pub param: Option<usize>,
    /// Its bounds, in order.
    pub bounds: Vec<Bound>,
}

/// A trait impl's header, read from the engine's entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplHeader {
    /// The self type's form.
    pub form: SelfForm,
    /// An entry no grammar allows, or the engine's `x:`: never proven.
    pub opaque: bool,
    /// The type and const parameters, in order.
    pub params: Vec<String>,
    /// [`SelfForm::Plain`]: the self type's type arguments, as indexes of `params`.
    pub self_args: Vec<usize>,
    /// The trait's generic arguments, in order.
    pub trait_args: Vec<String>,
    /// The predicates, from the parameters' bounds and the where clause.
    pub predicates: Vec<Predicate>,
}

impl ImplHeader {
    /// The header the engine's entries write; `None` when it recorded none, which
    /// proves nothing.
    pub fn read(entries: &[String]) -> Option<Self> {
        let mut header = ImplHeader {
            form: SelfForm::Other,
            opaque: false,
            params: Vec::new(),
            self_args: Vec::new(),
            trait_args: Vec::new(),
            predicates: Vec::new(),
        };
        if entries.is_empty() {
            return None;
        }
        for (i, entry) in entries.iter().enumerate() {
            let Some((kind, text)) = entry.split_once(':') else {
                header.opaque = true;
                continue;
            };
            match kind {
                "s" if i == 0 => {
                    header.form = match text {
                        "plain" => SelfForm::Plain,
                        "blanket" => SelfForm::Blanket,
                        "other" => SelfForm::Other,
                        _ => {
                            header.opaque = true;
                            SelfForm::Other
                        }
                    };
                }
                "p" => header.params.push(text.to_owned()),
                "a" => match header.params.iter().position(|p| p == text) {
                    Some(index) => header.self_args.push(index),
                    None => header.opaque = true,
                },
                "t" => header.trait_args.push(text.to_owned()),
                "w" => {
                    let param = header.params.iter().position(|p| p == text);
                    header.predicates.push(Predicate {
                        subject: text.to_owned(),
                        param,
                        bounds: Vec::new(),
                    });
                }
                "b" | "r" | "l" | "o" => {
                    let Some(predicate) = header.predicates.last_mut() else {
                        header.opaque = true;
                        continue;
                    };
                    predicate.bounds.push(match kind {
                        "b" => Bound::Trait {
                            path: text.to_owned(),
                            args: Vec::new(),
                        },
                        "r" => Bound::Relaxed,
                        "l" => Bound::Lifetime,
                        _ => Bound::Other,
                    });
                }
                "g" => match header
                    .predicates
                    .last_mut()
                    .and_then(|p| p.bounds.last_mut())
                {
                    Some(Bound::Trait { args, .. }) => args.push(text.to_owned()),
                    _ => header.opaque = true,
                },
                // `x:`, an `s:` that is not first, or a kind no grammar has.
                _ => header.opaque = true,
            }
        }
        Some(header)
    }

    /// Whether the header writes the impl's self type whole: a path of its parameters
    /// (with the type's own name) or one parameter. An `s:other` self type's arguments
    /// are not recorded, so `impl Tr for Cell<u8>` and `impl Tr for Cell<u16>` have one
    /// header.
    pub fn writes_self_type(&self) -> bool {
        !self.opaque && self.form != SelfForm::Other
    }

    /// Whether the impl applies to every instance of its self type: nothing it requires
    /// is unknown.
    pub fn requires_nothing(&self) -> bool {
        self.writes_self_type()
            && self.predicates.iter().all(|p| {
                p.bounds
                    .iter()
                    .all(|b| matches!(b, Bound::Relaxed | Bound::Lifetime))
            })
    }

    /// Whether `caller`, the header of the impl a call on `self` or `Self` is made from,
    /// requires everything this impl requires, both over the same type: the self types'
    /// parameters correspond position for position, and every trait bound, renamed, is
    /// one of `caller`'s predicates word for word. A predicate on a parameter the self
    /// type does not hold, on any other subject, or with another kind of bound is
    /// unknown.
    pub fn entailed_by(&self, caller: &ImplHeader) -> bool {
        if self.opaque
            || caller.opaque
            || self.form != SelfForm::Plain
            || caller.form != SelfForm::Plain
            || self.self_args.len() != caller.self_args.len()
        {
            return false;
        }
        let rename: BTreeMap<&str, &str> = self
            .self_args
            .iter()
            .zip(&caller.self_args)
            .map(|(&ours, &theirs)| (self.params[ours].as_str(), caller.params[theirs].as_str()))
            .collect();
        self.predicates.iter().all(|predicate| {
            predicate.bounds.iter().all(|bound| match bound {
                Bound::Relaxed | Bound::Lifetime => true,
                Bound::Other => false,
                Bound::Trait { path, args } => {
                    let Some(subject) = predicate
                        .param
                        .and_then(|p| rename.get(self.params[p].as_str()))
                    else {
                        return false;
                    };
                    let Some(args) = args
                        .iter()
                        .map(|a| self.renamed(a, &rename))
                        .collect::<Option<Vec<String>>>()
                    else {
                        return false;
                    };
                    caller.predicates.iter().any(|theirs| {
                        theirs.subject == *subject
                            && theirs.bounds.iter().any(|b| {
                                matches!(b, Bound::Trait { path: p, args: a } if p == path && *a == args)
                            })
                    })
                }
            })
        })
    }

    /// `text` with this impl's parameters renamed; `None` when it names one `rename`
    /// does not map.
    fn renamed(&self, text: &str, rename: &BTreeMap<&str, &str>) -> Option<String> {
        let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find(word) {
            out.push_str(&rest[..start]);
            let after = &rest[start..];
            let end = after.find(|c: char| !word(c)).unwrap_or(after.len());
            let token = &after[..end];
            if self.params.iter().any(|p| p == token) {
                out.push_str(rename.get(token)?);
            } else {
                out.push_str(token);
            }
            rest = &after[end..];
        }
        out.push_str(rest);
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(entries: &[&str]) -> ImplHeader {
        let entries: Vec<String> = entries.iter().map(|e| (*e).to_owned()).collect();
        ImplHeader::read(&entries).expect("a header")
    }

    #[test]
    fn nothing_recorded_is_no_header() {
        assert_eq!(ImplHeader::read(&[]), None);
    }

    #[test]
    fn an_impl_requires_nothing_only_with_plain_self_and_no_trait_bound() {
        assert!(header(&["s:plain"]).requires_nothing());
        assert!(
            header(&["s:plain", "p:T", "a:T", "w:T", "r:?Sized", "l:'static"]).requires_nothing()
        );
        assert!(header(&["s:blanket", "p:T"]).requires_nothing());
        assert!(!header(&["s:plain", "p:T", "a:T", "w:T", "b:Clone"]).requires_nothing());
        assert!(
            !header(&["s:plain", "p:F", "a:F", "w:F", "o:for<'a>Fn(&'a u8)"]).requires_nothing()
        );
        assert!(!header(&["s:other"]).requires_nothing());
        assert!(!header(&["s:plain", "x:header"]).requires_nothing());
        // A bound before any predicate, or an argument before any trait bound: opaque.
        assert!(!header(&["s:plain", "b:Clone"]).requires_nothing());
        assert!(!header(&["s:plain", "p:T", "a:T", "w:T", "r:?Sized", "g:u8"]).requires_nothing());
    }

    #[test]
    fn only_a_plain_or_blanket_self_type_is_written_whole() {
        assert!(header(&["s:plain", "p:T", "a:T", "w:T", "b:Clone"]).writes_self_type());
        assert!(header(&["s:blanket", "p:T"]).writes_self_type());
        assert!(!header(&["s:other"]).writes_self_type());
        assert!(!header(&["s:plain", "x:header"]).writes_self_type());
    }

    #[test]
    fn a_callers_predicates_entail_the_same_bounds_renamed() {
        // `impl<H, S> Clone for M<H, S> where H: Clone`, called from
        // `impl<A, B> Tr<B> for M<A, B> where A: Clone + Send, B: 'static`.
        let target = header(&["s:plain", "p:H", "p:S", "a:H", "a:S", "w:H", "b:Clone"]);
        let caller = header(&[
            "s:plain",
            "p:A",
            "p:B",
            "a:A",
            "a:B",
            "t:B",
            "w:A",
            "b:Clone",
            "b:Send",
            "w:B",
            "l:'static",
        ]);
        assert!(target.entailed_by(&caller));
        // The bound on the other parameter is not entailed.
        let wrong = header(&["s:plain", "p:A", "p:B", "a:A", "a:B", "w:B", "b:Clone"]);
        assert!(!target.entailed_by(&wrong));
        // Arguments renamed too: `X: From<Y>` against `A: From<B>`.
        let from = header(&[
            "s:plain", "p:X", "p:Y", "a:X", "a:Y", "w:X", "b:From", "g:Y",
        ]);
        let has = header(&[
            "s:plain", "p:A", "p:B", "a:A", "a:B", "w:A", "b:From", "g:B",
        ]);
        let other = header(&[
            "s:plain", "p:A", "p:B", "a:A", "a:B", "w:A", "b:From", "g:A",
        ]);
        assert!(from.entailed_by(&has));
        assert!(!from.entailed_by(&other));
    }

    #[test]
    fn what_no_position_or_word_maps_is_unknown() {
        // A parameter the self type does not hold (the trait's `S`).
        let target = header(&["s:plain", "p:E", "p:S", "a:E", "t:S", "w:S", "b:Send"]);
        let caller = header(&["s:plain", "p:E", "p:S", "a:E", "t:S", "w:S", "b:Send"]);
        assert!(!target.entailed_by(&caller));
        // A projection, another bound, another arity, another form, an opaque caller.
        let projection = header(&["s:plain", "p:T", "a:T", "w:T::Item", "b:Clone"]);
        let any = header(&["s:plain", "p:T", "a:T", "w:T::Item", "b:Clone"]);
        assert!(!projection.entailed_by(&any));
        let hrtb = header(&["s:plain", "p:T", "a:T", "w:T", "o:for<'a>Fn(&'a u8)"]);
        assert!(!hrtb.entailed_by(&hrtb.clone()));
        let one = header(&["s:plain", "p:T", "a:T", "w:T", "b:Clone"]);
        let two = header(&["s:plain", "p:T", "p:U", "a:T", "a:U", "w:T", "b:Clone"]);
        assert!(!one.entailed_by(&two));
        let other = header(&["s:other", "p:T", "w:T", "b:Clone"]);
        assert!(!other.entailed_by(&other.clone()));
        let opaque = header(&["s:plain", "p:T", "a:T", "w:T", "b:Clone", "x:header"]);
        assert!(!one.entailed_by(&opaque));
        // A word that only contains a parameter's name is no parameter.
        let words = header(&["s:plain", "p:T", "a:T", "w:T", "b:Tr", "g:Tx"]);
        let caller = header(&["s:plain", "p:U", "a:U", "w:U", "b:Tr", "g:Tx"]);
        assert!(words.entailed_by(&caller));
    }
}
