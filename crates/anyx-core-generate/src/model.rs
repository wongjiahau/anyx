//! What the generators read from a checked schema: its data types, its
//! served functions and its bodied functions, with every type normalized.
//!
//! The rules are about the core's own constructs, never about what a schema
//! means by them:
//!
//! - A family's parameters become generic parameters and its indices are
//!   dropped, because indices only constrain which values fit.
//! - A binder of `Type` is erased: types don't exist at run time.
//! - An empty `DATA` outside the prelude is opaque: whatever serves the
//!   schema chooses its representation.
//! - A function with a parameter of an indexed family is split into one
//!   case per constructor of that family. Fixing the constructor solves the
//!   parameters its indices mention, and a parameter whose type then fits
//!   exactly one constructor is replaced by that constructor's fields.

use crate::Error;
use anyx_core::kernel::{self, Index, Term, TypeChecker};
use anyx_core::lowering::{self, GlobalKind};
use std::collections::HashMap;

/// A type the generated code can spell.
#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Primitive(Primitive),
    List(Box<Ty>),
    Optional(Box<Ty>),
    Data {
        name: String,
        arguments: Vec<Ty>,
    },
    /// A generic parameter of the data type being declared.
    Variable(String),
    Opaque(String),
    /// A type that is only known at run time, such as a solved index.
    Erased,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primitive {
    Text,
    Integer,
    Instant,
    Duration,
    Boolean,
    Nothing,
    Never,
}

/// The prelude: data types every target has its own spelling of.
fn primitive(name: &str) -> Option<Primitive> {
    Some(match name {
        "Text" => Primitive::Text,
        "Integer" => Primitive::Integer,
        "Instant" => Primitive::Instant,
        "Duration" => Primitive::Duration,
        "Boolean" => Primitive::Boolean,
        "Nothing" => Primitive::Nothing,
        "Never" => Primitive::Never,
        _ => return None,
    })
}

const LIST: &str = "List of _";
const OPTIONAL: &str = "Optional _";

pub(crate) fn is_prelude(name: &str) -> bool {
    primitive(name).is_some() || name == LIST || name == OPTIONAL
}

pub struct Data {
    pub name: String,
    /// The names of the generic parameters, one per parameter of `Type`.
    pub generics: Vec<String>,
    /// The family positions those generics stand for.
    generic_positions: Vec<usize>,
    /// Whether some argument of the family varies between constructors.
    pub indexed: bool,
    pub opaque: bool,
    /// Written as a `RECORD`: sent as its fields alone, without `kind`.
    pub record: bool,
    pub constructors: Vec<Constructor>,
    /// The opaque types a field mentions, which the type is generic over.
    pub opaque_fields: Vec<String>,
}

pub struct Constructor {
    pub name: String,
    pub data: String,
    pub binders: Vec<Binder>,
    /// The family's arguments in the constructor's result, normalized, in
    /// scope of the binders.
    result_arguments: Vec<Term>,
}

pub struct Binder {
    pub name: String,
    /// `None` for an erased binder of `Type`.
    pub ty: Option<Ty>,
    /// The binder's type, in scope of the binders before it.
    kernel_type: Term,
}

impl Constructor {
    pub fn fields(&self) -> impl Iterator<Item = (&str, &Ty)> {
        self.binders
            .iter()
            .filter_map(|binder| Some((binder.name.as_str(), binder.ty.as_ref()?)))
    }

    pub fn erased(&self, position: usize) -> bool {
        self.binders[position].ty.is_none()
    }

    /// The constructor's name without the words that lead from a slot of
    /// `Type` to the next slot: `Answered` for `answered _ or _ with _`.
    pub fn variant(&self) -> String {
        crate::names::pascal_case(&self.kind())
    }

    /// The constructor's words as JSON names it under `kind`, without slots.
    pub fn kind(&self) -> String {
        let mut words = Vec::new();
        let mut slot = 0;
        let mut skipping = false;
        for word in self.name.split_whitespace() {
            if word == "_" {
                skipping = self
                    .binders
                    .get(slot)
                    .is_some_and(|binder| binder.ty.is_none());
                slot += 1;
            } else if !skipping {
                words.push(word);
            }
        }
        words.join(" ")
    }
}

