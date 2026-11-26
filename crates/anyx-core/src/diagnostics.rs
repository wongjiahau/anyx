//! Source locations for declaration checking errors.

use crate::{
    kernel::DeclarationCheckingError,
    lowering::{default_name, field_parts, head_parts, overload_name, record_constructor_name},
    parser,
};
use anyx::Span;

impl DeclarationCheckingError {
    /// Locate the signature, body, or first declaration in a recursive group.
    pub fn span(&self, document: &parser::Document) -> Span {
        for declaration in &document.declaration {
            match declaration {
                parser::Declaration::Function(function) => {
                    let names = |failed: &str| declares(&function.head, failed);
                    if matches!(self, Self::Body { name: failed, .. } if names(failed)) {
                        return function.body.span();
                    }
                    if matches!(self, Self::NonTerminatingFunctions(failed) if failed.iter().any(|failed| names(failed)))
                    {
                        return function.head.span();
                    }
                    if let Some(span) =
                        self.signature_span(&function.head, function.signature_result.span())
                    {
                        return span;
                    }
                }
                parser::Declaration::ServedFunction(function) => {
                    if let Some(span) = self.signature_span(&function.head, function.result.span())
                    {
                        return span;
                    }
                }
                parser::Declaration::Record(record) => {
                    if let Some(span) = self.record_span(record) {
                        return span;
                    }
                }
                parser::Declaration::Data(data) => {
                    if let Some(span) =
                        self.signature_span(&data.head, data.signature_result.span())
                    {
                        return span;
                    }
                    for constructor in &data.constructors.value {
                        if let Some(span) =
                            self.signature_span(&constructor.head, constructor.r#type.span())
                        {
                            return span;
                        }
                    }
                }
            }
        }
        document.span
    }

    /// The record's head for its own errors and its constructor's, and a
    /// field's `DEFAULT` for errors in it.
    fn record_span(&self, record: &parser::Record) -> Option<Span> {
        let (name, parameters) = head_parts(&record.head);
        if let Some(span) = self.signature_span(&record.head, record.signature_result.span()) {
            return Some(span);
        }
        let slots = parameters.len()
            + record
                .fields
                .value
                .iter()
                .map(|field| field_parts(field).1.len())
                .sum::<usize>();
        let constructor = record_constructor_name(&name.text, slots);
        let failed = match self {
            Self::Signature { name, .. }
            | Self::ConstructorMustReturnData {
                constructor: name, ..
            }
            | Self::NonPositiveOccurrence {
                declaration: name, ..
            }
            | Self::Body { name, .. } => name,
            _ => return None,
        };
        if *failed == constructor {
            return Some(record.fields.span);
        }
        record.fields.value.iter().find_map(|field| {
            let parser::Field::Defaulted { entry, .. } = field else {
                return None;
            };
            field_parts(field)
                .1
                .iter()
                .any(|slot| *failed == default_name(&name.text, &slot.binder.name.value))
                .then(|| entry.span())
        })
    }

    fn signature_span(&self, head: &parser::Head, result: Span) -> Option<Span> {
        let (name, _) = head_parts(head);
        let (failed, span) = match self {
            Self::DuplicateGlobal(name) => (name, head.span()),
            Self::Signature { name, .. } => (name, head.span().to(result)),
            Self::DataMustReturnUniverse(name) => (name, result),
            Self::ConstructorMustReturnData { constructor, .. } => (constructor, result),
            Self::ServedFunctionMustReturnAValue(name) => (name, result),
            Self::NonPositiveOccurrence { declaration, .. } => {
                (declaration, head.span().to(result))
            }
            Self::OverloadsClash { second, .. } => (second, head.span()),
            Self::Body { .. } | Self::NonTerminatingFunctions(_) => return None,
        };
        (failed == &name.text || declares(head, failed)).then_some(span)
    }
}

/// Whether `head` declares the function the kernel knows as `name`: its
/// own name, or its name as an overload, `path of (Details)`.
fn declares(head: &parser::Head, name: &str) -> bool {
    head_parts(head).0.text == name || overload_name(head) == name
}
