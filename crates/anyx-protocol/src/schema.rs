//! A protocol has one flat namespace, split across explicitly imported files.

use anyx::{NamePart, Node, NodeKind, Span, line_column};

use crate::check::Checker;
use crate::codec::Outcome;
use crate::grammar as ast;
use crate::value::Value;

/// A source file, useful for embedding a protocol in generated code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchemaFile<'a> {
    pub path: &'a str,
    pub source: &'a str,
}

/// A schema that passed every check.
#[derive(Clone, Debug, PartialEq)]
pub struct Schema {
    /// The files as written, which the guide shows as they are: the root
    /// first, then each file in import traversal order.
    pub files: Vec<SourceFile>,
    pub doc: Option<String>,
    pub name: String,
    pub version: u32,
    pub types: Vec<TypeDefinition>,
    pub methods: Vec<Method>,
    /// The calls every server answers, after its own methods: `SCHEMA`,
    /// then `SCHEMA FILES`.
    pub built_ins: Vec<Method>,
    /// The prelude's `INTRINSIC FUNCTION`s, which a value may be written
    /// as a call of, as a `Duration` is: `1 hours 30 minutes`.
    pub intrinsic_functions: Vec<IntrinsicFunction>,
}

/// An `INTRINSIC FUNCTION` of the prelude: a value of the type it returns
/// may be written as a call of it, and the implementation computes the
/// value from the call's arguments (`src/prelude.rs`).
#[derive(Clone, Debug, PartialEq)]
pub struct IntrinsicFunction {
    pub shape: Shape,
    pub returns: Type,
}

/// A file of a checked schema.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceFile {
    pub path: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeDefinition {
    /// An inline record with an internal name for generated code.
    pub anonymous: bool,
    /// `PUBLIC`: direct importers may use it.
    pub public: bool,
    pub doc: Option<String>,
    /// Its shape: lowercase words, such as `file summary`, with `_` for
    /// each slot of a generic record or choice, such as `page of _` for
    /// `TYPE Page of (Item: TYPE) { … }`. A type is known by its words and
    /// where its slots sit, so `page` and `page of _` are two types.
    pub name: String,
    /// The type parameters of a generic record or choice, one per slot of
    /// its shape, in order: `(Value: TYPE) or (Problem: TYPE)` has `Value`
    /// and `Problem`. Empty for any other type.
    pub parameters: Vec<String>,
    pub kind: TypeKind,
}

impl TypeDefinition {
    /// Its shape with each parameter in its slot, as a use of it is
    /// written: `page of (Item)`, or `file summary` for a type that isn't
    /// generic.
    pub fn shape(&self) -> String {
        fill_slots(&self.name, &self.parameters)
    }
}

/// `text`, a shape with `_` for each slot, with each slot filled by the next
/// of `arguments` in parentheses: `page of _` with `Text` is
/// `page of (Text)`. A slot left over stays `_`.
pub fn fill_slots(text: &str, arguments: &[String]) -> String {
    let mut arguments = arguments.iter();
    text.split(' ')
        .map(|word| match (word, word == "_") {
            (_, true) => arguments
                .next()
                .map_or_else(|| "_".to_string(), |argument| format!("({argument})")),
            (word, false) => word.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeKind {
    Record(Vec<Field>),
    /// An alias of a union of variants, `` TYPE ALIAS Case [| `smart,
    /// `any |] ``: its variants, each written in its own shape. Unlike other
    /// aliases it keeps its name, for generated code and messages, while
    /// its values are its variants, written the same.
    Variants(Vec<Shaped>),
    /// A named type, `NAMED TYPE File Id (value: Text)`: a type of its own, which no
    /// other type fits. A value is written with its constructor, the shape
    /// of its name with a value in each slot, `File Id "a"`, and holds the
    /// values of its slots, as a variant of the constructor's shape does.
    Distinct {
        /// The shape of its name: `File Id _`.
        constructor: Vec<ShapePart>,
        /// The type of its one slot, or a tuple of its slots' types:
        /// `Text` for `File Id (Text)`, `()` for a named type with none.
        underlying: Type,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub doc: Option<String>,
    /// Its words, which name it in generated code and in conditions:
    /// `path`, `hidden`, `changed more than ago`; for a field written as
    /// its variant alone, its parameter's name.
    pub name: String,
    pub r#type: Type,
    /// The value supplied when the field is omitted. Without a default the
    /// field is required, including when its type admits `null`.
    pub default: Option<crate::value::Value>,
    pub form: FieldForm,
}

/// A value's outermost form: `` `newest first `` is the variant
/// `newest first`, `"src"` a string. A variant finds the field without a
/// name an entry fills.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Written {
    /// A call of this shape that isn't marked as a variant, such as a named
    /// type's constructor `File Id _`, or a field `changed within _`.
    Call(Vec<ShapePart>),
    /// A variant of this shape, marked with a backtick: `` `newest first ``.
    Variant(Vec<ShapePart>),
    String,
    Number,
    List,
    Braces,
}

impl std::fmt::Display for Written {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Written::Call(shape) => write!(f, "`{}`", ShapePart::text(shape)),
            Written::Variant(shape) => write!(f, "`` `{} ``", ShapePart::text(shape)),
            Written::String => f.write_str("a string"),
            Written::Number => f.write_str("a number"),
            Written::List => f.write_str("a list `[…]`"),
            Written::Braces => f.write_str("braces `{…}`"),
        }
    }
}

impl Written {
    /// Whether an entry written as `found` may be a value of a type
    /// written as `self`, or the two clash as members of a union: the same
    /// form.
    pub fn admits(&self, found: &Written) -> bool {
        self == found
    }

    /// A node's outermost form, read with the request grammar, `None` for
    /// one no value is written as.
    pub fn of(node: &anyx::Node) -> Option<Written> {
        crate::request_grammar::decode_value(node)
            .ok()
            .map(|value| crate::request::written(&value))
    }
}

/// `text` quoted as code: `` `Text` ``, or with two backticks around text
/// that holds one, as `` `null ``.
pub(crate) fn code(text: &str) -> String {
    match text.contains('`') {
        true => format!("`` {text} ``"),
        false => format!("`{text}`"),
    }
}

/// `file summary` as `fileSummary`.
pub fn camel_case(name: &str) -> String {
    name.split(' ')
        .enumerate()
        .map(|(index, word)| match index {
            0 => word.to_string(),
            _ => {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                    .unwrap_or_default()
            }
        })
        .collect()
}

/// A value's outermost form in JSON, which tells a union's member there:
/// a variant by its `kind`, and other values by their JSON type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonForm {
    Number,
    String,
    Array,
    /// An object without a `kind`: a record or a map.
    Object,
    /// An object with this `kind`: a variant, such as `customHue`.
    Kind(String),
    /// An object whose one key is this tag: a named type's value, such as
    /// `{"FileId": "a"}`.
    Tag(String),
}

impl std::fmt::Display for JsonForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonForm::Number => f.write_str("a number"),
            JsonForm::String => f.write_str("a string"),
            JsonForm::Array => f.write_str("an array"),
            JsonForm::Object => f.write_str("an object"),
            JsonForm::Kind(kind) => write!(f, "an object of the kind \"{kind}\""),
            JsonForm::Tag(tag) => write!(f, "an object tagged \"{tag}\""),
        }
    }
}

/// The key a named type's value goes under in JSON, its words in camel
/// case, which start with a capital as its name does: `FileId` for `File
/// Id _`, `RangeFromTo` for `Range from _ to _`.
pub fn json_tag(constructor: &[ShapePart]) -> String {
    camel_case(&ShapePart::words(constructor))
}

/// The types of a named type's slots, in order, from the type it holds:
/// that type for one slot, and the tuple's for any other number.
pub fn slot_types<'t>(constructor: &[ShapePart], underlying: &'t Type) -> Vec<&'t Type> {
    match underlying {
        Type::Tuple(slots, _)
            if constructor
                .iter()
                .filter(|part| **part == ShapePart::Slot)
                .count()
                != 1 =>
        {
            slots.iter().collect()
        }
        one => vec![one],
    }
}

/// A named type's constructor as a shape, with a parameter for each slot,
/// so that a value of it is written and read as a variant of that shape
/// is: `Value::Variant` with the shape `File Id _` and the slots' values.
pub fn constructor_shape(constructor: &[ShapePart], underlying: &Type) -> Shape {
    Shape {
        text: ShapePart::text(constructor),
        parts: constructor.to_vec(),
        parameters: slot_types(constructor, underlying)
            .into_iter()
            .map(|r#type| Parameter {
                name: ShapePart::words(constructor),
                r#type: r#type.clone(),
            })
            .collect(),
        optional: Vec::new(),
    }
}

/// How a record value writes a field.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldForm {
    /// Its shape with its value in the one slot: `path _` for
    /// `path: Text`, `_ case`, `at least _ bytes`.
    Shape(String),
    /// Its variant alone, `` `newest first ``, declared `` `(order: Order) ``
    /// and found by the variant's shape. Every value of its type is a
    /// variant. The field's name is its parameter's.
    Unnamed,
}

