//! Tags as the use cases take them: strings in tagma's grammar,
//! `(namespace:)key(=value)`, parsed into [`Tag`]s at the edge of every
//! write, and admitted only if they could be queried and the project's
//! schema allows them: a write is refused at the first that is not, with
//! the reason, and, where the project declares tag permits, only if they
//! permit it ([`permitted`]). Only what a write adds is checked; a tag
//! already held, or one being removed, never is.
//!
//! One write gives a scalar target one value at most ([`check_scalars`]),
//! and the value it gives displaces the one held before the retag is sent
//! ([`collapse`]): decide never reads the schema.

use std::collections::BTreeMap;
use std::fmt;

use angzarr_embedded_diagnostic::{Arg, Diagnostic};
use ports::{NamespacePermit, TagPermits};

pub use tasks::Tag;

use crate::schema::{TagSchema, FACET_NAMESPACE, SEMVER};
use crate::semver::Version;

/// The words tagma's query grammar reads as operators, in any case: a tag
/// spelled as one could never be matched.
pub const OPERATOR_WORDS: [&str; 3] = ["and", "or", "not"];

/// Why a tag given to a write was refused.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TagRefusal {
    /// Blank: a tag given is never read as none given.
    Blank,
    /// Not a tag in tagma's grammar: it could never be queried.
    Unparseable { tag: String, detail: String },
    /// `and`, `or` or `not`, which a query always reads as an operator.
    OperatorWord { tag: String },
    /// In the `tagma` namespace (or under it), which is the schema's.
    ReservedNamespace { tag: String, namespace: String },
    /// A value outside its target's declared enum.
    NotInEnum {
        tag: String,
        value: String,
        target: String,
        members: String,
    },
    /// A value that is not a finite number, where a range is declared.
    NotANumber {
        tag: String,
        value: String,
        target: String,
        min: String,
        max: String,
    },
    /// A number outside its target's declared range.
    OutOfRange {
        tag: String,
        value: String,
        target: String,
        min: String,
        max: String,
    },
    /// A value that is not a SemVer 2.0.0 version, where `type=semver` is
    /// declared.
    NotSemver {
        tag: String,
        value: String,
        target: String,
        detail: String,
    },
    /// A tag whose target is retired: plans' `ctxloom:plan`, whose work
    /// plans and their governs links now do.
    Retired { tag: String, target: String },
    /// Not permitted by the project's tag permits: `permit` is what
    /// `taskloom permit` takes to permit it (its target, no value).
    NotPermitted { tag: String, permit: String },
    /// Two values for one scalar target in one write: `first` and
    /// `second`, the first two given, neither more the one meant.
    ScalarConflict {
        target: String,
        first: String,
        second: String,
    },
}

impl fmt::Display for TagRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_embedded_diagnostic::display(self, f)
    }
}

impl Diagnostic for TagRefusal {
    fn code(&self) -> &'static str {
        match self {
            TagRefusal::Blank => "tasks-application-tag-blank",
            TagRefusal::Unparseable { .. } => "tasks-application-tag-unparseable",
            TagRefusal::OperatorWord { .. } => "tasks-application-tag-operator-word",
            TagRefusal::ReservedNamespace { .. } => "tasks-application-tag-reserved-namespace",
            TagRefusal::NotInEnum { .. } => "tasks-application-tag-not-in-enum",
            TagRefusal::NotANumber { .. } => "tasks-application-tag-not-a-number",
            TagRefusal::OutOfRange { .. } => "tasks-application-tag-out-of-range",
            TagRefusal::NotSemver { .. } => "tasks-application-tag-not-semver",
            TagRefusal::Retired { .. } => "tasks-application-tag-retired",
            TagRefusal::NotPermitted { .. } => "tasks-application-tag-not-permitted",
            TagRefusal::ScalarConflict { .. } => "tasks-application-tag-scalar-conflict",
        }
    }

    fn args(&self) -> Vec<(&'static str, Arg)> {
        let s = |v: &String| Arg::from(v.as_str());
        match self {
            TagRefusal::Blank => Vec::new(),
            TagRefusal::Unparseable { tag, detail } => {
                vec![("tag", s(tag)), ("ext_err_txt", s(detail))]
            }
            TagRefusal::OperatorWord { tag } => vec![("tag", s(tag))],
            TagRefusal::ReservedNamespace { tag, namespace } => {
                vec![("tag", s(tag)), ("namespace", s(namespace))]
            }
            TagRefusal::NotInEnum {
                tag,
                value,
                target,
                members,
            } => vec![
                ("tag", s(tag)),
                ("value", s(value)),
                ("target", s(target)),
                ("members", s(members)),
            ],
            TagRefusal::NotANumber {
                tag,
                value,
                target,
                min,
                max,
            }
            | TagRefusal::OutOfRange {
                tag,
                value,
                target,
                min,
                max,
            } => vec![
                ("tag", s(tag)),
                ("value", s(value)),
                ("target", s(target)),
                ("min", s(min)),
                ("max", s(max)),
            ],
            TagRefusal::NotSemver {
                tag,
                value,
                target,
                detail,
            } => vec![
                ("tag", s(tag)),
                ("value", s(value)),
                ("target", s(target)),
                ("ext_err_txt", s(detail)),
            ],
            TagRefusal::NotPermitted { tag, permit } => {
                vec![("tag", s(tag)), ("permit", s(permit))]
            }
            TagRefusal::Retired { tag, target } => vec![("tag", s(tag)), ("target", s(target))],
            TagRefusal::ScalarConflict {
                target,
                first,
                second,
            } => vec![
                ("target", s(target)),
                ("first", s(first)),
                ("second", s(second)),
            ],
        }
    }
}

