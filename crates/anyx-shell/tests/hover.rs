use anyx_shell::{Interpreter, environment::Environment};

fn shell() -> Interpreter {
    let mut shell = Interpreter::new();
    shell.register("environment", Environment::new()).unwrap();
    shell
}

#[test]
fn describes_manually_typed_arguments_and_their_source_ranges() {
    let source = "environment / set variable \"日本語\" to \"value\"";
    for (text, label) in [("日本語", "name: Text"), ("value", "value: Text")] {
        let offset = source.find(text).unwrap();
        let info = shell().argument_info(source, offset);
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].label, label);
        assert_eq!(&source[info[0].range.clone()], format!("\"{text}\""));
        assert!(info[0].signature.contains("set variable (name ⁽Text)"));
        assert!(
            info[0]
                .documentation
                .as_ref()
                .unwrap()
                .contains("Values may be empty")
        );
    }
}

#[test]
fn prefers_nested_arguments_and_supports_grouping_sugar() {
    for source in [
        "environment / set variable \"A\" to (environment / read variable \"B\")",
        "environment / set variable \"A\" to ⁽environment / read variable \"B\"",
        "environment / list variables, environment / read variable \"B\"",
    ] {
        let info = shell().argument_info(source, source.find('B').unwrap());
        assert_eq!(info.len(), 1, "{source}");
        assert_eq!(info[0].label, "name: Text");
        assert_eq!(info[0].signature, "read variable (name ⁽Text)");
    }
}

#[test]
fn tolerates_unfinished_arguments_and_invalid_values() {
    for source in [
        "environment / read variable \"HOME",
        "environment / read variable 42",
        "environment / read variable (\"HOME\"",
    ] {
        let info = shell().argument_info(source, source.len());
        assert_eq!(info.len(), 1, "{source}");
        assert_eq!(info[0].label, "name: Text");
    }
}

#[test]
fn ignores_method_words_comments_unknown_scopes_and_invalid_offsets() {
    for source in [
        "environment / read variable",
        "unknown / read variable \"A\"",
        "environment / read variable \"A\" # comment",
        "environment / unknown \"A\"",
    ] {
        assert!(
            shell().argument_info(source, source.len()).is_empty(),
            "{source}"
        );
    }
    let source = "environment / read variable \"日本語\"";
    assert!(shell().argument_info(source, usize::MAX).is_empty());
    assert!(
        shell()
            .argument_info(source, source.find('日').unwrap() + 1)
            .is_empty()
    );
}