/// A call's shape: words with a typed slot between or around them, such as
/// `conflict at revision (revision: Integer)`. With optional parts, it is
/// the full form, `find _ in _` for `find (pattern: Text) [in (paths: LIST
/// Text) DEFAULT in ["."]]`, and a call may be written in any of its
/// [`Shape::forms`].
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// The words with `_` for each slot: `conflict at revision _`.
    pub text: String,
    /// Its words and slots, in order, as the parser read them.
    pub parts: Vec<ShapePart>,
    pub parameters: Vec<Parameter>,
    /// The parts a call may leave out, in order.
    pub optional: Vec<OptionalPart>,
}

/// A part of a shape that a call may leave out, `[in (paths: LIST Text)
/// DEFAULT in ["."]]`, and what its parameters hold then.
#[derive(Clone, Debug)]
pub struct OptionalPart {
    /// Its words and slots among the shape's parts.
    pub parts: std::ops::Range<usize>,
    /// Its parameters among the shape's parameters.
    pub parameters: std::ops::Range<usize>,
    /// A value for each of its parameters, read from its `DEFAULT` once
    /// every type is known.
    pub default: Vec<Value>,
    /// The `DEFAULT`'s entry as written, `in ["."]`, which the checker
    /// reads into [`OptionalPart::default`].
    pub(crate) entry: Node,
}

/// Two parts are the same when they cover the same parts and hold the same
/// defaults, wherever their entries were written.
impl PartialEq for OptionalPart {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
            && self.parameters == other.parameters
            && self.default == other.default
    }
}

/// A way to write a call of a shape: some of its optional parts left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    /// The words with `_` for each slot it writes: `find _`.
    pub text: String,
    /// Whether each of the shape's optional parts is written.
    pub written: Vec<bool>,
}

impl Shape {
    /// `self` with each of `parameters` replaced by the argument at its
    /// place in `arguments`, in the types of its slots.
    pub fn substitute(&self, parameters: &[String], arguments: &[Type]) -> Shape {
        Shape {
            text: self.text.clone(),
            parts: self.parts.clone(),
            parameters: self
                .parameters
                .iter()
                .map(|parameter| Parameter {
                    name: parameter.name.clone(),
                    r#type: parameter.r#type.substitute(parameters, arguments),
                })
                .collect(),
            optional: self.optional.clone(),
        }
    }

    /// Every way to write a call of it: the full form first, then with
    /// optional parts left out, earlier parts varying slowest. A call has a
    /// word, so leaving out every part of a shape made only of optional
    /// parts, as `Duration`'s, is no way to write it.
    pub fn forms(&self) -> Vec<Form> {
        let count = self.optional.len();
        (0..1usize << count)
            .map(|left_out| {
                let written: Vec<bool> = (0..count)
                    .map(|index| left_out & (1 << (count - 1 - index)) == 0)
                    .collect();
                Form {
                    text: ShapePart::text(&self.written_parts(&written)),
                    written,
                }
            })
            .filter(|form| form.text.split(' ').any(|part| part != "_"))
            .collect()
    }

    /// The form a call named `text` is written in, if it is one of its.
    pub fn form(&self, text: &str) -> Option<Form> {
        if self.optional.is_empty() {
            return (self.text == text).then(|| Form {
                text: self.text.clone(),
                written: Vec::new(),
            });
        }
        self.forms().into_iter().find(|form| form.text == text)
    }

    /// Each way to write it as a variant, marked with a backtick.
    pub fn written_forms(&self) -> Vec<Written> {
        self.forms()
            .iter()
            .map(|form| Written::Variant(self.written_parts(&form.written)))
            .collect()
    }

    /// The words of each way to write it, the full form's first: a
    /// variant is told from another by these.
    pub fn form_words(&self) -> Vec<String> {
        self.forms()
            .iter()
            .map(|form| ShapePart::words(&self.written_parts(&form.written)))
            .collect()
    }

    /// Its parts that `written` writes.
    fn written_parts(&self, written: &[bool]) -> Vec<ShapePart> {
        self.parts
            .iter()
            .enumerate()
            .filter(|(index, _)| self.writes(written, index, |part| &part.parts))
            .map(|(_, part)| part.clone())
            .collect()
    }

    /// Whether the part or parameter at `index` is written, where `range`
    /// says which of them an optional part holds.
    fn writes(
        &self,
        written: &[bool],
        index: &usize,
        range: impl Fn(&OptionalPart) -> &std::ops::Range<usize>,
    ) -> bool {
        self.optional
            .iter()
            .zip(written)
            .all(|(part, written)| *written || !range(part).contains(index))
    }

    /// The shape as `form` writes it, with only the parameters it writes
    /// and no optional parts: what a call written in it is read against.
    pub fn written_as(&self, form: &Form) -> Shape {
        Shape {
            text: form.text.clone(),
            parts: self.written_parts(&form.written),
            parameters: self
                .parameters
                .iter()
                .enumerate()
                .filter(|(index, _)| self.writes(&form.written, index, |part| &part.parameters))
                .map(|(_, parameter)| parameter.clone())
                .collect(),
            optional: Vec::new(),
        }
    }

    /// Every argument of a call written in `form`: the `written` ones, and
    /// each left-out part's default. `None` while a default it needs isn't
    /// read yet, which only the checker sees.
    pub fn fill(&self, form: &Form, written: Vec<Value>) -> Option<Vec<Value>> {
        self.fill_with(form, written, |_, value| value.clone())
    }

    /// Every argument of a call written in `form`, as [`Self::fill`] has
    /// them, each left-out part's default made by `default` from the
    /// parameter's index and the default value: for a caller
    /// whose arguments aren't values yet, such as the shell's.
    pub fn fill_with<T>(
        &self,
        form: &Form,
        written: Vec<T>,
        mut default: impl FnMut(usize, &Value) -> T,
    ) -> Option<Vec<T>> {
        let mut written = written.into_iter();
        (0..self.parameters.len())
            .map(|index| {
                match self
                    .optional
                    .iter()
                    .zip(&form.written)
                    .find(|(part, _)| part.parameters.contains(&index))
                {
                    Some((part, false)) => part
                        .default
                        .get(index - part.parameters.start)
                        .map(|value| default(index, value)),
                    _ => written.next(),
                }
            })
            .collect()
    }

    /// The default of the parameter at `index`, when an optional part
    /// holds it: what a call that leaves the part out has there.
    pub fn default_of(&self, index: usize) -> Option<&Value> {
        let part = self
            .optional
            .iter()
            .find(|part| part.parameters.contains(&index))?;
        part.default.get(index - part.parameters.start)
    }

    /// The optional part at `index` alone, as its `DEFAULT` writes it:
    /// `in (paths: LIST Text)`.
    pub fn part(&self, index: usize) -> Shape {
        let part = &self.optional[index];
        let parts = self.parts[part.parts.clone()].to_vec();
        Shape {
            text: ShapePart::text(&parts),
            parts,
            parameters: self.parameters[part.parameters.clone()].to_vec(),
            optional: Vec::new(),
        }
    }