/// `spelled`, trimmed, parsed as tagma parses a tag; `None` when it is
/// empty once trimmed, which is no tag at all.
pub fn parse(spelled: &str) -> Option<Result<Tag, TagRefusal>> {
    let trimmed = spelled.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        tagma_core::Tag::parse(trimmed)
            .map(Tag::from)
            .map_err(|detail| TagRefusal::Unparseable {
                tag: trimmed.to_string(),
                detail,
            }),
    )
}

/// Every tag `spelled` names, in its order, as a write is given them;
/// refused at the first that does not parse, or is blank: a tag given is
/// never read as none (an import, reading what Go wrote, leaves a blank
/// one out through [`parse`] instead).
pub fn parse_all(spelled: &[String]) -> Result<Vec<Tag>, TagRefusal> {
    spelled
        .iter()
        .map(|s| parse(s).unwrap_or(Err(TagRefusal::Blank)))
        .collect()
}

/// Whether `tag` passes what every task's tag must, whatever the
/// project's schema: it could be queried (not a bare operator word), and
/// it stays out of tagma's reserved namespaces (`tagma`, `tagma.*`), where
/// tagma reads a tag as its own config, whatever item carries it. A write
/// ([`admit`]) and an import both apply it.
pub fn schema_free(tag: &Tag) -> Result<(), TagRefusal> {
    let bare = tag.namespace.is_none() && tag.value.is_none();
    if bare && OPERATOR_WORDS.contains(&tag.key.to_ascii_lowercase().as_str()) {
        return Err(TagRefusal::OperatorWord {
            tag: tag.to_string(),
        });
    }
    let Some(namespace) = &tag.namespace else {
        return Ok(());
    };
    let under = namespace
        .strip_prefix(FACET_NAMESPACE)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'));
    if under {
        return Err(TagRefusal::ReservedNamespace {
            tag: tag.to_string(),
            namespace: namespace.clone(),
        });
    }
    Ok(())
}

/// Whether `tag` may be written under `schema`: it passes [`schema_free`],
/// its target is not retired ([`retired`]), and its value is one its
/// target's declared enum, range and type allow.
pub fn admit(tag: &Tag, schema: &TagSchema) -> Result<(), TagRefusal> {
    let spelled = || tag.to_string();
    schema_free(tag)?;
    retired(tag)?;
    let Some(value) = &tag.value else {
        return Ok(());
    };
    let target = tag.target();
    if let Some(members) = schema.enum_of(&target) {
        if !members.contains(value) {
            return Err(TagRefusal::NotInEnum {
                tag: spelled(),
                value: value.clone(),
                target,
                members: members.join(", "),
            });
        }
    }
    if let Some(range) = schema.range_of(&target) {
        let (min, max) = (range.min.to_string(), range.max.to_string());
        match value.parse::<f64>().ok().filter(|f| f.is_finite()) {
            None => {
                return Err(TagRefusal::NotANumber {
                    tag: spelled(),
                    value: value.clone(),
                    target,
                    min,
                    max,
                })
            }
            Some(n) if n < range.min || n > range.max => {
                return Err(TagRefusal::OutOfRange {
                    tag: spelled(),
                    value: value.clone(),
                    target,
                    min,
                    max,
                })
            }
            Some(_) => {}
        }
    }
    if schema.type_of(&target) == Some(SEMVER) {
        if let Err(why) = Version::parse(value) {
            return Err(TagRefusal::NotSemver {
                tag: spelled(),
                value: value.clone(),
                target,
                detail: why.to_string(),
            });
        }
    }
    Ok(())
}

/// Refused when `tag`'s target is retired ([`plans::tag_is_retired`]): a
/// write never adds one, though a task may still carry one an import kept
/// (`plan import` migrates them).
pub fn retired(tag: &Tag) -> Result<(), TagRefusal> {
    let target = tag.target();
    match plans::tag_is_retired(&target) {
        true => Err(TagRefusal::Retired {
            tag: tag.to_string(),
            target,
        }),
        false => Ok(()),
    }
}

