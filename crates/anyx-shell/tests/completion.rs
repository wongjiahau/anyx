use anyx_protocol::{Call, Outcome, Schema};
use anyx_shell::{Interpreter, Tool, environment::Environment};

fn shell() -> Interpreter {
    let mut shell = Interpreter::new();
    shell.register("environment", Environment::new()).unwrap();
    shell
}

fn candidates(source: &str) -> Vec<String> {
    shell().complete(source, source.len()).1
}

#[test]
fn completes_scopes_and_method_words() {
    for source in ["", "env", "environment"] {
        assert_eq!(candidates(source), ["environment / "]);
    }
    for source in ["environment / ", "environment/ "] {
        let found = candidates(source);
        assert!(found.contains(&"list variables ".into()));
        assert!(found.contains(&"read variable ".into()));
        assert!(found.contains(&"SCHEMA ".into()));
    }
    assert_eq!(candidates("environment / li"), ["list variables "]);
    assert_eq!(candidates("environment / list v"), ["variables "]);
    assert_eq!(candidates("environment "), ["/ "]);
    assert_eq!(candidates("environment/"), ["/ "]);
    assert_eq!(candidates("environment / set variable \"A\" t"), ["to "]);
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
        ["to "]
    );
}

#[test]
fn avoids_strings_comments_unknown_scopes_and_argument_slots() {
    for source in [
        "unknown / ",
        "environment / read variable ",
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
    let (start, found) = shell().complete(source, offset);
    assert!(found.contains(&"read variable ".into()));
    assert_eq!(&source[start..offset], "re");
    assert!(shell().complete(source, usize::MAX).1.is_empty());
    assert!(
        shell()
            .complete(source, source.find('日').unwrap() + 1)
            .1
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
    assert_eq!(shell.complete("o", 1).1, ["other / "]);
    assert_eq!(shell.complete("other / g", 9).1, ["greet "]);
}