    /// The words alone, which name it: `conflict at revision`.
    pub fn words(&self) -> String {
        self.parts
            .iter()
            .filter_map(|part| match part {
                ShapePart::Word(word) => Some(word.as_str()),
                ShapePart::Slot => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Calls `visit` on every shape in `schema`: methods', variants', and
/// those in types, the shapes in their parameters' types after them.
pub(crate) fn each_shape(schema: &mut Schema, visit: &mut dyn FnMut(&mut Shape)) {
    fn in_type(r#type: &mut Type, visit: &mut dyn FnMut(&mut Shape)) {
        match r#type {
            Type::Refined(base, _) => in_type(base, visit),
            Type::List(item) | Type::Map(item) => in_type(item, visit),
            Type::Generic(_, types) | Type::Tuple(types, _) | Type::Union(types) => {
                types.iter_mut().for_each(|r#type| in_type(r#type, visit))
            }
            Type::Variant(shape) => in_shape(shape, visit),
            _ => {}
        }
    }
    fn in_shape(shape: &mut Shape, visit: &mut dyn FnMut(&mut Shape)) {
        visit(shape);
        for parameter in &mut shape.parameters {
            in_type(&mut parameter.r#type, visit);
        }
    }
    for definition in &mut schema.types {
        match &mut definition.kind {
            TypeKind::Record(fields) => fields
                .iter_mut()
                .for_each(|field| in_type(&mut field.r#type, visit)),
            TypeKind::Variants(variants) => variants
                .iter_mut()
                .for_each(|variant| in_shape(&mut variant.shape, visit)),
            TypeKind::Distinct { underlying, .. } => in_type(underlying, visit),
        }
    }
    for function in &mut schema.intrinsic_functions {
        in_shape(&mut function.shape, visit);
    }
    for method in schema.methods.iter_mut().chain(&mut schema.built_ins) {
        in_shape(&mut method.shape, visit);
        if let Some(returns) = &mut method.returns {
            in_type(&mut returns.r#type, visit);
        }
        for error in &mut method.errors {
            in_shape(&mut error.shape, visit);
        }
        method
            .error_groups
            .iter_mut()
            .for_each(|group| in_type(group, visit));
    }
}

/// A word or a slot of a shape, from a call's name as the parser read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapePart {
    Word(String),
    Slot,
}

impl ShapePart {
    /// The parts of a call's name, without their spans.
    pub fn of(name: &[anyx::NamePart]) -> Vec<ShapePart> {
        name.iter()
            .map(|part| match part {
                anyx::NamePart::Word { text, .. } => ShapePart::Word(text.clone()),
                anyx::NamePart::Slot => ShapePart::Slot,
            })
            .collect()
    }

    /// The words with `_` for each slot, joined by spaces: `changed within _`.
    /// The words alone, without the slots: `File Id` for `File Id _`.
    pub fn words(parts: &[ShapePart]) -> String {
        parts
            .iter()
            .filter_map(|part| match part {
                ShapePart::Word(word) => Some(word.as_str()),
                ShapePart::Slot => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn text(parts: &[ShapePart]) -> String {
        parts
            .iter()
            .map(|part| match part {
                ShapePart::Word(word) => word.as_str(),
                ShapePart::Slot => "_",
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A choice's variant, or an error: a shape with a doc.
#[derive(Clone, Debug, PartialEq)]
pub struct Shaped {
    pub doc: Option<String>,
    pub shape: Shape,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Method {
    pub doc: Option<String>,
    pub shape: Shape,
    /// What a call returns when it succeeds, named, or `None` for a
    /// method that `RETURNS NOTHING`.
    pub returns: Option<Parameter>,
    /// Conditions checked on the whole request before any handler runs.
    pub requires: Option<crate::refinement::Predicate<usize>>,
    /// Conditions checked before sending a successful method result.
    pub ensures: Option<crate::refinement::Predicate<usize>>,
    /// Its errors, variants each written in its shape alone: those in its
    /// `ERRORS` in the order written, with each alias's variants in its
    /// place. Empty for `ERRORS [| |]`.
    pub errors: Vec<Shaped>,
    /// The aliases of unions of variants its `ERRORS` names, such as `File
    /// Errors`, whose variants are among [`Method::errors`]. Generated code
    /// converts a value of each into the method's errors.
    pub error_groups: Vec<Type>,
    /// Its `EXAMPLES`, in the order written, each checked against it.
    pub examples: Vec<Example>,
    /// What a call may do to the world: `READ ONLY`, `IDEMPOTENT`, or
    /// neither.
    pub effect: Effect,
    /// The call that undoes it, from `UNDONE BY`.
    pub undone_by: Option<Undo>,
}

/// What a call of a method may do, as its markers claim. Each marker is a
/// claim of safety, so a method without one is the worst case.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Effect {
    /// No marker: a call may change anything, and calling it again may do
    /// it again, so it is never retried on its own.
    #[default]
    Unmarked,
    /// `IDEMPOTENT`: calling it again with the same arguments leaves the
    /// same state as calling it once, even when the second response
    /// differs. Safe to retry after a lost response.
    Idempotent,
    /// `READ ONLY`: a call asks for no change, so it is also idempotent.
    /// Safe to call freely.
    ReadOnly,
}

impl Effect {
    pub fn read_only(self) -> bool {
        self == Effect::ReadOnly
    }

    /// Whether calling it again is safe: `IDEMPOTENT`, or `READ ONLY`, which
    /// implies it.
    pub fn idempotent(self) -> bool {
        self != Effect::Unmarked
    }
}

/// `UNDONE BY move (new path: Text) to (path: Text)`: the call of another
/// method of the same protocol that compensates a call of this one, with
/// this method's parameters as its arguments. It also claims that the call
/// disclosed nothing outside the account that undoing would not take back.
#[derive(Clone, Debug, PartialEq)]
pub struct Undo {
    /// The undoing method, an index into [`Schema::methods`].
    pub method: usize,
    /// For each parameter of the undoing method, the index of the parameter
    /// of this method that fills it.
    pub arguments: Vec<usize>,
    /// The call as written after `UNDONE BY`, such as
    /// `move (new path: Text) to (path: Text)`.
    pub text: String,
}

/// An example of a method: a call of it and its outcome side by side, as
/// in `write "a" at revision 3 content "x" RETURNS 4`.
#[derive(Clone, Debug, PartialEq)]
pub struct Example {
    pub doc: Option<String>,
    /// The call and its outcome as written in the schema, without the doc.
    pub text: String,
    /// The call alone as written, which a request may send as it is.
    pub call: String,
    /// The call's arguments, one per parameter of the method.
    pub arguments: Vec<Value>,
    /// What the call returns, [`Outcome::Value`], or the error it returns,
    /// [`Outcome::Error`].
    pub outcome: Outcome,
}

impl Method {
    /// The method shape, without any file path.
    pub fn full_name(&self) -> String {
        self.shape.text.clone()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub r#type: Type,
}

/// What [`Schema::fits_as`] compares: a type, or a literal's own type.
#[derive(Clone, Copy, PartialEq)]
enum Fitting {
    Type,
    Literal,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Text,
    /// Raw bytes, such as a file. On the wire, a base64 string.
    Bytes,
    Integer,
    /// A finite 64-bit IEEE 754 float: `f64` in Rust, `number` in
    /// TypeScript. Written with a point or an exponent, `1.5`, `3.0`, `1e3`.
    Float,
    /// A point in time, to the millisecond. On the wire, RFC 3339 text in
    /// UTC: `"2026-11-14T06:00:08.000Z"`.
    Instant,
    /// A length of time, to the millisecond. On the wire, whole numbers
    /// with their units: `1 hours 30 minutes`.
    Duration,
    /// A type narrowed by a checked predicate: `(Integer) WHERE [1 <= _]`.
    Refined(Box<Type>, crate::refinement::Predicate),
    List(Box<Type>),
    /// Text keys, each to a value of the type. On the wire, braces of keys
    /// and values: `{ "hymn" 3, "ode" 1 }`.
    Map(Box<Type>),
    /// The index of a type in [`Schema::types`], one that isn't generic.
    Named(usize),
    /// A generic record or choice with a type in each slot, `page of
    /// (Text)`: the index of the type in [`Schema::types`], and a type for
    /// each of its parameters, in the order of its slots.
    Generic(usize, Vec<Type>),
    /// The slots of a record's field with several of them, as in
    /// `between (start: Integer) and (end: Integer)`: a type for each, in order. It has
    /// no syntax of its own; a value is written in the field's shape.
    /// Slot types and their names in Anyx shape order. Both are empty for a unit.
    Tuple(Vec<Type>, Vec<String>),
    /// A type parameter, by its name, within the generic record or choice
    /// that declares it. A use of that type puts its argument in its place,
    /// so a value is never of this type.
    Parameter(String),
    /// Values of any of its members, each written as that member writes
    /// it: `` [| Integer, `null |] `` holds `3` and `null`. Its members are as
    /// written; [`Schema::members`] reads through nested unions. A value's
    /// form tells its member, so the checker rejects two members written
    /// alike.
    Union(Vec<Type>),
    /// A variant as a type of its own, whose values are written in its
    /// shape: `null`, `custom hue (hue: Float)`. It is a union's member.
    Variant(Box<Shape>),
}

impl Type {
    /// The members of a union, reading through the unions among them, in
    /// order: `` [| Text, [| `null, `none |] |] `` has `Text`, `null` and
    /// `none`. Any other type is its own one member.
    pub fn members(&self) -> Vec<&Type> {
        match self {
            Type::Union(members) => members.iter().flat_map(Type::members).collect(),
            other => vec![other],
        }
    }

    /// `self` with each of `parameters` replaced by the argument at its
    /// place in `arguments`.
    pub fn substitute(&self, parameters: &[String], arguments: &[Type]) -> Type {
        match self {
            Type::Parameter(name) => parameters
                .iter()
                .position(|parameter| parameter == name)
                .and_then(|index| arguments.get(index))
                .cloned()
                .unwrap_or_else(|| self.clone()),
            Type::Refined(base, predicate) => Type::Refined(
                Box::new(base.substitute(parameters, arguments)),
                predicate.clone(),
            ),
            Type::List(item) => Type::List(Box::new(item.substitute(parameters, arguments))),
            Type::Map(value) => Type::Map(Box::new(value.substitute(parameters, arguments))),
            Type::Tuple(slots, names) => Type::Tuple(
                slots
                    .iter()
                    .map(|slot| slot.substitute(parameters, arguments))
                    .collect(),
                names.clone(),
            ),
            Type::Generic(index, inner) => Type::Generic(
                *index,
                inner
                    .iter()
                    .map(|argument| argument.substitute(parameters, arguments))
                    .collect(),
            ),
            Type::Union(members) => Type::Union(
                members
                    .iter()
                    .map(|member| member.substitute(parameters, arguments))
                    .collect(),
            ),
            Type::Variant(shape) => {
                Type::Variant(Box::new(shape.substitute(parameters, arguments)))
            }
            _ => self.clone(),
        }
    }

    /// The type parameters it mentions, each once, in the order met.
    pub fn parameters(&self) -> Vec<&str> {
        fn walk<'t>(r#type: &'t Type, found: &mut Vec<&'t str>) {
            match r#type {
                Type::Parameter(name) if !found.contains(&name.as_str()) => found.push(name),
                Type::List(item) | Type::Map(item) | Type::Refined(item, _) => walk(item, found),
                Type::Generic(_, arguments)
                | Type::Tuple(arguments, _)
                | Type::Union(arguments) => {
                    arguments.iter().for_each(|argument| walk(argument, found))
                }
                Type::Variant(shape) => shape
                    .parameters
                    .iter()
                    .for_each(|parameter| walk(&parameter.r#type, found)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        walk(self, &mut found);
        found
    }
}

impl TypeKind {
    /// `self` with each of `parameters` replaced by the argument at its
    /// place in `arguments`, in every field or variant.
    pub fn substitute(&self, parameters: &[String], arguments: &[Type]) -> TypeKind {
        match self {
            TypeKind::Record(fields) => TypeKind::Record(
                fields
                    .iter()
                    .map(|field| Field {
                        r#type: field.r#type.substitute(parameters, arguments),
                        ..field.clone()
                    })
                    .collect(),
            ),
            TypeKind::Variants(variants) => TypeKind::Variants(
                variants
                    .iter()
                    .map(|variant| Shaped {
                        doc: variant.doc.clone(),
                        shape: Shape {
                            text: variant.shape.text.clone(),
                            parts: variant.shape.parts.clone(),
                            optional: variant.shape.optional.clone(),
                            parameters: variant
                                .shape
                                .parameters
                                .iter()
                                .map(|parameter| Parameter {
                                    name: parameter.name.clone(),
                                    r#type: parameter.r#type.substitute(parameters, arguments),
                                })
                                .collect(),
                        },
                    })
                    .collect(),
            ),
            TypeKind::Distinct {
                constructor,
                underlying,
            } => TypeKind::Distinct {
                constructor: constructor.clone(),
                underlying: underlying.substitute(parameters, arguments),
            },
        }
    }
}

/// The words of the built-in call every server answers with the guide to
/// its schema, without how to send a request: `SCHEMA`. All-caps words
/// belong to the schema language, so no method of a schema is called that.
pub const INTROSPECTION: &str = "SCHEMA";

/// The words of the built-in call every server answers with the files of
/// its schema, the entry first, so a caller can check the schema itself.
pub const SCHEMA_FILES: &str = "SCHEMA FILES";

/// The built-in `SCHEMA` method: no parameters, returning the guide as
/// `Text`, with no errors.
pub fn introspection() -> &'static Method {
    static METHOD: std::sync::LazyLock<Method> = std::sync::LazyLock::new(|| {
        built_in(
            &["SCHEMA"],
            "The guide to this protocol and its schema.",
            "guide",
            Type::Text,
        )
    });
    &METHOD
}

/// The calls every server answers, given the types of its schema, which
/// hold the prelude's `Schema File`.
pub(crate) fn built_ins(types: &[TypeDefinition]) -> Vec<Method> {
    let schema_file = types
        .iter()
        .position(|definition| definition.name == "Schema File")
        .expect("the prelude declares `Schema File`");
    vec![
        introspection().clone(),
        built_in(
            &["SCHEMA", "FILES"],
            "The files of this protocol's schema, the entry first, then each file it imports.",
            "files",
            Type::List(Box::new(Type::Named(schema_file))),
        ),
    ]
}

fn built_in(words: &[&str], doc: &str, returns: &str, r#type: Type) -> Method {
    Method {
        doc: Some(doc.to_string()),
        shape: Shape {
            text: words.join(" "),
            parts: words
                .iter()
                .map(|word| ShapePart::Word(word.to_string()))
                .collect(),
            parameters: Vec::new(),
            optional: Vec::new(),
        },
        returns: Some(Parameter {
            name: returns.to_string(),
            r#type,
        }),
        requires: None,
        ensures: None,
        errors: Vec::new(),
        error_groups: Vec::new(),
        examples: Vec::new(),
        effect: Effect::ReadOnly,
        undone_by: None,
    }
}

/// `keyword` before the type `name`, as the formatter writes it: with a
/// `:` only where `name` starts with a keyword it would join, as in
/// `LIST: LIST Text`, and without one otherwise, `LIST Text`.
fn keyword_before(keyword: &str, name: String) -> String {
    match name.split(' ').next().is_some_and(anyx::is_keyword) {
        true => format!("{keyword}: {name}"),
        false => format!("{keyword} {name}"),
    }
}

/// A decoded file and the indices of its direct imports.
pub(crate) struct DecodedFile {
    pub imports: Vec<usize>,
    pub document: ast::Document,
}

impl Schema {
    /// Checks a single source, including a headerless library. Imports require
    /// [`Schema::load`], which supplies the file reader.
    pub fn parse(source: &str) -> Result<Schema, Vec<Diagnostic>> {
        Self::load("schema.protocol.anyx", |path| {
            if path == "schema.protocol.anyx" {
                Ok(source.to_string())
            } else {
                Err(format!("no reader supplied for `{path}`; use Schema::load"))
            }
        })
    }

    /// Checks an entry file and exactly the files reached through `USE`.
    /// The reader receives normalized paths relative to the caller's working
    /// directory (or absolute paths when the entry is absolute). Each file is
    /// read once. Libraries may be checked without a `PROTOCOL` header.
    pub fn load<E: std::fmt::Display>(
        entry: &str,
        mut read: impl FnMut(&str) -> Result<String, E>,
    ) -> Result<Schema, Vec<Diagnostic>> {
        let entry = normalize_path(entry);
        let mut loader = Loader {
            read: &mut read,
            files: Vec::new(),
            decoded: Vec::new(),
            visiting: Vec::new(),
            attempted: std::collections::BTreeSet::new(),
            diagnostics: Vec::new(),
        };
        loader.visit(&entry);
        if !loader.diagnostics.is_empty() {
            return Err(loader.diagnostics);
        }
        // Append the implicit library after user types to keep their indices stable.
        let prelude = loader.files.len();
        for decoded in &mut loader.decoded {
            decoded.imports.push(prelude);
        }
        loader.files.push(SourceFile {
            path: crate::prelude::PATH.into(),
            source: crate::prelude::SOURCE.into(),
        });
        loader.decoded.push(DecodedFile {
            imports: Vec::new(),
            document: crate::prelude::declarations(),
        });
        let mut checker = Checker::default();
        let mut schema = checker.schema(loader.files, &loader.decoded);
        // Embedded schemas keep only explicit sources; loading injects the prelude.
        schema.files.pop();
        let mut diagnostics = checker.diagnostics;
        diagnostics.sort_by_key(|diagnostic| {
            (
                schema
                    .files
                    .iter()
                    .position(|file| file.path == diagnostic.path),
                diagnostic.span.start,
                diagnostic.message.clone(),
            )
        });
        diagnostics.dedup();
        if diagnostics.is_empty() {
            Ok(schema)
        } else {
            Err(diagnostics)
        }
    }

    /// Whether this entry declares an endpoint rather than a library.
    pub fn is_protocol(&self) -> bool {
        !self.name.is_empty()
    }

    /// `shape` with each slot written as its parameter and type:
    /// `move (path: Text) to (new path: Text)`.
    pub fn typed_shape(&self, shape: &Shape) -> String {
        let mut parameters = shape.parameters.iter();
        shape
            .text
            .split(' ')
            .map(|word| match word {
                "_" => parameters.next().map_or_else(
                    || "_".to_string(),
                    |parameter| {
                        format!(
                            "({}: {})",
                            parameter.name,
                            self.type_name(&parameter.r#type)
                        )
                    },
                ),
                word => word.to_string(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A type's plain protocol name, filling the slots of generic shapes.
    pub fn type_name(&self, r#type: &Type) -> String {
        match r#type {
            Type::Text
            | Type::Bytes
            | Type::Integer
            | Type::Float
            | Type::Instant
            | Type::Duration => crate::prelude::of(r#type)
                .map_or_else(|| format!("{type:?}"), |intrinsic| intrinsic.name.clone()),
            Type::Refined(base, predicate) => {
                format!("({}) WHERE [{}]", self.type_name(base), predicate.source)
            }
            Type::List(item) => keyword_before("LIST", self.type_name(item)),
            Type::Map(value) => keyword_before("MAP", self.type_name(value)),
            Type::Named(index) if self.types[*index].anonymous => {
                self.anonymous_record_name(r#type)
            }
            Type::Named(index) => self.types[*index].name.clone(),
            Type::Generic(index, _) if self.types[*index].anonymous => {
                self.anonymous_record_name(r#type)
            }
            Type::Generic(index, arguments) => {
                let definition = &self.types[*index];
                let arguments: Vec<String> = arguments
                    .iter()
                    .map(|argument| self.type_name(argument))
                    .collect();
                fill_slots(&definition.name, &arguments)
            }
            Type::Parameter(name) => name.clone(),
            Type::Tuple(slots, _) => slots
                .iter()
                .map(|slot| format!("({})", self.type_name(slot)))
                .collect::<Vec<_>>()
                .join(" "),
            Type::Union(members) => match self.nullable_item(r#type) {
                Some(item) => format!("{}?", self.type_name_grouped(item)),
                None if members.is_empty() => "[| |]".to_string(),
                None => format!(
                    "[| {} |]",
                    members
                        .iter()
                        .map(|member| self.type_name(member))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            Type::Variant(shape) => format!("`{}", self.shape_name(shape)),
        }
    }

    /// Whether a method argument uses an inline record that has no spelling
    /// in a request's type annotation and is supplied by the method's schema.
    pub fn has_anonymous_record(&self, r#type: &Type) -> bool {
        match r#type {
            Type::Named(index) => self.types[*index].anonymous,
            Type::Generic(index, arguments) => {
                self.types[*index].anonymous
                    || arguments
                        .iter()
                        .any(|argument| self.has_anonymous_record(argument))
            }
            Type::Refined(base, _) | Type::List(base) | Type::Map(base) => {
                self.has_anonymous_record(base)
            }
            Type::Union(members) | Type::Tuple(members, _) => members
                .iter()
                .any(|member| self.has_anonymous_record(member)),
            Type::Variant(shape) => shape
                .parameters
                .iter()
                .any(|parameter| self.has_anonymous_record(&parameter.r#type)),
            _ => false,
        }
    }

    fn anonymous_record_name(&self, r#type: &Type) -> String {
        let Some((_, kind)) = self.instance(r#type) else {
            unreachable!("an anonymous record has a type definition");
        };
        let TypeKind::Record(fields) = kind.as_ref() else {
            unreachable!("an anonymous definition is a record");
        };
        if fields.is_empty() {
            return "{}".to_string();
        }
        let fields = fields
            .iter()
            .map(|field| match &field.form {
                FieldForm::Unnamed => {
                    format!("`({}: {})", field.name, self.type_name(&field.r#type))
                }
                FieldForm::Shape(shape) => {
                    let slots = match &field.r#type {
                        Type::Tuple(slots, names) => slots
                            .iter()
                            .zip(names)
                            .map(|(slot, name)| format!("{name}: {}", self.type_name(slot)))
                            .collect(),
                        slot => vec![self.type_name(slot)],
                    };
                    fill_slots(shape, &slots)
                }
            })
            .collect::<Vec<_>>();
        format!("{{ {} }}", fields.join(", "))
    }

    /// The name of `type`, in parentheses when it is more than one word, so
    /// that a postfix `?` takes all of it: `Text?`, `(LIST Text)?`.
    fn type_name_grouped(&self, r#type: &Type) -> String {
        let name = self.type_name(r#type);
        match name.contains(' ') {
            true => format!("({name})"),
            false => name,
        }
    }

    /// A variant's shape with the type of each slot: `custom hue (hue:
    /// Float)`.
    pub fn shape_name(&self, shape: &Shape) -> String {
        let mut parameters = shape.parameters.iter();
        shape
            .parts
            .iter()
            .map(|part| match part {
                ShapePart::Word(word) => word.clone(),
                ShapePart::Slot => parameters.next().map_or("_".to_string(), |parameter| {
                    format!(
                        "({}: {})",
                        parameter.name,
                        self.type_name(&parameter.r#type)
                    )
                }),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The member of the union `type` that a value written as `found`
    /// belongs to. The checker makes sure at most one is.
    pub fn member_written_as<'t>(&self, r#type: &'t Type, found: &Written) -> Option<&'t Type> {
        r#type.members().into_iter().find(|member| {
            self.written_forms(member)
                .is_some_and(|forms| forms.iter().any(|form| form.admits(found)))
        })
    }

    /// The member of the union `type` that holds `value`, by its form.
    pub fn member_holding<'t>(&self, r#type: &'t Type, value: &Value) -> Option<&'t Type> {
        let found = match value {
            Value::Text(_) | Value::Bytes(_) | Value::Instant(_) => Written::String,
            Value::Integer(_) | Value::Float(_) => Written::Number,
            Value::Duration(_) => {
                return r#type
                    .members()
                    .into_iter()
                    .find(|member| *self.underlying(member) == Type::Duration);
            }
            Value::List(_) => Written::List,
            Value::Map(_) | Value::Record(_) => Written::Braces,
            Value::Variant { shape, .. } => {
                return r#type.members().into_iter().find(|member| {
                    self.written_forms(member).is_some_and(|forms| {
                        forms.iter().any(|form| {
                            matches!(form, Written::Call(parts) | Written::Variant(parts) if ShapePart::text(parts) == *shape)
                        })
                    })
                });
            }
            Value::Tuple(_) | Value::Nothing => return None,
        };
        r#type.members().into_iter().find(|member| {
            self.written_forms(member)
                .is_some_and(|forms| forms.contains(&found))
        })
    }

    /// The forms a value of `type` takes in JSON, which tell a union's
    /// member there as [`Schema::written_forms`] does in anyx. `None` for a
    /// type parameter.
    pub fn json_forms(&self, r#type: &Type) -> Option<Vec<JsonForm>> {
        Some(match self.underlying(r#type) {
            Type::Text | Type::Bytes | Type::Instant => vec![JsonForm::String],
            Type::Integer | Type::Float | Type::Duration => vec![JsonForm::Number],
            Type::List(_) => vec![JsonForm::Array],
            Type::Tuple(_, names) if names.is_empty() => vec![JsonForm::Array],
            Type::Tuple(_, _) => vec![JsonForm::Object],
            Type::Map(_) => vec![JsonForm::Object],
            Type::Parameter(_) => return None,
            Type::Variant(shape) => vec![JsonForm::Kind(camel_case(&shape.words()))],
            Type::Union(members) => {
                let mut forms = Vec::new();
                for member in members {
                    forms.extend(self.json_forms(member)?);
                }
                forms
            }
            underlying => match self.instance(underlying).map(|(_, kind)| kind) {
                Some(kind) => match kind.as_ref() {
                    TypeKind::Variants(variants) => variants
                        .iter()
                        .map(|variant| JsonForm::Kind(camel_case(&variant.shape.words())))
                        .collect(),
                    TypeKind::Distinct { constructor, .. } => {
                        vec![JsonForm::Tag(json_tag(constructor))]
                    }
                    TypeKind::Record(_) => vec![JsonForm::Object],
                },
                None => vec![JsonForm::Object],
            },
        })
    }

    /// The record or choice that `type` names, after reading past named
    /// types, and its fields or variants: for a generic type, with its
    /// arguments in place of its parameters, so that a value of
    /// `page of (Text)` reads like a record whose `items` are
    /// `LIST Text`. `None` for any other type.
    pub fn instance<'s>(
        &'s self,
        r#type: &'s Type,
    ) -> Option<(&'s TypeDefinition, std::borrow::Cow<'s, TypeKind>)> {
        match self.underlying(r#type) {
            Type::Named(index) => {
                let definition = &self.types[*index];
                Some((definition, std::borrow::Cow::Borrowed(&definition.kind)))
            }
            Type::Generic(index, arguments) => {
                let definition = &self.types[*index];
                Some((
                    definition,
                    std::borrow::Cow::Owned(
                        definition
                            .kind
                            .substitute(&definition.parameters, arguments),
                    ),
                ))
            }
            _ => None,
        }
    }

    /// Whether a value of type `actual` is a value of type `expected`, so
    /// that it may be passed where `expected` is:
    ///
    /// - a refinement fits its base, `Integer WHERE [0 <= _]` fits
    ///   `Integer`, and another refinement only with the same predicate. A
    ///   base never fits its refinement: the type promises the predicate.
    /// - a member fits its union, and a union fits when each of its members
    ///   does. An alias of a union of variants is its variants.
    /// - `LIST`, `MAP`, tuples, a variant's slots and a generic record's
    ///   arguments are covariant: `LIST Depth` fits `LIST Integer`.
    /// - a record fits another when each of their fields with the same
    ///   words fits, and each field it lacks has a `DEFAULT`; a field the
    ///   other lacks never fits.
    /// - a named type fits only itself, by name.
    pub fn fits(&self, actual: &Type, expected: &Type) -> bool {
        self.fits_as(actual, expected, Fitting::Type, &mut Vec::new())
    }

    /// Whether a literal whose own type is `actual`, such as `Integer` for
    /// `3`, may be read as `expected`, before its value is checked against
    /// the predicates on the way: as [`Schema::fits`], except that a type
    /// also fits its refinements, since the value itself is checked.
    pub fn fits_literal(&self, actual: &Type, expected: &Type) -> bool {
        self.fits_as(actual, expected, Fitting::Literal, &mut Vec::new())
    }

    /// `assumed` holds the pairs of named types being compared further up,
    /// which fit unless something else doesn't: a record that holds a
    /// `LIST` of itself fits another that does.
    fn fits_as(
        &self,
        actual: &Type,
        expected: &Type,
        fitting: Fitting,
        assumed: &mut Vec<(Type, Type)>,
    ) -> bool {
        if actual == expected {
            return true;
        }
        let mut fits =
            |actual: &Type, expected: &Type| self.fits_as(actual, expected, fitting, assumed);
        match (actual, expected) {
            (Type::Union(members), _) => members.iter().all(|member| fits(member, expected)),
            (_, _) if self.variants_of(actual).is_some() => {
                let members = self.variants_of(actual).unwrap_or_default();
                members.iter().all(|member| fits(member, expected))
            }
            (Type::Refined(base, predicate), _) => {
                matches!(expected, Type::Refined(other, same) if same == predicate && fits(base, other))
                    || fits(base, expected)
            }
            (_, Type::Refined(base, _)) => fitting == Fitting::Literal && fits(actual, base),
            (_, Type::Union(members)) => members.iter().any(|member| fits(actual, member)),
            (_, _) if self.variants_of(expected).is_some() => {
                let members = self.variants_of(expected).unwrap_or_default();
                members.iter().any(|member| fits(actual, member))
            }
            (Type::List(actual), Type::List(expected))
            | (Type::Map(actual), Type::Map(expected)) => fits(actual, expected),
            (Type::Tuple(actual, _), Type::Tuple(expected, _)) => {
                actual.len() == expected.len()
                    && actual
                        .iter()
                        .zip(expected)
                        .all(|(actual, expected)| fits(actual, expected))
            }
            (Type::Variant(actual), Type::Variant(expected)) => {
                actual.text == expected.text
                    && actual
                        .parameters
                        .iter()
                        .zip(&expected.parameters)
                        .all(|(actual, expected)| fits(&actual.r#type, &expected.r#type))
            }
            (Type::Named(_) | Type::Generic(..), Type::Named(_) | Type::Generic(..)) => {
                let pair = (actual.clone(), expected.clone());
                if assumed.contains(&pair) {
                    return true;
                }
                let (Some((_, actual_kind)), Some((_, expected_kind))) =
                    (self.instance(actual), self.instance(expected))
                else {
                    return false;
                };
                match (actual_kind.as_ref(), expected_kind.as_ref()) {
                    (TypeKind::Record(actual_fields), TypeKind::Record(expected_fields)) => {
                        assumed.push(pair);
                        let fit = actual_fields.iter().all(|field| {
                            expected_fields.iter().any(|other| other.name == field.name)
                        }) && expected_fields.iter().all(|other| {
                            match actual_fields.iter().find(|field| field.name == other.name) {
                                Some(field) => {
                                    field.form == other.form
                                        && self.fits_as(
                                            &field.r#type,
                                            &other.r#type,
                                            fitting,
                                            assumed,
                                        )
                                }
                                None => other.default.is_some(),
                            }
                        });
                        assumed.pop();
                        fit
                    }
                    // A named type fits only itself, which `==` found.
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Whether the types can accept a common value. Refinements are compared
    /// by their bases because overload selection cannot rely on predicates.
    pub(crate) fn types_overlap(&self, a: &Type, b: &Type) -> bool {
        self.overlap(a, b, &mut Vec::new())
    }

    fn overlap(&self, a: &Type, b: &Type, records: &mut Vec<(Type, Type)>) -> bool {
        let (a, b) = (self.underlying(a), self.underlying(b));
        let mut overlaps = |a: &Type, b: &Type| self.overlap(a, b, records);
        if let Some(members) = self.variants_of(a) {
            return members.iter().any(|member| overlaps(member, b));
        }
        if let Some(members) = self.variants_of(b) {
            return members.iter().any(|member| overlaps(a, member));
        }
        match (a, b) {
            (Type::Union(members), _) => members.iter().any(|member| overlaps(member, b)),
            (_, Type::Union(members)) => members.iter().any(|member| overlaps(a, member)),
            (Type::List(_), Type::List(_)) | (Type::Map(_), Type::Map(_)) => true,
            (Type::Tuple(a, _), Type::Tuple(b, _)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| overlaps(a, b))
            }
            (Type::Variant(a), Type::Variant(b)) => a.forms().iter().any(|form| {
                b.forms()
                    .iter()
                    .filter(|other| form.text == other.text)
                    .any(|other| {
                        a.written_as(form)
                            .parameters
                            .iter()
                            .zip(&b.written_as(other).parameters)
                            .all(|(a, b)| overlaps(&a.r#type, &b.r#type))
                    })
            }),
            (Type::Named(_) | Type::Generic(..), Type::Named(_) | Type::Generic(..)) => {
                let (Some((_, a_kind)), Some((_, b_kind))) = (self.instance(a), self.instance(b))
                else {
                    return false;
                };
                let (TypeKind::Record(a_fields), TypeKind::Record(b_fields)) =
                    (a_kind.as_ref(), b_kind.as_ref())
                else {
                    return a == b;
                };
                let pair = (a.clone(), b.clone());
                if records.contains(&pair) {
                    // Requiring the same pair again cannot produce a finite value.
                    return false;
                }
                records.push(pair);
                let mut required_fields_fit = |fields: &[Field], others: &[Field]| {
                    fields
                        .iter()
                        .filter(|field| field.default.is_none())
                        .all(|field| {
                            others.iter().any(|other| {
                                field.form == other.form
                                    && self.overlap(&field.r#type, &other.r#type, records)
                            })
                        })
                };
                // A field with a default on both sides can be omitted.
                let shared = required_fields_fit(a_fields, b_fields)
                    && required_fields_fit(b_fields, a_fields);
                records.pop();
                shared
            }
            _ => a == b,
        }
    }

    /// The variants of an alias of a union of variants, as types: `Shade`,
    /// `` TYPE ALIAS Shade [| `dark, `light |] ``, is `` `dark `` and
    /// `` `light ``. `None` for any other type.
    fn variants_of(&self, r#type: &Type) -> Option<Vec<Type>> {
        if !matches!(r#type, Type::Named(_) | Type::Generic(..)) {
            return None;
        }
        match self.instance(r#type)?.1.as_ref() {
            TypeKind::Variants(variants) => Some(
                variants
                    .iter()
                    .map(|variant| Type::Variant(Box::new(variant.shape.clone())))
                    .collect(),
            ),
            _ => None,
        }
    }

    /// `type` without its refinements, which is how the wire and JSON write
    /// it: `Integer` for `Integer WHERE [0 <= _]`. A named type is written
    /// with its constructor, so it stays.
    pub fn underlying<'t>(&'t self, r#type: &'t Type) -> &'t Type {
        let mut underlying = r#type;
        while let Type::Refined(base, _) = underlying {
            underlying = base;
        }
        underlying
    }

    /// The type a value of `type` holds, past refinements and named types:
    /// `Text` for `File Id`, `NAMED TYPE File Id (value: Text)`. A refinement's
    /// predicate reads a value as this type.
    pub fn held_type<'t>(&'t self, r#type: &'t Type) -> &'t Type {
        let mut underlying = r#type;
        // The checker rejects named types that lead back to themselves, so
        // named transitions are bounded; inline refinements are finite trees.
        let mut named = 0;
        loop {
            match underlying {
                Type::Refined(base, _) => underlying = base,
                Type::Named(index) => {
                    named += 1;
                    if named > self.types.len() {
                        break;
                    }
                    match &self.types[*index].kind {
                        TypeKind::Distinct {
                            underlying: inner, ..
                        } => underlying = inner,
                        TypeKind::Record(_) | TypeKind::Variants(_) => break,
                    }
                }
                _ => break,
            }
        }
        underlying
    }

    /// `item` for the nullable `item?`, `` [| item, `null |] ``.
    pub fn nullable_item<'t>(&'t self, r#type: &'t Type) -> Option<&'t Type> {
        match self.underlying(r#type) {
            Type::Union(members) => match members.as_slice() {
                [item, Type::Variant(null)] if null.text == "null" => Some(item),
                _ => None,
            },
            _ => None,
        }
    }

    /// The field of `fields` that an entry of this call shape fills, as in
    /// `path _` or `_ case`; a variant is found by
    /// its shape instead, with [`Schema::unnamed_field`].
    pub fn shaped_field(&self, fields: &[Field], shape: &str) -> Option<usize> {
        fields.iter().position(|field| match &field.form {
            FieldForm::Shape(text) => shape == text,
            FieldForm::Unnamed => false,
        })
    }

    /// Finds the unnamed field when exactly one field accepts the variant.
    pub fn unnamed_field(&self, fields: &[Field], found: &Written) -> Option<usize> {
        self.resolve_unnamed_field(fields, found).ok().flatten()
    }

    /// Finds the unnamed field for a variant. If several fields share the
    /// variant, it can only be written by leaving the fields out.
    pub fn resolve_unnamed_field(
        &self,
        fields: &[Field],
        found: &Written,
    ) -> Result<Option<usize>, String> {
        let mut matches = fields.iter().enumerate().filter(|(_, field)| {
            field.form == FieldForm::Unnamed
                && self
                    .written_forms(&field.r#type)
                    .is_some_and(|forms| forms.iter().any(|known| known.admits(found)))
        });
        let Some((index, first)) = matches.next() else {
            return Ok(None);
        };
        let Some((_, second)) = matches.next() else {
            return Ok(Some(index));
        };
        let names = [first, second]
            .into_iter()
            .chain(matches.map(|(_, field)| field))
            .map(|field| code(&field.name))
            .collect::<Vec<_>>();
        Err(format!(
            "{found} could fill more than one unnamed field ({}): leave it out",
            names.join(", ")
        ))
    }

    /// The forms a value of `type` is written in: a variant by its shape,
    /// and other values by their kind. `None` for a type parameter, whose
    /// form isn't known.
    pub fn written_forms(&self, r#type: &Type) -> Option<Vec<Written>> {
        Some(match self.underlying(r#type) {
            Type::Text | Type::Bytes | Type::Instant => vec![Written::String],
            Type::Integer | Type::Float => vec![Written::Number],
            Type::Duration => self
                .intrinsic_functions
                .iter()
                .filter(|function| function.returns == Type::Duration)
                .flat_map(|function| {
                    function
                        .shape
                        .forms()
                        .into_iter()
                        .map(|form| Written::Call(function.shape.written_parts(&form.written)))
                })
                .collect(),
            Type::List(_) => vec![Written::List],
            Type::Map(_) => vec![Written::Braces],
            Type::Parameter(_) => return None,
            Type::Variant(shape) => shape.written_forms(),
            Type::Union(members) => {
                let mut forms = Vec::new();
                for member in members {
                    forms.extend(self.written_forms(member)?);
                }
                forms
            }
            underlying => match self.instance(underlying).map(|(_, kind)| kind) {
                Some(kind) => match kind.as_ref() {
                    TypeKind::Variants(variants) => variants
                        .iter()
                        .flat_map(|variant| variant.shape.written_forms())
                        .collect(),
                    TypeKind::Distinct { constructor, .. } => {
                        vec![Written::Call(constructor.clone())]
                    }
                    TypeKind::Record(_) => vec![Written::Braces],
                },
                None => vec![Written::Braces],
            },
        })
    }

    /// The method a request calls by `full_name`, such as `files/read _`,
    /// or `SCHEMA`, the built-in one.
    pub fn method(&self, full_name: &str) -> Option<&Method> {
        self.methods
            .iter()
            .chain(&self.built_ins)
            .find(|method| method.full_name() == full_name)
    }

    /// Whether another method has the same shape: an
    /// overload, which a call chooses by its arguments' types.
    pub fn is_overloaded(&self, method: &Method) -> bool {
        self.methods
            .iter()
            .filter(|other| other.shape.text == method.shape.text)
            .nth(1)
            .is_some()
    }

    /// `method`'s full name with each slot holding its parameter's type, as
    /// written in the flat namespace: `tag (LIST Text)`, `open (File Id)`.
    pub fn typed_name(&self, method: &Method) -> String {
        let types: Vec<String> = method
            .shape
            .parameters
            .iter()
            .map(|parameter| self.type_name(&parameter.r#type))
            .collect();
        fill_slots(&method.shape.text, &types)
    }

    /// What JSON and the generated TypeScript call `method` by: its full
    /// name, `tag _`, or its typed name, `tag (LIST Text)`, when it is
    /// overloaded.
    pub fn call_name(&self, method: &Method) -> String {
        match self.is_overloaded(method) {
            true => self.typed_name(method),
            false => method.full_name(),
        }
    }

    /// The index of the built-in `SCHEMA` call, one past the schema's own
    /// methods, as a [`crate::Call`]'s `method` holds it.
    pub fn introspection(&self) -> usize {
        self.methods.len()
    }

    /// The index of the built-in `SCHEMA FILES` call, after `SCHEMA`.
    pub fn schema_files(&self) -> usize {
        self.methods.len() + 1
    }

    /// The index of every method a call may name: the schema's own, then
    /// the built-in ones.
    pub fn calls(&self) -> std::ops::Range<usize> {
        0..self.methods.len() + self.built_ins.len()
    }

    /// The method at `index`: one of the schema's own, or a built-in one
    /// past them.
    pub fn method_at(&self, index: usize) -> &Method {
        self.methods
            .get(index)
            .unwrap_or_else(|| &self.built_ins[index - self.methods.len()])
    }

    /// The errors `method` may return.
    pub fn errors_of<'m>(&self, method: &'m Method) -> Vec<&'m Shaped> {
        method.errors.iter().collect()
    }
}

/// Lexical path normalization also works in WebAssembly without a filesystem.
fn normalize_path(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            ".." if path.starts_with('/') => {}
            _ => parts.push(part),
        }
    }
    format!(
        "{}{}",
        if path.starts_with('/') { "/" } else { "" },
        parts.join("/")
    )
}

struct Loader<'a, F> {
    read: &'a mut F,
    files: Vec<SourceFile>,
    decoded: Vec<DecodedFile>,
    visiting: Vec<String>,
    attempted: std::collections::BTreeSet<String>,
    diagnostics: Vec<Diagnostic>,
}

impl<E: std::fmt::Display, F: FnMut(&str) -> Result<String, E>> Loader<'_, F> {
    fn visit(&mut self, path: &str) -> Option<usize> {
        if let Some(index) = self.files.iter().position(|file| file.path == path) {
            return Some(index);
        }
        if !self.attempted.insert(path.to_string()) {
            return None;
        }
        let source = match (self.read)(path) {
            Ok(source) => source,
            Err(error) => {
                self.diagnostics.push(Diagnostic {
                    path: path.to_string(),
                    span: Span::empty(0),
                    message: format!("cannot read `{path}`: {error}"),
                });
                return None;
            }
        };
        let document = match decode_file(&SchemaFile {
            path,
            source: &source,
        }) {
            Ok(document) => document,
            Err(errors) => {
                self.diagnostics.extend(errors);
                return None;
            }
        };
        let imports: Vec<_> = document
            .definition
            .iter()
            .filter_map(|definition| {
                if let ast::Definition::Import(import) = definition {
                    Some(import.clone())
                } else {
                    None
                }
            })
            .collect();
        let index = self.files.len();
        self.files.push(SourceFile {
            path: path.to_string(),
            source,
        });
        self.decoded.push(DecodedFile {
            imports: Vec::new(),
            document,
        });
        self.visiting.push(path.to_string());
        for import in imports {
            let relative = &import.path.value.value;
            let error = |message| Diagnostic {
                path: path.to_string(),
                span: import.span,
                message,
            };
            if relative.is_empty()
                || relative.starts_with('/')
                || relative.contains('\\')
                || relative.contains(':')
            {
                self.diagnostics.push(error(format!(
                    "expected a relative file path in `USE`, found `{relative}`"
                )));
                continue;
            }
            let parent = path.rfind('/').map_or("", |slash| &path[..=slash]);
            let target = normalize_path(&format!("{parent}{relative}"));
            if let Some(start) = self.visiting.iter().position(|path| *path == target) {
                let mut cycle = self.visiting[start..].to_vec();
                cycle.push(target);
                self.diagnostics.push(error(format!(
                    "`USE \"{relative}\"` makes a circular import: {}",
                    cycle.join(" → ")
                )));
                continue;
            }
            let before = self.diagnostics.len();
            if let Some(imported) = self.visit(&target) {
                if self.decoded[imported].document.header.is_some() {
                    self.diagnostics.push(error(format!("cannot import `{target}`: it has a `PROTOCOL` header; move shared declarations into a headerless library")));
                } else if !self.decoded[index].imports.contains(&imported) {
                    self.decoded[index].imports.push(imported);
                }
            } else if self.diagnostics.len() > before {
                // Report read failures at USE; syntax errors keep their own file.
                if self.diagnostics[before].message.starts_with("cannot read") {
                    let message = self.diagnostics.remove(before).message;
                    self.diagnostics.push(error(message));
                }
            }
        }
        self.visiting.pop();
        Some(index)
    }
}

/// Parses `file` and decodes it with the schema language's grammar.
fn decode_file(file: &SchemaFile) -> Result<ast::Document, Vec<Diagnostic>> {
    let found = |span: Span, message: String| Diagnostic {
        path: file.path.to_string(),
        span,
        message,
    };
    let parsed = anyx::parse(file.source);
    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .into_iter()
            .map(|error| found(error.span, error.message))
            .collect());
    }
    let nodes = parsed.syntax.nodes();
    ast::decode_document(&nodes).map_err(|error| {
        if let Some(shape) = slashed_method(&nodes, error.span) {
            let name = &file.source[shape.start..shape.end];
            return vec![found(
                shape,
                format!(
                    "found the method `{name}`, with `/` in its name; expected words alone, method names are flat: write `METHOD read` and call it as `read`"
                ),
            )];
        }
        if let Some((span, intrinsic)) = old_built_in(&nodes, error.span) {
            return vec![found(
                span,
                format!(
                    "found `{old}`; expected `{new}`, its name since the built-in types are declared in the prelude: type names are written in Title Case, and all-caps words belong to the schema language",
                    old = &file.source[span.start..span.end],
                    new = intrinsic.name
                ),
            )];
        }
        // The definition the error is in, since `INTRINSIC METHOD` fails at
        // `METHOD`, after the `INTRINSIC` that caused it.
        let definition = nodes
            .iter()
            .find(|node| node.span.start <= error.span.start && error.span.start < node.span.end)
            .map_or(error.span, |node| node.span);
        if let Some(span) = word_within(&nodes, definition, "INTRINSIC") {
            return vec![found(
                span,
                "found `INTRINSIC` in a schema; expected it only in the prelude, which declares the built-in types, since the implementation supplies what an `INTRINSIC` declaration means; remove it".to_string(),
            )];
        }
        if let Some(part) = optional_part_without_words(&nodes, error.span) {
            return vec![found(
                part,
                "found an optional part with no words; expected a word in it before `DEFAULT`, as in `[in (paths: LIST Text) DEFAULT in [\".\"]]`, since slots side by side can't be told apart; add a word".to_string(),
            )];
        }
        if let Some(default) = default_outside_record(&nodes, error.span) {
            return vec![found(
                default,
                "found a `DEFAULT` outside a record; expected one only on a record field, as in `limit (Integer) DEFAULT limit 50`, since a parameter is always written".to_string(),
            )];
        }
        if let Some(default) = default_in_slot(&nodes, error.span) {
            return vec![found(
                default,
                "found a `DEFAULT` in a field's slot; expected it after the whole field, with the entry the field takes when left out, as in `age (Integer) DEFAULT age 0` or `(age: Integer) DEFAULT age 0`".to_string(),
            )];
        }
        let message = match (unnamed_return(&nodes, error.span), lone_error(&nodes, error.span)) {
            (true, _) => "found a `RETURNS` without a name; expected `RETURNS (name: Type)`, such as `RETURNS (new revision: Integer)`, since the name says what the value is".to_string(),
            (_, true) => "found an `ERROR` on its own; expected each error in the brackets of the method that has it, `` METHOD read (path: Text) RETURNS (file: File) ERRORS [| `missing |] ``, or of an alias of a union of variants that methods share by naming it there: `` TYPE ALIAS File Errors [| `trashed, `missing |] ``".to_string(),
            _ => error.message(),
        };
        vec![found(error.span, message)]
    })
}

/// The argument right after `word` in a top-level call that has it, if
/// `span` is inside it, such as the name of a `METHOD`. Only a slot that
/// comes straight after the word counts: in `METHOD _ RETURNS NOTHING
/// ERRORS _`, nothing comes after `RETURNS`, and the `ERRORS` list belongs
/// to `ERRORS`.
fn after_word<'a>(nodes: &'a [Node], span: Span, word: &str) -> Option<&'a Node> {
    nodes.iter().find_map(|node| {
        let NodeKind::Call(call) = &node.kind else {
            return None;
        };
        let mut slot = 0;
        call.name
            .iter()
            .enumerate()
            .find_map(|(index, part)| match part {
                NamePart::Slot => {
                    slot += 1;
                    None
                }
                NamePart::Word { text, .. }
                    if text == word && call.name.get(index + 1) == Some(&NamePart::Slot) =>
                {
                    call.arguments.get(slot).filter(|after| {
                        after.span.start <= span.start && span.end <= after.span.end
                    })
                }
                NamePart::Word { .. } => None,
            })
    })
}

/// Each word of a call's name within `span`, among `nodes` and the nodes
/// they hold, with its span.
fn words_within(nodes: &[Node], span: Span) -> Vec<(&str, Span)> {
    fn walk<'n>(node: &'n Node, span: Span, found: &mut Vec<(&'n str, Span)>) {
        if node.span.end <= span.start || span.end <= node.span.start {
            return;
        }
        let children: &[Node] = match &node.kind {
            NodeKind::Call(call) => {
                for part in &call.name {
                    if let NamePart::Word { text, span: at } = part
                        && span.start <= at.start
                        && at.end <= span.end
                    {
                        found.push((text, *at));
                    }
                }
                &call.arguments
            }
            NodeKind::Parens(nodes)
            | NodeKind::Brackets(nodes)
            | NodeKind::Braces(nodes)
            | NodeKind::CustomBracket(_, nodes) => nodes,
            _ => &[],
        };
        children.iter().for_each(|child| walk(child, span, found));
    }
    let mut found = Vec::new();
    nodes.iter().for_each(|node| walk(node, span, &mut found));
    found.sort_by_key(|(_, at)| at.start);
    found
}

/// The first all-caps name of a built-in type from before the prelude,
/// such as `INTEGER`, within `span`, and the type it is now.
fn old_built_in(nodes: &[Node], span: Span) -> Option<(Span, &'static crate::prelude::Intrinsic)> {
    words_within(nodes, span)
        .into_iter()
        .find_map(|(text, at)| crate::prelude::renamed(text).map(|intrinsic| (at, intrinsic)))
}

/// Where `word` first is within `span`.
fn word_within(nodes: &[Node], span: Span, word: &str) -> Option<Span> {
    words_within(nodes, span)
        .into_iter()
        .find(|(text, _)| *text == word)
        .map(|(_, at)| at)
}

/// Whether `span` is inside what a `METHOD` returns, written without a name:
/// `RETURNS (Text)` rather than `RETURNS (text: Text)`.
fn unnamed_return(nodes: &[Node], span: Span) -> bool {
    after_word(nodes, span, "RETURNS").is_some_and(|returned| {
        !matches!(&returned.kind, NodeKind::Call(call)
            if call.name.iter().any(|part| matches!(part, NamePart::Word { text, .. } if !anyx::is_keyword(text)))
                && call.name.contains(&NamePart::Slot))
    })
}

/// Where the optional part that `span` is in is, when it holds a
/// `DEFAULT` but no word before it: `[(factor: Integer) DEFAULT 1]`.
fn optional_part_without_words(nodes: &[Node], span: Span) -> Option<Span> {
    fn within(node: &Node, span: Span) -> Option<Span> {
        if !(node.span.start <= span.start && span.end <= node.span.end) {
            return None;
        }
        if let NodeKind::Brackets(items) = &node.kind
            && let [item] = &items[..]
            && let NodeKind::Call(call) = &item.kind
            && matches!(call.name.first(), Some(NamePart::Slot))
            && call
                .name
                .iter()
                .any(|part| matches!(part, NamePart::Word { text, .. } if text == "DEFAULT"))
            && call
                .name
                .iter()
                .take_while(
                    |part| !matches!(part, NamePart::Word { text, .. } if text == "DEFAULT"),
                )
                .all(|part| matches!(part, NamePart::Slot))
        {
            return Some(node.span);
        }
        let children: &[Node] = match &node.kind {
            NodeKind::Call(call) => &call.arguments,
            NodeKind::Parens(nodes)
            | NodeKind::Brackets(nodes)
            | NodeKind::Braces(nodes)
            | NodeKind::CustomBracket(_, nodes) => nodes,
            _ => &[],
        };
        children.iter().find_map(|child| within(child, span))
    }
    nodes.iter().find_map(|node| within(node, span))
}

fn default_outside_record(nodes: &[Node], span: Span) -> Option<Span> {
    fn has_word(node: &Node, word: &str) -> bool {
        matches!(&node.kind, NodeKind::Call(call)
            if call.name.iter().any(|part| matches!(part, NamePart::Word { text, .. } if text == word)))
    }
    fn within(node: &Node, span: Span) -> Option<Span> {
        if !(node.span.start <= span.start && span.end <= node.span.end) {
            return None;
        }
        let children: &[Node] = match &node.kind {
            NodeKind::Call(call) => &call.arguments,
            NodeKind::Parens(nodes)
            | NodeKind::Brackets(nodes)
            | NodeKind::CustomBracket(_, nodes) => nodes,
            NodeKind::Braces(_) => return None,
            _ => &[],
        };
        children
            .iter()
            .find_map(|child| within(child, span))
            .or_else(|| has_word(node, "DEFAULT").then_some(node.span))
    }
    nodes.iter().find_map(|node| within(node, span))
}

/// Where the call with a `DEFAULT` that `span` is in is, when it fills a
/// slot of a record field, as in `age: Integer DEFAULT age 0`, which is
/// `age (Integer DEFAULT age 0)`.
fn default_in_slot(nodes: &[Node], span: Span) -> Option<Span> {
    fn has_default(node: &Node) -> bool {
        matches!(&node.kind, NodeKind::Call(call)
            if call.name.iter().any(|part| matches!(part, NamePart::Word { text, .. } if text == "DEFAULT")))
    }
    fn within(node: &Node, span: Span, in_call: bool) -> Option<Span> {
        if !(node.span.start <= span.start && span.end <= node.span.end) {
            return None;
        }
        let (children, call): (&[Node], bool) = match &node.kind {
            NodeKind::Call(call) => (&call.arguments, true),
            NodeKind::Parens(nodes) => (nodes, in_call),
            NodeKind::Brackets(nodes)
            | NodeKind::Braces(nodes)
            | NodeKind::CustomBracket(_, nodes) => (nodes, false),
            _ => (&[], false),
        };
        children
            .iter()
            .find_map(|child| within(child, span, call))
            .or_else(|| (in_call && has_default(node)).then_some(node.span))
    }
    nodes.iter().find_map(|node| within(node, span, false))
}

/// Whether `span` is in a top-level `ERROR`, which a schema no longer has:
/// errors are defined in the brackets of a method or an `ERRORS` group.
fn lone_error(nodes: &[Node], span: Span) -> bool {
    nodes.iter().any(|node| {
        node.span.start <= span.start
            && span.end <= node.span.end
            && matches!(&node.kind, NodeKind::Call(call)
                if call.name.iter().any(|part| matches!(part, NamePart::Word { text, .. } if text == "ERROR")))
    })
}

/// Where the name of the `METHOD` that `span` is in is, if it has a `/`, as
/// in `METHOD files/read (path: Text)`.
fn slashed_method(nodes: &[Node], span: Span) -> Option<Span> {
    let shape = after_word(nodes, span, "METHOD")?;
    let NodeKind::Call(call) = &shape.kind else {
        return None;
    };
    call.name
        .iter()
        .any(|part| matches!(part, NamePart::Word { text, .. } if text == "/"))
        .then_some(shape.span)
}

/// Something wrong in a schema, and where.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    /// The normalized path of the source file.
    pub path: String,
    pub span: Span,
    pub message: String,
}

impl Diagnostic {
    /// `file:line:column: message`, then the line with the span underlined.
    /// `source` is the file at [`Diagnostic::path`].
    pub fn render(&self, source: &str, file: &str) -> String {
        let position = line_column(source, self.span.start);
        let line = source.lines().nth(position.line - 1).unwrap_or("");
        let start = self.span.start.min(source.len());
        let width = source[start..self.span.end.clamp(start, source.len())]
            .lines()
            .next()
            .map_or(1, |text| text.chars().count().max(1));
        format!(
            "{file}:{}:{}: {}\n  {line}\n  {}{}",
            position.line,
            position.column,
            self.message,
            " ".repeat(position.column - 1),
            "^".repeat(width)
        )
    }
}