pub struct Parameter {
    pub name: String,
    pub erased: bool,
    kernel_type: Term,
}

pub struct Function {
    pub name: String,
    pub parameters: Vec<Parameter>,
    result: Term,
    /// `None` for a served function.
    pub body: Option<lowering::Term>,
    /// The parameter bindings, for naming locals in the body.
    pub bindings: Vec<lowering::BindingIdentifier>,
    pub split: Option<Split>,
}

/// One case of a function per constructor of the parameter at `position`.
pub struct Split {
    pub position: usize,
    pub data: String,
    pub cases: Vec<Case>,
}

pub struct Case {
    pub constructor: String,
    pub parameters: Vec<Argument>,
    pub result: Ty,
}

/// What becomes of each parameter once the constructor is fixed.
pub enum Argument {
    Erased,
    /// Its value follows from the constructor.
    Solved(Term),
    /// The split parameter: the constructor's fields.
    Split,
    /// Its type fits one constructor: that constructor's fields.
    Collapsed {
        constructor: String,
        fields: Vec<(String, Ty)>,
    },
    Plain {
        ty: Ty,
        /// The constructors that fit, when the type is an indexed family.
        fitting: Option<Vec<String>>,
    },
}

pub struct Schema {
    pub(crate) checker: TypeChecker,
    pub datas: Vec<Data>,
    pub functions: Vec<Function>,
    data_index: HashMap<String, usize>,
    constructor_index: HashMap<String, (usize, usize)>,
}

impl Schema {
    pub fn data(&self, name: &str) -> &Data {
        &self.datas[self.data_index[name]]
    }

    pub fn constructor(&self, name: &str) -> &Constructor {
        let (data, constructor) = self.constructor_index[name];
        &self.datas[data].constructors[constructor]
    }

    pub fn function(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|function| function.name == name)
    }

    pub fn served(&self) -> impl Iterator<Item = &Function> {
        self.functions
            .iter()
            .filter(|function| function.body.is_none())
    }

    /// Bodied functions whose results are values, not types.
    pub fn bodied(&self) -> impl Iterator<Item = &Function> {
        self.functions
            .iter()
            .filter(|function| function.body.is_some())
    }

    /// Bodied functions a client calls: those split by a parameter.
    pub fn entries(&self) -> impl Iterator<Item = &Function> {
        self.bodied().filter(|function| function.split.is_some())
    }
}

