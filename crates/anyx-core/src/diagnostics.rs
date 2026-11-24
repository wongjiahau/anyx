//! Source locations for declaration checking errors.

use crate::{kernel::DeclarationCheckingError, lowering::head_parts, parser};
use anyx::Span;

impl DeclarationCheckingError {
    /// Locate the signature, body, or first declaration in a recursive group.
    pub fn span(&self, document: &parser::Document) -> Span {
        for declaration in &document.declaration {
            match declaration {
                parser::Declaration::Function(function) => {
                    let (name, _) = head_parts(&function.head);
                    if matches!(self, Self::Body { name: failed, .. } if failed == &name.text) {
                        return function.body.span();
                    }
                    if matches!(self, Self::NonTerminatingFunctions(names) if names.contains(&name.text))
                    {
                        return function.head.span();
                    }
                    if let Some(span) =
                        self.signature_span(&function.head, function.signature_result.span())
                    {
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

    fn signature_span(&self, head: &parser::Head, result: Span) -> Option<Span> {
        let (name, _) = head_parts(head);
        let (failed, span) = match self {
            Self::DuplicateGlobal(name) => (name, head.span()),
            Self::Signature { name, .. } => (name, head.span().to(result)),
            Self::DataMustReturnUniverse(name) => (name, result),
            Self::ConstructorMustReturnData { constructor, .. } => (constructor, result),
            Self::NonPositiveOccurrence { declaration, .. } => {
                (declaration, head.span().to(result))
            }
            Self::Body { .. } | Self::NonTerminatingFunctions(_) => return None,
        };
        (failed == &name.text).then_some(span)
    }
}