/// Whether the project's tag `permits` permit `tag`: always where it
/// declares none (`None`); else when `schema` declares its target (any
/// facet), its namespace is permitted open, its key is among its closed
/// namespace's keys, or, with no namespace, it is a permitted flat tag. A
/// permit is of a target, never a value. Refused naming the target, the
/// narrowest permit that admits it.
pub fn permitted(
    tag: &Tag,
    permits: Option<&TagPermits>,
    schema: &TagSchema,
) -> Result<(), TagRefusal> {
    let Some(permits) = permits else {
        return Ok(());
    };
    let target = tag.target();
    let admitted = schema.declares(&target)
        || match &tag.namespace {
            None => permits.flat.contains(&tag.key),
            Some(namespace) => match permits.namespaces.get(namespace) {
                Some(NamespacePermit::Open) => true,
                Some(NamespacePermit::Closed(keys)) => keys.contains(&tag.key),
                None => false,
            },
        };
    if admitted {
        return Ok(());
    }
    Err(TagRefusal::NotPermitted {
        tag: tag.to_string(),
        permit: target,
    })
}

/// Every tag `spelled` names, parsed and admitted under `schema`, in its
/// order; refused at the first that is not.
pub fn admit_all(spelled: &[String], schema: &TagSchema) -> Result<Vec<Tag>, TagRefusal> {
    let tags = parse_all(spelled)?;
    for tag in &tags {
        admit(tag, schema)?;
    }
    Ok(tags)
}

/// Whether `add`, the tags one write gives, gives each scalar target of
/// `schema` one value at most (the same value given twice, however spelled,
/// is one); refused at the first target given a second.
pub fn check_scalars(schema: &TagSchema, add: &[Tag]) -> Result<(), TagRefusal> {
    let mut first: BTreeMap<String, &Tag> = BTreeMap::new();
    for tag in add.iter().filter(|tag| tag.namespace.is_some()) {
        let target = tag.target();
        if !schema.is_scalar(&target) {
            continue;
        }
        match first.get(&target) {
            Some(held) if held.value != tag.value => {
                return Err(TagRefusal::ScalarConflict {
                    first: held.to_string(),
                    second: tag.to_string(),
                    target,
                })
            }
            Some(_) => {}
            None => {
                first.insert(target, tag);
            }
        }
    }
    Ok(())
}

/// `add`, with every value of a scalar target but its last dropped, and the
/// tags of `held` that a surviving scalar value displaces: a different
/// value of the same target. A scalar target is always namespaced, so a
/// tag with no namespace is never collapsed. A write refuses two
/// different values first ([`check_scalars`]), so what it collapses is one
/// value given more than once.
pub fn collapse(schema: &TagSchema, held: &[Tag], add: Vec<Tag>) -> (Vec<Tag>, Vec<Tag>) {
    let scalar = |tag: &Tag| {
        let target = tag.target();
        (tag.namespace.is_some() && schema.is_scalar(&target)).then_some(target)
    };
    let mut last: BTreeMap<String, usize> = BTreeMap::new();
    for (i, tag) in add.iter().enumerate() {
        if let Some(target) = scalar(tag) {
            last.insert(target, i);
        }
    }
    let kept: Vec<Tag> = add
        .into_iter()
        .enumerate()
        .filter(|(i, tag)| scalar(tag).is_none_or(|target| last[&target] == *i))
        .map(|(_, tag)| tag)
        .collect();
    let surviving: BTreeMap<String, &Option<String>> = kept
        .iter()
        .filter_map(|tag| Some((scalar(tag)?, &tag.value)))
        .collect();
    let displaced = held
        .iter()
        .filter(|tag| {
            scalar(tag)
                .and_then(|target| surviving.get(&target).copied())
                .is_some_and(|value| *value != tag.value)
        })
        .cloned()
        .collect();
    (kept, displaced)
}

/// One of every refusal, for the catalog test.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn samples() -> Vec<TagRefusal> {
    let x = || "x".to_string();
    vec![
        TagRefusal::Blank,
        TagRefusal::Unparseable {
            tag: x(),
            detail: x(),
        },
        TagRefusal::OperatorWord { tag: x() },
        TagRefusal::ReservedNamespace {
            tag: x(),
            namespace: x(),
        },
        TagRefusal::NotInEnum {
            tag: x(),
            value: x(),
            target: x(),
            members: x(),
        },
        TagRefusal::NotANumber {
            tag: x(),
            value: x(),
            target: x(),
            min: x(),
            max: x(),
        },
        TagRefusal::OutOfRange {
            tag: x(),
            value: x(),
            target: x(),
            min: x(),
            max: x(),
        },
        TagRefusal::NotSemver {
            tag: x(),
            value: x(),
            target: x(),
            detail: x(),
        },
        TagRefusal::Retired {
            tag: x(),
            target: x(),
        },
        TagRefusal::NotPermitted {
            tag: x(),
            permit: x(),
        },
        TagRefusal::ScalarConflict {
            target: x(),
            first: x(),
            second: x(),
        },
    ]
}

#[cfg(test)]
#[path = "tags.test.rs"]
mod tests;