pub(crate) fn build(
    checker: TypeChecker,
    kernel: &kernel::Document,
    lowered: &lowering::Document,
) -> Result<Schema, Error> {
    let mut schema = Schema {
        checker,
        datas: Vec::new(),
        functions: Vec::new(),
        data_index: HashMap::new(),
        constructor_index: HashMap::new(),
    };

    // The shape of each data type comes first, so field types can name any.
    let mut pending = Vec::new();
    for (declaration, lowered) in kernel.declarations.iter().zip(&lowered.declarations) {
        let (
            kernel::Declaration::Data {
                signature,
                constructors,
            },
            lowering::Declaration::Data {
                signature: lowered_signature,
                constructors: lowered_constructors,
                record,
            },
        ) = (declaration, lowered)
        else {
            continue;
        };
        let arity = signature.parameter_types.len();
        let mut results = Vec::new();
        for constructor in constructors {
            let environment = vec![None; constructor.parameter_types.len()];
            let result = schema
                .checker
                .normalize_in(&constructor.result, &environment)
                .map_err(|error| Error::new(format!("`{}`: {error}", constructor.name)))?;
            results.push(spine(&result).1);
        }
        let binders = kernel::parameter_binders(signature, constructors);
        let mut generics = Vec::new();
        let mut generic_positions = Vec::new();
        let mut indexed = false;
        for position in 0..arity {
            let uniform = constructors.iter().enumerate().all(|(index, constructor)| {
                matches!(results[index].get(position), Some(Term::Local(Index(local)))
                    if binders[index].contains(&(constructor.parameter_types.len() - 1 - local)))
            });
            if uniform && signature.parameter_types[position] == Term::Type {
                generics.push(crate::names::pascal_case(
                    lowered_signature.parameters[position]
                        .binding
                        .name
                        .as_deref()
                        .unwrap_or("item"),
                ));
                generic_positions.push(position);
            } else if !uniform {
                indexed = true;
            }
        }
        schema
            .data_index
            .insert(signature.name.clone(), schema.datas.len());
        for (position, constructor) in constructors.iter().enumerate() {
            schema
                .constructor_index
                .insert(constructor.name.clone(), (schema.datas.len(), position));
        }
        schema.datas.push(Data {
            name: signature.name.clone(),
            generics,
            generic_positions,
            indexed,
            opaque: constructors.is_empty() && !is_prelude(&signature.name),
            record: *record,
            constructors: Vec::new(),
            opaque_fields: Vec::new(),
        });
        pending.push((constructors, lowered_constructors, results));
    }

    for (data, (constructors, lowered_constructors, results)) in pending.into_iter().enumerate() {
        let mut built = Vec::new();
        for ((constructor, lowered), result_arguments) in
            constructors.iter().zip(lowered_constructors).zip(results)
        {
            // Which family parameter each binder passes, if any.
            let mut generic_of = HashMap::new();
            let depth = constructor.parameter_types.len();
            for (generic, position) in schema.datas[data]
                .generics
                .iter()
                .zip(&schema.datas[data].generic_positions)
            {
                if let Some(Term::Local(Index(local))) = result_arguments.get(*position) {
                    generic_of.insert(depth - 1 - local, generic.clone());
                }
            }
            let mut binders = Vec::new();
            let mut scope = Vec::new();
            for (position, (parameter_type, parameter)) in constructor
                .parameter_types
                .iter()
                .zip(&lowered.parameters)
                .enumerate()
            {
                let name = parameter.binding.name.clone().unwrap_or_default();
                let kernel_type = schema
                    .checker
                    .normalize_in(parameter_type, &vec![None; position])
                    .map_err(|error| Error::new(format!("`{}`: {error}", constructor.name)))?;
                let ty = if kernel_type == Term::Type {
                    scope.push(match generic_of.get(&position) {
                        Some(generic) => Scope::Type(Ty::Variable(generic.clone())),
                        None => Scope::Type(Ty::Erased),
                    });
                    None
                } else {
                    let ty = schema.ty(&kernel_type, &scope).map_err(|error| {
                        Error::new(format!("field `{name}` of `{}`: {error}", constructor.name))
                    })?;
                    scope.push(Scope::Value);
                    Some(ty)
                };
                binders.push(Binder {
                    name,
                    ty,
                    kernel_type,
                });
            }
            built.push(Constructor {
                name: constructor.name.clone(),
                data: schema.datas[data].name.clone(),
                binders,
                result_arguments,
            });
        }
        let mut opaque_fields = Vec::new();
        for constructor in &built {
            for (_, ty) in constructor.fields() {
                collect_opaque(ty, &mut opaque_fields);
            }
        }
        schema.datas[data].constructors = built;
        schema.datas[data].opaque_fields = opaque_fields;
    }

    for (declaration, lowered) in kernel.declarations.iter().zip(&lowered.declarations) {
        let (signature, lowered_signature, body) = match (declaration, lowered) {
            (
                kernel::Declaration::Function { signature, .. },
                lowering::Declaration::Function {
                    signature: lowered_signature,
                    body,
                },
            ) => (signature, lowered_signature, Some(body)),
            (
                kernel::Declaration::Served { signature },
                lowering::Declaration::Served {
                    signature: lowered_signature,
                },
            ) => (signature, lowered_signature, None),
            _ => continue,
        };
        // A function that computes a type is an alias, already unfolded.
        if matches!(signature.result, Term::Type | Term::Kind) {
            continue;
        }
        let parameters: Vec<Parameter> = signature
            .parameter_types
            .iter()
            .zip(&lowered_signature.parameters)
            .map(|(parameter_type, parameter)| Parameter {
                name: parameter.binding.name.clone().unwrap_or_default(),
                erased: *parameter_type == Term::Type,
                kernel_type: parameter_type.clone(),
            })
            .collect();
        let split = schema
            .split(&parameters, &signature.result)
            .map_err(|error| Error::new(format!("`{}`: {error}", signature.name)))?;
        schema.functions.push(Function {
            name: signature.name.clone(),
            parameters,
            result: signature.result.clone(),
            body: body.map(clone_term),
            bindings: lowered_signature
                .parameters
                .iter()
                .map(|parameter| parameter.binding.identifier)
                .collect(),
            split,
        });
    }
    for data in &schema.datas {
        let mut kinds: HashMap<String, &str> = HashMap::new();
        for constructor in &data.constructors {
            if let Some(other) = kinds.insert(constructor.kind(), &constructor.name) {
                return Err(Error::new(format!(
                    "`{other}` and `{}` would both be sent as `{}`",
                    constructor.name,
                    constructor.kind()
                )));
            }
        }
    }
    let mut spelled: HashMap<String, &str> = HashMap::new();
    for data in &schema.datas {
        if let Some(other) = spelled.insert(crate::names::type_name(&data.name), &data.name) {
            return Err(Error::new(format!(
                "`{other}` and `{}` would both be called `{}`",
                data.name,
                crate::names::type_name(&data.name)
            )));
        }
    }
    Ok(schema)
}

