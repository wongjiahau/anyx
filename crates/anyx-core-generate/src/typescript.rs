//! The TypeScript client of a schema: its types, a JSON codec that restores
//! instants as `Date` (integers and durations stay JSON numbers, as the
//! old protocol's clients had them), and a `Client` with one
//! method per constructor of each entry point's split parameter.

use crate::Error;
use crate::model::{Argument, Primitive, Schema, Ty};
use crate::names::{camel_case, quoted, type_name};
use std::fmt::Write;

pub fn typescript(schema: &Schema) -> Result<String, Error> {
    let mut out = String::new();
    out.push_str("// Generated from an anyx core schema. Don't edit.\n\n");
    for data in &schema.datas {
        if crate::model::is_prelude(&data.name) || data.opaque || !data.opaque_fields.is_empty() {
            continue;
        }
        declare_type(&mut out, data);
    }
    out.push_str(CODEC);
    declare_descriptors(schema, &mut out);
    declare_client(schema, &mut out)?;
    Ok(out)
}

fn ty(ty: &Ty) -> String {
    match ty {
        Ty::Primitive(primitive) => match primitive {
            Primitive::Text => "string".into(),
            Primitive::Integer | Primitive::Duration => "number".into(),
            Primitive::Instant => "Date".into(),
            Primitive::Boolean => "boolean".into(),
            Primitive::Nothing => "null".into(),
            Primitive::Never => "never".into(),
        },
        Ty::List(item) => format!("{}[]", wrap(item)),
        Ty::Optional(item) => format!("{} | null", self::ty(item)),
        Ty::Data { name, arguments } if arguments.is_empty() => type_name(name),
        Ty::Data { name, arguments } => format!(
            "{}<{}>",
            type_name(name),
            arguments
                .iter()
                .map(self::ty)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Ty::Variable(name) => name.clone(),
        Ty::Opaque(_) | Ty::Erased => "unknown".into(),
    }
}

fn wrap(item: &Ty) -> String {
    let spelled = ty(item);
    if matches!(item, Ty::Optional(_)) {
        format!("({spelled})")
    } else {
        spelled
    }
}

fn declare_type(out: &mut String, data: &crate::model::Data) {
    let generics = if data.generics.is_empty() {
        String::new()
    } else {
        format!("<{}>", data.generics.join(", "))
    };
    let _ = writeln!(
        out,
        "/** `{}`. */\nexport type {}{generics} =",
        data.name,
        type_name(&data.name)
    );
    for constructor in &data.constructors {
        let mut fields = vec![format!("kind: {}", quoted(&constructor.kind()))];
        for (name, field_ty) in constructor.fields() {
            fields.push(format!("{}: {}", camel_case(name), ty(field_ty)));
        }
        let _ = writeln!(out, "  | {{ {} }}", fields.join("; "));
    }
    out.push_str(";\n\n");
}

/// A descriptor the codec walks to restore what JSON can't carry.
fn descriptor(ty: &Ty) -> String {
    match ty {
        Ty::Primitive(primitive) => format!(
            "{{ k: {} }}",
            quoted(match primitive {
                Primitive::Text => "text",
                Primitive::Integer => "integer",
                Primitive::Instant => "instant",
                Primitive::Duration => "duration",
                Primitive::Boolean => "boolean",
                Primitive::Nothing => "nothing",
                Primitive::Never => "never",
            })
        ),
        Ty::List(item) => format!("{{ k: \"list\", item: {} }}", descriptor(item)),
        Ty::Optional(item) => format!("{{ k: \"optional\", item: {} }}", descriptor(item)),
        Ty::Data { name, arguments } => format!(
            "{{ k: \"data\", name: {}, args: [{}] }}",
            quoted(name),
            arguments
                .iter()
                .map(descriptor)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Ty::Variable(name) => format!("{{ k: \"var\", name: {} }}", quoted(name)),
        Ty::Opaque(_) | Ty::Erased => "{ k: \"erased\" }".into(),
    }
}

fn declare_descriptors(schema: &Schema, out: &mut String) {
    out.push_str("const DATA: Record<string, DataDescriptor> = {\n");
    for data in &schema.datas {
        if crate::model::is_prelude(&data.name) || data.opaque || !data.opaque_fields.is_empty() {
            continue;
        }
        let constructors: Vec<String> = data
            .constructors
            .iter()
            .map(|constructor| {
                let fields: Vec<String> = constructor
                    .fields()
                    .map(|(name, field_ty)| {
                        format!("[{}, {}]", quoted(&camel_case(name)), descriptor(field_ty))
                    })
                    .collect();
                format!("{}: [{}]", quoted(&constructor.kind()), fields.join(", "))
            })
            .collect();
        let generics: Vec<String> = data.generics.iter().map(|name| quoted(name)).collect();
        let _ = writeln!(
            out,
            "  {}: {{ params: [{}], constructors: {{ {} }} }},",
            quoted(&data.name),
            generics.join(", "),
            constructors.join(", ")
        );
    }
    out.push_str("};\n\n");
}

fn declare_client(schema: &Schema, out: &mut String) -> Result<(), Error> {
    let mut methods = String::new();
    let mut parameter_names = String::new();
    for function in schema.entries() {
        let split = function.split.as_ref().expect("entries are split");
        for case in &split.cases {
            let mut parameters: Vec<(String, Ty)> = Vec::new();
            let mut arguments = Vec::new();
            let fresh = |name: &str, parameters: &mut Vec<(String, Ty)>, field_ty: &Ty| {
                let mut identifier = camel_case(name);
                while parameters
                    .iter()
                    .any(|(existing, _)| *existing == identifier)
                {
                    identifier.push('_');
                }
                parameters.push((identifier.clone(), field_ty.clone()));
                identifier
            };
            for (position, argument) in case.parameters.iter().enumerate() {
                let parameter = &function.parameters[position];
                let key = quoted(&camel_case(&parameter.name));
                let (constructor, fields): (&str, Vec<(String, Ty)>) = match argument {
                    Argument::Erased | Argument::Solved(_) => continue,
                    Argument::Split => (
                        &case.constructor,
                        schema
                            .constructor(&case.constructor)
                            .fields()
                            .map(|(name, field_ty)| (name.to_string(), field_ty.clone()))
                            .collect(),
                    ),
                    Argument::Collapsed {
                        constructor,
                        fields,
                    } => (constructor, fields.clone()),
                    Argument::Plain { ty, .. } => {
                        let identifier = fresh(&parameter.name, &mut parameters, ty);
                        arguments.push(format!("[{key}, {identifier}, {}]", descriptor(ty)));
                        continue;
                    }
                };
                let constructor_data = &schema.constructor(constructor).data;
                let mut values = vec![format!(
                    "kind: {}",
                    quoted(&schema.constructor(constructor).kind())
                )];
                for (name, field_ty) in &fields {
                    let identifier = fresh(name, &mut parameters, field_ty);
                    values.push(format!("{}: {identifier}", camel_case(name)));
                }
                let data_ty = Ty::Data {
                    name: constructor_data.clone(),
                    arguments: schema
                        .data(constructor_data)
                        .generics
                        .iter()
                        .map(|_| Ty::Erased)
                        .collect(),
                };
                arguments.push(format!(
                    "[{key}, {{ {} }}, {}]",
                    values.join(", "),
                    descriptor(&data_ty)
                ));
            }
            let name = camel_case(&case.constructor);
            let signature: Vec<String> = parameters
                .iter()
                .map(|(identifier, parameter_ty)| format!("{identifier}: {}", ty(parameter_ty)))
                .collect();
            let names: Vec<String> = parameters
                .iter()
                .map(|(identifier, _)| quoted(identifier))
                .collect();
            let _ = writeln!(parameter_names, "  {name}: [{}],", names.join(", "));
            let _ = writeln!(
                methods,
                "  /** `{}` at `{}`. */\n  {name}({}): Promise<{}> {{\n    return this.call({}, [{}], {}) as Promise<{}>;\n  }}\n",
                function.name,
                case.constructor,
                signature.join(", "),
                ty(&case.result),
                quoted(&function.name),
                arguments.join(", "),
                descriptor(&case.result),
                ty(&case.result)
            );
        }
    }
    let _ = write!(
        out,
        r#"/** Each method's parameters, in order. */
export const METHOD_PARAMETERS: Record<string, readonly string[]> = {{
{parameter_names}}};

/** A call the server could not fit to its entry point's types. */
export class InvalidRequest extends Error {{}}

interface QueuedCall {{
  call: unknown;
  result: Descriptor;
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
}}

/** Calls the schema's entry points through a transport that sends and
 * receives JSON text. `batch` sends several calls as one request. */
export class Client {{
  private queue: QueuedCall[] | null = null;
  private readonly send: (request: string) => Promise<string>;
  constructor(send: (request: string) => Promise<string>) {{
    this.send = send;
  }}

  async batch<R>(build: (calls: Client) => Promise<R>[]): Promise<R[]> {{
    const calls = new Client(this.send);
    const queue: QueuedCall[] = [];
    calls.queue = queue;
    const results = build(calls);
    calls.queue = null;
    if (queue.length) void this.sendCalls(queue);
    return Promise.all(results);
  }}

  private call(fn: string, args: [string, unknown, Descriptor][], result: Descriptor): Promise<unknown> {{
    const encoded: Record<string, unknown> = {{}};
    for (const [key, value, descriptor] of args) encoded[key] = encode(descriptor, value, {{}});
    return new Promise((resolve, reject) => {{
      const queued: QueuedCall = {{ call: {{ function: fn, arguments: encoded }}, result, resolve, reject }};
      if (this.queue) this.queue.push(queued);
      else void this.sendCalls([queued]);
    }});
  }}

  private async sendCalls(calls: QueuedCall[]): Promise<void> {{
    try {{
      const response = await this.send(JSON.stringify(calls.map((queued) => queued.call)));
      const answers = JSON.parse(response) as unknown[];
      calls.forEach((queued, index) => {{
        const answer = answers[index] as {{ kind?: string; message?: string }};
        if (answer?.kind === "invalid request") queued.reject(new InvalidRequest(answer.message));
        else queued.resolve(decode(queued.result, answer, {{}}));
      }});
    }} catch (error) {{
      for (const queued of calls) queued.reject(error);
    }}
  }}

{methods}}}
"#
    );
    Ok(())
}

const CODEC: &str = r#"type Descriptor =
  | { k: "text" | "integer" | "instant" | "duration" | "boolean" | "nothing" | "never" | "erased" }
  | { k: "list" | "optional"; item: Descriptor }
  | { k: "data"; name: string; args: Descriptor[] }
  | { k: "var"; name: string };

interface DataDescriptor {
  params: string[];
  constructors: Record<string, [string, Descriptor][]>;
}

type Bindings = Record<string, Descriptor>;

function walk(
  descriptor: Descriptor,
  value: unknown,
  bindings: Bindings,
  leaf: (k: string, value: unknown) => unknown,
): unknown {
  switch (descriptor.k) {
    case "list":
      return (value as unknown[]).map((item) => walk(descriptor.item, item, bindings, leaf));
    case "optional":
      return value === null || value === undefined ? null : walk(descriptor.item, value, bindings, leaf);
    case "var":
      return walk(bindings[descriptor.name] ?? { k: "erased" }, value, bindings, leaf);
    case "data": {
      const data = DATA[descriptor.name];
      if (!data) return value;
      const record = value as Record<string, unknown>;
      const fields = data.constructors[record.kind as string];
      if (!fields) return value;
      const inner: Bindings = {};
      data.params.forEach((param, index) => {
        const arg = descriptor.args[index] ?? { k: "erased" };
        inner[param] = arg.k === "var" ? bindings[arg.name] ?? { k: "erased" } : arg;
      });
      const result: Record<string, unknown> = { kind: record.kind };
      for (const [key, field] of fields) result[key] = walk(field, record[key], inner, leaf);
      return result;
    }
    default:
      return leaf(descriptor.k, value);
  }
}

function encode(descriptor: Descriptor, value: unknown, bindings: Bindings): unknown {
  return walk(descriptor, value, bindings, (k, leaf) => {
    if (k === "instant") return (leaf as Date).getTime();
    return leaf;
  });
}

function decode(descriptor: Descriptor, value: unknown, bindings: Bindings): unknown {
  return walk(descriptor, value, bindings, (k, leaf) => {
    if (k === "instant") return new Date(leaf as number);
    return leaf;
  });
}

"#;
