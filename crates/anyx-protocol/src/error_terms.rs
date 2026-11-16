//! Reads an error list into its terms: own errors in brackets, after a
//! group's name in an `ERRORS` group, then the groups it joins with `+`.
//! Joined terms make one call, `_ + _`, with plain group names.

use anyx::{NamePart, Span};

use crate::grammar as ast;

/// An error list, read.
pub(crate) struct ErrorTerms<'a> {
    /// The group's name, written before its own errors:
    /// `file errors [trashed, missing]`.
    pub(crate) name: Option<ast::Spanned<String>>,
    /// The errors defined in the brackets.
    pub(crate) own: &'a [ast::Error],
    /// The groups joined with `+`, in order.
    pub(crate) groups: Vec<GroupReference>,
}

/// A group joined with `+`, by its plain name.
#[derive(Clone)]
pub(crate) struct GroupReference {
    pub(crate) name: String,
    pub(crate) span: Span,
}

/// What a list holds that it can't.
pub(crate) enum Misplaced {
    /// Something that is neither errors in brackets nor a group.
    NotATerm(Span),
    /// Errors in brackets after a `+`.
    OwnAfterGroup(Span),
}

pub(crate) fn read(list: &ast::ErrorTerm) -> Result<ErrorTerms<'_>, Misplaced> {
    let mut terms = ErrorTerms {
        name: None,
        own: &[],
        groups: Vec::new(),
    };
    for (index, term) in joined_terms(list)?.into_iter().enumerate() {
        match own_errors(term) {
            Some((name, own)) if index == 0 => {
                terms.name = name;
                terms.own = own;
            }
            Some(_) => return Err(Misplaced::OwnAfterGroup(span(term))),
            None => terms.groups.push(group(term)?),
        }
    }
    Ok(terms)
}

/// The terms joined by `+`; path-qualified group names are rejected.
fn joined_terms(list: &ast::ErrorTerm) -> Result<Vec<&ast::ErrorTerm>, Misplaced> {
    let ast::ErrorTerm::Call {
        name, arguments, ..
    } = list
    else {
        return Ok(vec![list]);
    };
    let symbols: Vec<&str> = name
        .parts
        .iter()
        .filter_map(|part| match part {
            NamePart::Word { text, .. } if !text.starts_with(char::is_alphanumeric) => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect();
    if symbols.is_empty() {
        return Ok(vec![list]);
    }
    let slots_between_symbols = name.parts.len() + 1 == 2 * arguments.len()
        && name
            .parts
            .iter()
            .enumerate()
            .all(|(index, part)| match part {
                NamePart::Slot => index % 2 == 0,
                NamePart::Word { text, .. } => index % 2 == 1 && text == "+",
            });
    if !slots_between_symbols {
        return Err(Misplaced::NotATerm(span(list)));
    }
    Ok(arguments.iter().collect())
}

/// The errors in brackets, after the group's name if it has one, when
/// `term` is that.
fn own_errors(term: &ast::ErrorTerm) -> Option<(Option<ast::Spanned<String>>, &[ast::Error])> {
    match term {
        ast::ErrorTerm::Own(errors) => Some((None, &errors.value)),
        ast::ErrorTerm::Call {
            name, arguments, ..
        } => match (name.parts.split_last(), arguments.as_slice()) {
            (Some((NamePart::Slot, words)), [ast::ErrorTerm::Own(errors)])
                if !words.is_empty() && !words.contains(&NamePart::Slot) =>
            {
                let name = ast::Spanned {
                    value: name.words().collect::<Vec<_>>().join(" "),
                    span: name.span,
                };
                Some((Some(name), &errors.value))
            }
            _ => None,
        },
    }
}

/// A group by its plain name.
fn group(term: &ast::ErrorTerm) -> Result<GroupReference, Misplaced> {
    match term {
        ast::ErrorTerm::Call {
            name, arguments, ..
        } if arguments.is_empty() && !name.parts.contains(&NamePart::Slot) => Ok(GroupReference {
            name: name.words().collect::<Vec<_>>().join(" "),
            span: span(term),
        }),
        _ => Err(Misplaced::NotATerm(span(term))),
    }
}

pub(crate) fn span(term: &ast::ErrorTerm) -> Span {
    match term {
        ast::ErrorTerm::Own(errors) => errors.span,
        ast::ErrorTerm::Call { span, .. } => *span,
    }
}