#[derive(Clone)]
enum Scope {
    Type(Ty),
    Value,
}

fn collect_opaque(ty: &Ty, found: &mut Vec<String>) {
    match ty {
        Ty::Opaque(name) if !found.contains(name) => found.push(name.clone()),
        Ty::List(item) | Ty::Optional(item) => collect_opaque(item, found),
        Ty::Data { arguments, .. } => {
            for argument in arguments {
                collect_opaque(argument, found);
            }
        }
        _ => {}
    }
}

/// The head of an application and its arguments in written order.
pub(crate) fn spine(term: &Term) -> (&Term, Vec<Term>) {
    let mut head = term;
    let mut spines = Vec::new();
    while let Term::Apply {
        function,
        arguments,
    } = head
    {
        spines.push(arguments);
        head = function;
    }
    (head, spines.into_iter().rev().flatten().cloned().collect())
}

fn global_name(term: &Term) -> Option<(&str, GlobalKind)> {
    match term {
        Term::Global(global) => Some((&global.name, global.kind)),
        _ => None,
    }
}

impl Schema {
    /// Spell a normalized type, with `scope` giving each local outermost first.
    fn ty(&self, term: &Term, scope: &[Scope]) -> Result<Ty, String> {
        match term {
            Term::Type | Term::Kind => Ok(Ty::Erased),
            Term::Local(Index(index)) => match scope.get(scope.len().wrapping_sub(1 + index)) {
                Some(Scope::Type(ty)) => Ok(ty.clone()),
                _ => Err("a type that depends on a value has no spelling yet".into()),
            },
            Term::Global(_) | Term::Apply { .. } => {
                let (head, arguments) = spine(term);
                let Some((name, GlobalKind::Data)) = global_name(head) else {
                    return Err("only data types can be spelled".into());
                };
                if let Some(primitive) = primitive(name) {
                    return Ok(Ty::Primitive(primitive));
                }
                if name == LIST || name == OPTIONAL {
                    let item = self.ty(&arguments[0], scope)?;
                    return Ok(if name == LIST {
                        Ty::List(Box::new(item))
                    } else {
                        Ty::Optional(Box::new(item))
                    });
                }
                let data = self.data(name);
                if data.opaque {
                    return Ok(Ty::Opaque(name.into()));
                }
                Ok(Ty::Data {
                    name: name.into(),
                    arguments: data
                        .generic_positions
                        .iter()
                        .map(|position| self.ty(&arguments[*position], scope))
                        .collect::<Result<_, _>>()?,
                })
            }
            Term::FunctionType { .. } => Err("function types have no spelling yet".into()),
            _ => Err("this type has no spelling".into()),
        }
    }

