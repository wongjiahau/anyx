use anyx_protocol::{Call, Outcome, Schema};
use anyx_shell::{Interpreter, Tool, environment::Environment};

fn shell() -> Interpreter {
    let mut shell = Interpreter::new();
    shell.register("environment", Environment::new()).unwrap();
    shell
}

fn candidates(source: &str) -> Vec<String> {
    shell()
        .completions(source, source.len())
        .items
        .into_iter()
        .map(|item| item.insert_text)
        .collect()
}

#[test]
fn completes_scopes_and_method_words() {
    for source in ["", "env", "environment"] {
        assert_eq!(candidates(source), ["environment / "]);
    }
    for source in ["environment / ", "environment/ "] {
        let found = candidates(source);
        assert!(found.contains(&"list variables".into()));
        assert!(found.contains(&"read variable \"name\"".into()));
        assert!(found.contains(&"SCHEMA".into()));
    }
    assert_eq!(candidates("environment / li"), ["list variables"]);
    assert_eq!(candidates("environment / list v"), ["variables"]);
    assert_eq!(candidates("environment "), ["/ "]);
    assert_eq!(candidates("environment/"), ["/ "]);
    assert_eq!(
        candidates("environment / set variable \"A\" t"),
        ["to \"value\""]
    );
}

#[test]
fn understands_nesting_grouping_sugar_and_multiple_commands() {
    for source in [
        "environment / set variable \"A\" to (env",
        "environment / set variable \"A\" to: env",
        "environment / list variables, env",
        "environment / set variable \"A\" to (environment / read variable \"B\"), env",
    ] {
        assert_eq!(candidates(source), ["environment / "], "{source}");
    }
    assert_eq!(
        candidates("environment / set variable (environment / read variable \"A\") t"),
        ["to \"value\""]
    );
}

#[test]
fn avoids_strings_comments_unknown_scopes_and_argument_slots() {
    for source in [
        "unknown / ",
        "environment / read variable \"env",
        "environment / read variable # env",
        "environment / read variable @\"env",
        "## env",
        "environment / list variables ## env",
        "environment / list variables ",
    ] {
        assert!(candidates(source).is_empty(), "{source}");
    }
    assert_eq!(candidates("## comment\nenv"), ["environment / "]);
}

#[test]
fn offsets_are_utf8_bytes_and_preserve_surrounding_source() {
    let source = "environment / set variable \"日本語\" to (environment / re)";
    let offset = source.find("re)").unwrap() + 2;
    let result = shell().completions(source, offset);
    let found: Vec<_> = result
        .items
        .into_iter()
        .map(|item| item.insert_text)
        .collect();
    let start = result.replace.start;
    assert!(found.contains(&"read variable \"name\"".into()));
    assert_eq!(&source[start..offset], "re");
    assert!(shell().completions(source, usize::MAX).items.is_empty());
    assert!(
        shell()
            .completions(source, source.find('日').unwrap() + 1)
            .items
            .is_empty()
    );
}

struct NeverInvoke(Schema);
impl Tool for NeverInvoke {
    fn schema(&self) -> &Schema {
        &self.0
    }
    fn invoke(&mut self, _: &Call) -> Outcome {
        panic!("completion must not execute tools")
    }
}

#[test]
fn reads_registered_schemas_without_invoking_tools() {
    let mut shell = Interpreter::new();
    shell
        .register(
            "other",
            NeverInvoke(
                Schema::parse(
                    "PROTOCOL other VERSION 1, METHOD greet (name: Text) RETURNS (value: Text)",
                )
                .unwrap(),
            ),
        )
        .unwrap();
    assert_eq!(
        shell
            .completions("o", 1)
            .items
            .into_iter()
            .map(|item| item.insert_text)
            .collect::<Vec<_>>(),
        ["other / "]
    );
    assert_eq!(
        shell
            .completions("other / g", 9)
            .items
            .into_iter()
            .map(|item| item.insert_text)
            .collect::<Vec<_>>(),
        ["greet \"name\""]
    );
}

#[test]
fn method_templates_include_typed_slots_return_types_and_documentation() {
    let result = shell().completions("environment / set", 17);
    let item = &result.items[0];
    assert_eq!(item.insert_text, "set variable \"name\" to \"value\"");
    assert_eq!(item.tab_stops.len(), 2);
    assert_eq!(&item.insert_text[item.tab_stops[0].range.clone()], "name");
    assert_eq!(&item.insert_text[item.tab_stops[1].range.clone()], "value");
    assert_eq!(item.tab_stops[0].label, "name: Text");
    assert!(item.label.contains("Text"));
    assert!(item.detail.contains("Variable"));
    assert!(item.documentation.as_ref().unwrap().contains("empty"));
}

#[test]
fn replacing_in_the_middle_of_a_command_preserves_existing_arguments() {
    let source = "environment / set variable \"KEEP\" to \"UNCHANGED\"";
    let offset = source.find("variable").unwrap() + 2;
    let result = shell().completions(source, offset);
    assert_eq!(&source[result.replace.clone()], "variable");
    assert_eq!(result.items[0].insert_text, "variable");
    let mut edited = source.to_string();
    edited.replace_range(result.replace, &result.items[0].insert_text);
    assert_eq!(edited, source);
}
