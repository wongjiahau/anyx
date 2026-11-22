//! Type aliases: `TYPE ALIAS Paths: LIST Text` names a type without making
//! a new one, so each use of the alias is the type it stands for.

mod common;

use anyx_protocol::schema::{Type, TypeKind};
use common::{assert_snapshot, diagnostics, parse_files, render, schema};

#[test]
fn an_alias_is_the_type_it_stands_for() {
    let schema = schema(
        "PROTOCOL p VERSION 1,
        TYPE ALIAS Page of (Item: TYPE): { items: LIST Item },
        TYPE ALIAS Payment: { amount: Integer },
        # Many paths.
        TYPE ALIAS Paths: LIST Text,
        TYPE ALIAS Payments: Page of (Payment),
        METHOD list (paths: Paths) RETURNS (payments: Payments) ERRORS [| |],",
    );
    let list = schema.method("list _").unwrap();
    assert_eq!(
        list.shape.parameters[0].r#type,
        Type::List(Box::new(Type::Text))
    );
    assert_eq!(
        list.returns.as_ref().unwrap().r#type,
        Type::Generic(0, vec![Type::Named(1)])
    );
    // An alias makes no type of its own.
    assert!(
        schema
            .types
            .iter()
            .all(|known| known.name != "Paths" && known.name != "Payments")
    );
}

#[test]
fn a_generic_alias_puts_its_arguments_in_place_of_its_parameters() {
    let schema = schema(
        "PROTOCOL p VERSION 1,
        TYPE ALIAS (Item: TYPE) Pair: LIST Item,
        TYPE ALIAS Box of (Item: TYPE): { pairs: (Item) Pair },
        TYPE ALIAS Entry from (Key: TYPE) to (Value: TYPE): { key: Key, value: Value },
        TYPE ALIAS Table from (Key: TYPE) to (Value: TYPE): LIST: Entry from (Key) to (Value),
        METHOD a (x: (Integer) Pair) RETURNS (y: Table from (Text) to (Integer)) ERRORS [| |],
        METHOD b (x: Box of (Text)) RETURNS NOTHING ERRORS [| |],",
    );
    let a = schema.method("a _").unwrap();
    assert_eq!(
        a.shape.parameters[0].r#type,
        Type::List(Box::new(Type::Integer))
    );
    assert_eq!(
        a.returns.as_ref().unwrap().r#type,
        Type::List(Box::new(Type::Generic(1, vec![Type::Text, Type::Integer])))
    );
    let TypeKind::Record(fields) = &schema.types[0].kind else {
        panic!("expected a record");
    };
    assert_eq!(
        fields[0].r#type,
        Type::List(Box::new(Type::Parameter("Item".into())))
    );
}

#[test]
fn an_alias_may_be_used_before_it_is_declared() {
    let schema = schema(
        "PROTOCOL p VERSION 1,
        METHOD a (x: Names) RETURNS NOTHING ERRORS [| |],
        TYPE ALIAS Names: LIST Name,
        TYPE ALIAS Name: Text,",
    );
    assert_eq!(
        schema.method("a _").unwrap().shape.parameters[0].r#type,
        Type::List(Box::new(Type::Text))
    );
}

#[test]
fn a_public_alias_is_shared_by_a_direct_import() {
    let files = [
        (
            "protocol.protocol.anyx",
            "PROTOCOL p VERSION 1, USE \"./files.protocol.anyx\",",
        ),
        (
            "common.protocol.anyx",
            "PUBLIC TYPE ALIAS Paths: LIST Text, TYPE ALIAS Secret: Text,",
        ),
        (
            "files.protocol.anyx",
            "USE \"./common.protocol.anyx\", METHOD read (paths: Paths) RETURNS (text: Text) ERRORS [| |],",
        ),
    ];
    let schema = parse_files(&files).unwrap_or_else(|found| panic!("{}", render(&files, &found)));
    assert_eq!(
        schema.method("read _").unwrap().shape.parameters[0].r#type,
        Type::List(Box::new(Type::Text))
    );
    let files = [
        files[0],
        files[1],
        (
            "files.protocol.anyx",
            "USE \"./common.protocol.anyx\", METHOD read (id: Secret) RETURNS (text: Text) ERRORS [| |],",
        ),
    ];
    let found = parse_files(&files).unwrap_err();
    assert!(
        found
            .iter()
            .any(|diagnostic| diagnostic.message.contains("not visible")
                && diagnostic.message.contains("common.protocol.anyx")),
        "{}",
        render(&files, &found)
    );
}

#[test]
fn mistakes_in_type_aliases_are_diagnosed() {
    let cases = [
        (
            "an alias that leads back to itself, directly or through another",
            "PROTOCOL a VERSION 1, TYPE ALIAS Tree: LIST Tree, TYPE ALIAS Alpha: Beta, TYPE ALIAS Beta: LIST Alpha,",
        ),
        (
            "an alias with the name of another type",
            "PROTOCOL a VERSION 1, TYPE ALIAS Row: { a: Text }, TYPE ALIAS Row: Text, TYPE ALIAS Paths: Text, TYPE ALIAS Paths: Integer,",
        ),
        (
            "an alias without its type, or with a type in another slot",
            "PROTOCOL a VERSION 1, TYPE ALIAS (Item: TYPE) Pair, TYPE ALIAS Pair of (Text): LIST Text,",
        ),
        (
            "an alias over an unknown type",
            "PROTOCOL a VERSION 1, TYPE ALIAS Paths: LIST Path,",
        ),
        (
            "a parameter unused, twice, lowercase, or named like a type",
            "PROTOCOL a VERSION 1, TYPE ALIAS Row: { a: Text }, TYPE ALIAS (Item: TYPE) Pair: LIST Text, TYPE ALIAS (Item: TYPE) with (Item: TYPE): LIST Item, TYPE ALIAS (item: TYPE) Box: LIST item, TYPE ALIAS (Row: TYPE) Rows: LIST Row,",
        ),
        (
            "a name not in Title Case",
            "PROTOCOL a VERSION 1, TYPE ALIAS paths: LIST Text,",
        ),
        (
            "a use with the wrong number of arguments",
            "PROTOCOL a VERSION 1, TYPE ALIAS (Item: TYPE) Pair: LIST Item, METHOD a (x: Pair) RETURNS NOTHING ERRORS [| |],",
        ),
    ];
    let rendered: Vec<String> = cases
        .iter()
        .map(|(title, source)| format!("== {title}\n{source}\n{}\n", diagnostics(source)))
        .collect();
    assert_snapshot(
        "tests/snapshots/type_alias_diagnostics.txt",
        &rendered.join("\n"),
    );
}