    /// The type of a function's parameter or result, with its parameters of
    /// `Type` erased.
    pub fn erased_ty(&self, function: &Function, position: Option<usize>) -> Result<Ty, Error> {
        let (term, depth) = match position {
            Some(position) => (&function.parameters[position].kernel_type, position),
            None => (&function.result, function.parameters.len()),
        };
        let normal = self
            .checker
            .normalize_in(term, &vec![None; depth])
            .map_err(|error| Error::new(format!("`{}`: {error}", function.name)))?;
        let scope: Vec<Scope> = function.parameters[..depth]
            .iter()
            .map(|parameter| {
                if parameter.erased {
                    Scope::Type(Ty::Erased)
                } else {
                    Scope::Value
                }
            })
            .collect();
        self.ty(&normal, &scope)
            .map_err(|error| Error::new(format!("`{}`: {error}", function.name)))
    }

    fn split(&self, parameters: &[Parameter], result: &Term) -> Result<Option<Split>, String> {
        for (position, parameter) in parameters.iter().enumerate() {
            if parameter.erased {
                continue;
            }
            let normal = self
                .checker
                .normalize_in(&parameter.kernel_type, &vec![None; position])
                .map_err(|error| error.to_string())?;
            let (head, arguments) = spine(&normal);
            let Some((name, GlobalKind::Data)) = global_name(head) else {
                continue;
            };
            if is_prelude(name) || !self.data(name).indexed {
                continue;
            }
            let solves = arguments
                .iter()
                .any(|argument| matches!(argument, Term::Local(Index(local)) if *local < position));
            if !solves {
                continue;
            }
            let mut cases = Vec::new();
            for constructor in &self.data(name).constructors {
                cases.push(self.case(parameters, result, position, &arguments, constructor)?);
            }
            return Ok(Some(Split {
                position,
                data: name.into(),
                cases,
            }));
        }
        Ok(None)
    }

    fn case(
        &self,
        parameters: &[Parameter],
        result: &Term,
        split: usize,
        split_arguments: &[Term],
        constructor: &Constructor,
    ) -> Result<Case, String> {
        let mut environment: Vec<Option<Term>> = vec![None; parameters.len()];
        for (argument, index) in split_arguments.iter().zip(&constructor.result_arguments) {
            if let Term::Local(Index(local)) = argument {
                if !closed(index) {
                    return Err(format!(
                        "an index of `{}` depends on its fields, which has no spelling yet",
                        constructor.name
                    ));
                }
                environment[split - 1 - local] = Some(index.clone());
            }
        }
        let mut arguments = Vec::new();
        for (position, parameter) in parameters.iter().enumerate() {
            if position == split {
                arguments.push(Argument::Split);
                continue;
            }
            if parameter.erased {
                arguments.push(Argument::Erased);
                continue;
            }
            if let Some(value) = &environment[position] {
                arguments.push(Argument::Solved(value.clone()));
                continue;
            }
            let normal = self
                .checker
                .normalize_in(&parameter.kernel_type, &environment[..position])
                .map_err(|error| error.to_string())?;
            arguments.push(self.argument(&normal)?);
        }
        let normal = self
            .checker
            .normalize_in(result, &environment)
            .map_err(|error| error.to_string())?;
        Ok(Case {
            constructor: constructor.name.clone(),
            parameters: arguments,
            result: self.ty(&normal, &[])?,
        })
    }

    fn argument(&self, normal: &Term) -> Result<Argument, String> {
        let ty = self.ty(normal, &[])?;
        let (head, arguments) = spine(normal);
        let Some((name, GlobalKind::Data)) = global_name(head) else {
            return Ok(Argument::Plain { ty, fitting: None });
        };
        if is_prelude(name) || !self.data(name).indexed {
            return Ok(Argument::Plain { ty, fitting: None });
        }
        let mut fitting = Vec::new();
        for constructor in &self.data(name).constructors {
            if let Some(solved) = unify(&constructor.result_arguments, &arguments, constructor) {
                fitting.push((constructor, solved));
            }
        }
        if let [(constructor, solved)] = fitting.as_slice() {
            let mut fields = Vec::new();
            let mut environment = Vec::new();
            let mut scope = Vec::new();
            for (position, binder) in constructor.binders.iter().enumerate() {
                let given = solved[position].clone();
                if binder.ty.is_some() {
                    let normal = self
                        .checker
                        .normalize_in(&binder.kernel_type, &environment)
                        .map_err(|error| error.to_string())?;
                    fields.push((binder.name.clone(), self.ty(&normal, &scope)?));
                }
                scope.push(match &given {
                    Some(term) if binder.ty.is_none() => Scope::Type(self.ty(term, &[])?),
                    _ => Scope::Value,
                });
                environment.push(given);
            }
            return Ok(Argument::Collapsed {
                constructor: constructor.name.clone(),
                fields,
            });
        }
        Ok(Argument::Plain {
            ty,
            fitting: Some(
                fitting
                    .iter()
                    .map(|(constructor, _)| constructor.name.clone())
                    .collect(),
            ),
        })
    }
}

