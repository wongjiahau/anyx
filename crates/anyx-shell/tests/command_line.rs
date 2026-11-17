use std::io::Write;
use std::process::{Command, Output, Stdio};

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_anyx-shell"));
    command.env_clear().env("ANYX_INHERITED", "original");
    command
}

fn assert_anyx(bytes: &[u8]) {
    let source = std::str::from_utf8(bytes).unwrap();
    assert!(anyx::parse(source).errors.is_empty(), "{source}");
}

fn succeeds(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_anyx(&output.stdout);
}

#[test]
fn expression_inherits_environment_and_keeps_changes_local() {
    let output = command().args(["--expression", "environment / read variable \"ANYX_INHERITED\", environment / set variable \"ANYX_INHERITED\" to \"changed\", environment / read variable \"ANYX_INHERITED\""]).output().unwrap();
    succeeds(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"changed\""));
    let fresh = command()
        .args([
            "--expression",
            "environment / read variable \"ANYX_INHERITED\"",
        ])
        .output()
        .unwrap();
    succeeds(&fresh);
    assert_eq!(fresh.stdout, b"\"original\"\n");
}

#[test]
fn reads_a_script_file() {
    let output = command()
        .arg(format!(
            "{}/examples/environment.shell.anyx",
            env!("CARGO_MANIFEST_DIR")
        ))
        .output()
        .unwrap();
    succeeds(&output);
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("{ name"));
}

#[test]
fn reads_standard_input_with_or_without_dash() {
    for arguments in [vec![], vec!["-"]] {
        let mut child = command()
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"environment / list variables")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        succeeds(&output);
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "[{ name \"ANYX_INHERITED\", value \"original\" }]\n"
        );
    }
}

#[test]
fn failures_are_anyx_on_standard_error_with_failing_status() {
    for arguments in [
        vec!["--expression", "environment / read variable \"absent\""],
        vec!["--expression", "list variables"],
        vec!["--expression", "environment / read variable ("],
        vec!["--unknown"],
        vec!["/a/nonexistent/anyx-script.shell.anyx"],
    ] {
        let output = command().args(&arguments).output().unwrap();
        assert!(!output.status.success(), "{arguments:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_anyx(&output.stderr);
    }
}

#[test]
fn earlier_values_are_kept_when_a_method_fails() {
    let output = command().args(["--expression", "environment / read variable \"ANYX_INHERITED\", environment / read variable \"missing\""]).output().unwrap();
    assert!(!output.status.success());
    assert_eq!(output.stdout, b"\"original\"\n");
    assert_eq!(output.stderr, b"ERROR variable does not exist\n");
}

#[test]
fn help_is_an_anyx_text_value() {
    let output = command().arg("--help").output().unwrap();
    succeeds(&output);
    let source = String::from_utf8(output.stdout).unwrap();
    assert!(matches!(
        anyx::parse(&source).syntax.nodes()[0].kind,
        anyx::NodeKind::String(_)
    ));
}

#[cfg(unix)]
#[test]
fn non_unicode_inherited_values_are_reported_without_lossy_conversion() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let output = command()
        .env("INVALID", OsString::from_vec(vec![0xff]))
        .args(["--expression", "environment / list variables"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_anyx(&output.stderr);
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("not Unicode")
    );
}

#[test]
fn nested_expression_results_are_arguments_not_extra_output() {
    let output = command().args(["--expression", "environment / set variable \"COPY\" to (environment / read variable \"ANYX_INHERITED\")"]).output().unwrap();
    succeeds(&output);
    assert_eq!(output.stdout, b"{ name \"COPY\", value \"original\" }\n");
}
