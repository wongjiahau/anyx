//! Subtyping: whether a value of one type may be passed where another is
//! expected. A refinement fits its base, a member its union, and lists,
//! maps and generic slots are covariant. A record fits a larger one when
//! each field it lacks has a `DEFAULT`. A named type fits only itself.

mod common;

use anyx_protocol::Schema;
use anyx_protocol::schema::Type;
use common::schema;

const SOURCE: &str = "PROTOCOL p VERSION 1,
TYPE ALIAS Depth: Integer WHERE [0 <= _],
TYPE ALIAS Size: Integer WHERE [0 <= _],
TYPE ALIAS Small: Integer WHERE [_ < 10],
TYPE ALIAS Shade [| `dark, `light |],
TYPE ALIAS Colour [| `dark, `light, `custom hue (hue: Float) |],
TYPE ALIAS Key [| Integer, Text |],
NAMED TYPE File Id (Text),
NAMED TYPE Share Id (Text),
TYPE Page of (Item: TYPE) { items: LIST Item },
TYPE Point { x: Integer, y: Integer },
TYPE Point With Label { x: Integer, y: Integer, label (Text) DEFAULT label \"\" },
TYPE Point In Space { x: Integer, y: Integer, z: Integer },
TYPE Node { value: Integer, children: LIST Node },
TYPE Link { value: Integer, children: LIST Link },
METHOD use (depth: Depth) (size: Size) (small: Small) (shade: Shade) (colour: Colour) (key: Key)
    (file: File Id) (share: Share Id)
    (depths: Page of (Depth)) (integers: Page of (Integer))
    (point: Point) (labelled: Point With Label) (with z: Point In Space) (node: Node) (link: Link)
    RETURNS NOTHING
    ERRORS [| |],";

/// The types of `use`'s parameters, by name.
fn types() -> (Schema, impl Fn(&str) -> Type) {
    let schema = schema(SOURCE);
    let parameters = schema.methods[0].shape.parameters.clone();
    let named = move |name: &str| {
        parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap_or_else(|| panic!("no parameter {name}"))
            .r#type
            .clone()
    };
    (schema, named)
}

fn list(item: Type) -> Type {
    Type::List(Box::new(item))
}

#[test]
fn a_refinement_fits_its_base_and_not_the_other_way() {
    let (schema, r#type) = types();
    assert!(schema.fits(&r#type("depth"), &Type::Integer));
    assert!(!schema.fits(&Type::Integer, &r#type("depth")));
}

#[test]
fn a_refinement_fits_another_only_with_the_same_predicate() {
    let (schema, r#type) = types();
    assert!(schema.fits(&r#type("depth"), &r#type("size")));
    assert!(!schema.fits(&r#type("depth"), &r#type("small")));
}

#[test]
fn a_literal_fits_a_refinement_of_its_type() {
    let (schema, r#type) = types();
    assert!(schema.fits_literal(&Type::Integer, &r#type("depth")));
    assert!(schema.fits_literal(&list(Type::Integer), &list(r#type("depth"))));
    assert!(!schema.fits_literal(&Type::Text, &r#type("depth")));
}

#[test]
fn a_member_fits_its_union_and_a_union_fits_a_larger_one() {
    let (schema, r#type) = types();
    assert!(schema.fits(&Type::Integer, &r#type("key")));
    assert!(schema.fits(&r#type("depth"), &r#type("key")));
    assert!(schema.fits(&r#type("shade"), &r#type("colour")));
    assert!(!schema.fits(&r#type("colour"), &r#type("shade")));
    assert!(!schema.fits(&r#type("key"), &Type::Integer));
}

#[test]
fn lists_maps_and_generic_slots_are_covariant() {
    let (schema, r#type) = types();
    assert!(schema.fits(&list(r#type("depth")), &list(Type::Integer)));
    assert!(!schema.fits(&list(Type::Integer), &list(r#type("depth"))));
    assert!(schema.fits(
        &Type::Map(Box::new(r#type("shade"))),
        &Type::Map(Box::new(r#type("colour")))
    ));
    assert!(schema.fits(&r#type("depths"), &r#type("integers")));
    assert!(!schema.fits(&r#type("integers"), &r#type("depths")));
}

#[test]
fn a_record_fits_a_larger_one_whose_extra_fields_have_defaults() {
    let (schema, r#type) = types();
    assert!(schema.fits(&r#type("point"), &r#type("labelled")));
    assert!(!schema.fits(&r#type("point"), &r#type("with z")));
    // A field the larger record lacks never fits.
    assert!(!schema.fits(&r#type("labelled"), &r#type("point")));
    // Records that hold themselves compare field by field.
    assert!(schema.fits(&r#type("node"), &r#type("link")));
}

#[test]
fn a_named_type_fits_only_itself() {
    let (schema, r#type) = types();
    assert!(schema.fits(&r#type("file"), &r#type("file")));
    assert!(!schema.fits(&r#type("file"), &r#type("share")));
    assert!(!schema.fits(&r#type("file"), &Type::Text));
    assert!(!schema.fits(&Type::Text, &r#type("file")));
}