fn closed(term: &Term) -> bool {
    match term {
        Term::Local(_) => false,
        Term::Apply {
            function,
            arguments,
        } => closed(function) && arguments.iter().all(closed),
        Term::Type | Term::Kind | Term::Global(_) => true,
        _ => false,
    }
}

/// Match a constructor's result arguments, where its binders are variables,
/// against closed arguments. The binders it solves, outermost first.
fn unify(pattern: &[Term], given: &[Term], constructor: &Constructor) -> Option<Vec<Option<Term>>> {
    let depth = constructor.binders.len();
    let mut solved = vec![None; depth];
    fn go(pattern: &Term, given: &Term, depth: usize, solved: &mut [Option<Term>]) -> bool {
        match pattern {
            Term::Local(Index(local)) => {
                solved[depth - 1 - local] = Some(given.clone());
                true
            }
            Term::Apply { .. } | Term::Global(_) => {
                let (pattern_head, pattern_arguments) = spine(pattern);
                let (given_head, given_arguments) = spine(given);
                pattern_head == given_head
                    && pattern_arguments.len() == given_arguments.len()
                    && pattern_arguments
                        .iter()
                        .zip(&given_arguments)
                        .all(|(pattern, given)| go(pattern, given, depth, solved))
            }
            _ => pattern == given,
        }
    }
    pattern
        .iter()
        .zip(given)
        .all(|(pattern, given)| go(pattern, given, depth, &mut solved))
        .then_some(solved)
}

/// Lowered terms aren't `Clone`; a body is copied once, when it's read.
fn clone_term(term: &lowering::Term) -> lowering::Term {
    use lowering::Term as T;
    match term {
        T::Type => T::Type,
        T::Kind => T::Kind,
        T::Local(identifier) => T::Local(*identifier),
        T::Global { name, kind } => T::Global {
            name: name.clone(),
            kind: *kind,
        },
        T::FunctionType { parameter, result } => T::FunctionType {
            parameter: clone_parameter(parameter),
            result: Box::new(clone_term(result)),
        },
        T::Function { parameter, body } => T::Function {
            parameter: clone_binding(parameter),
            body: Box::new(clone_term(body)),
        },
        T::Apply {
            function,
            arguments,
        } => T::Apply {
            function: Box::new(clone_term(function)),
            arguments: arguments.iter().map(clone_term).collect(),
        },
        T::Match { subject, cases } => T::Match {
            subject: Box::new(clone_term(subject)),
            cases: cases
                .iter()
                .map(|case| lowering::Case {
                    pattern: clone_pattern(&case.pattern),
                    result: clone_term(&case.result),
                })
                .collect(),
        },
        T::List(elements) => T::List(elements.iter().map(clone_term).collect()),
    }
}

fn clone_binding(binding: &lowering::Binding) -> lowering::Binding {
    lowering::Binding {
        identifier: binding.identifier,
        name: binding.name.clone(),
    }
}

fn clone_parameter(parameter: &lowering::Parameter) -> lowering::Parameter {
    lowering::Parameter {
        binding: clone_binding(&parameter.binding),
        annotation: Box::new(clone_term(&parameter.annotation)),
    }
}

fn clone_pattern(pattern: &lowering::Pattern) -> lowering::Pattern {
    match pattern {
        lowering::Pattern::Constructor { name, arguments } => lowering::Pattern::Constructor {
            name: name.clone(),
            arguments: arguments.iter().map(clone_pattern).collect(),
        },
        lowering::Pattern::Binding(binding) => lowering::Pattern::Binding(clone_binding(binding)),
    }
}
