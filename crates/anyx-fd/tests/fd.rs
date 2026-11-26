//! fd's requests, written and read as the anyx shell writes and reads them.
use std::fs;
use std::path::Path;

use anyx_core_generate::client::{CallError, Client};

fn tree() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    for (path, contents) in [
        ("src/lib.rs", "pub fn a() {}\n"),
        ("src/main.rs", "fn main() {}\n"),
        ("src/Notes.md", "# notes\n"),
        ("src/deep/inner.rs", ""),
        ("target/build.rs", "ignored\n"),
        (".hidden.rs", "hidden\n"),
        (".gitignore", "target/\n"),
        ("README.md", "read me\n"),
    ] {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    // `.gitignore` is read in a git repository.
    fs::create_dir(root.join(".git")).unwrap();
    directory
}

/// The answer to `request`, an expression written against `fd.anyx`, or
/// `INVALID: …` for one the schema rules out.
fn answer(directory: &Path, request: &str) -> String {
    let client = Client::load(anyx_fd::SCHEMA, "fd.anyx").unwrap();
    match client.call_written(request) {
        Ok(call) => client
            .answer(&call, &anyx_fd::answer(&call.request, directory))
            .unwrap(),
        Err(CallError::Invalid { message, .. }) => format!("INVALID: {message}"),
        Err(CallError::Elsewhere) => "ELSEWHERE".to_owned(),
    }
}

fn found(paths: &str) -> String {
    format!("answered {paths}")
}

#[test]
fn find_skips_ignored_and_hidden_files_in_smart_case() {
    let directory = tree();
    assert_eq!(
        answer(directory.path(), r#"find "\\.rs$""#),
        found(r#"["src/deep/inner.rs", "src/lib.rs", "src/main.rs"]"#)
    );
    assert_eq!(
        answer(directory.path(), r#"find "notes""#),
        found(r#"["src/Notes.md"]"#)
    );
    assert_eq!(
        answer(directory.path(), r#"find "Notes""#),
        found(r#"["src/Notes.md"]"#)
    );
    assert_eq!(answer(directory.path(), r#"find "NOTES""#), found("[]"));
}

#[test]
fn a_search_changes_how_fd_searches() {
    let directory = tree();
    for (request, expected) in [
        (
            r#"find "\\.rs$" with { hidden, no ignore }"#,
            r#"[".hidden.rs", "src/deep/inner.rs", "src/lib.rs", "src/main.rs", "target/build.rs"]"#,
        ),
        (
            r#"find "*.md" with { glob pattern }"#,
            r#"["README.md", "src/Notes.md"]"#,
        ),
        (r#"find "NOTES" with { any case }"#, r#"["src/Notes.md"]"#),
        (r#"find "notes" with { exact case }"#, "[]"),
        (
            r#"find "." with { fixed text pattern, max depth 1 }"#,
            r#"["README.md"]"#,
        ),
        (
            r#"find "" with { only [directory] }"#,
            r#"["src", "src/deep"]"#,
        ),
        (
            r#"find "" with { extensions ["md"] }"#,
            r#"["README.md", "src/Notes.md"]"#,
        ),
        (
            r#"find "" with { min depth 2, max depth 2, only [file] }"#,
            r#"["src/Notes.md", "src/lib.rs", "src/main.rs"]"#,
        ),
        (
            r#"find "" with { min depth 3 }"#,
            r#"["src/deep/inner.rs"]"#,
        ),
        (
            r#"find "" with { only [empty] }"#,
            r#"["src/deep/inner.rs"]"#,
        ),
        (r#"find "" with { excluding ["src"] }"#, r#"["README.md"]"#),
        (
            r#"find "\\.rs$" with { also matching ["^m"] }"#,
            r#"["src/main.rs"]"#,
        ),
        (
            r#"find "src/d" with { full path }"#,
            r#"["src/deep", "src/deep/inner.rs"]"#,
        ),
        (r#"find "" with { at least 14 bytes }"#, r#"["src/lib.rs"]"#),
        (r#"find "" with { at most 1 results }"#, r#"["README.md"]"#),
        (r#"find "" with { at most 0 results }"#, "[]"),
        (r#"find "^s" with { prune }"#, r#"["src"]"#),
        (
            r#"find "" with { changed within (1 hours), only [file], at most 1 results }"#,
            r#"["README.md"]"#,
        ),
        (r#"find "" with { changed more than (1 hours) ago }"#, "[]"),
    ] {
        assert_eq!(
            answer(directory.path(), request),
            found(expected),
            "{request}"
        );
    }
}

#[test]
fn paths_are_searched_and_written_as_given() {
    let directory = tree();
    assert_eq!(
        answer(
            directory.path(),
            r#"find "rs$" in ["src/deep", "src"] with { max depth 1 }"#
        ),
        found(r#"["src/deep/inner.rs", "src/lib.rs", "src/main.rs"]"#)
    );
    assert_eq!(
        answer(directory.path(), r#"find "x" in ["missing"]"#),
        r#"failed ("missing" does not exist)"#
    );
}

#[test]
fn details_and_any_match() {
    let directory = tree();
    let details = answer(directory.path(), r#"details of "lib""#);
    assert!(
        details.starts_with(r#"answered [{ path "src/lib.rs", file, size 14 bytes, modified at ("#),
        "{details}"
    );
    assert!(
        details.ends_with(" seconds since the epoch) }]"),
        "{details}"
    );
    assert_eq!(
        answer(directory.path(), r#"any match of "main""#),
        "answered (true)"
    );
    assert_eq!(
        answer(directory.path(), r#"any match of "main" in ["src/deep"]"#),
        "answered (false)"
    );
}

#[test]
fn mistakes_are_failures_or_invalid_requests() {
    let directory = tree();
    assert_eq!(
        answer(directory.path(), r#"find "(""#),
        r#"failed (invalid pattern "unclosed group")"#
    );
    assert!(
        answer(directory.path(), r#"find "[" with { glob pattern }"#)
            .starts_with(r#"failed (invalid glob ""#)
    );
    // A search says each thing once, so it can't contradict itself.
    for request in [
        r#"find "x" with { glob pattern, fixed text pattern }"#,
        r#"find "x" with { hidden, hidden }"#,
        r#"find "x" with { glob }"#,
    ] {
        assert_eq!(
            answer(directory.path(), request),
            "INVALID: no record has exactly these fields",
            "{request}"
        );
    }
    assert!(answer(directory.path(), r#"find 3"#).starts_with("INVALID: "));
    assert_eq!(answer(directory.path(), r#"locate "x""#), "ELSEWHERE");
}

#[test]
fn the_program_writes_its_schema() {
    let program = env!("CARGO_BIN_EXE_anyx-fd");
    let output = std::process::Command::new(program)
        .arg("--anyx-schema")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), anyx_fd::SCHEMA);
}

#[test]
fn the_shell_brings_in_the_program_and_calls_it() {
    let directory = tree();
    let program = env!("CARGO_BIN_EXE_anyx-fd");
    let root = directory.path().to_str().unwrap();
    let mut shell = anyx_shell::Interpreter::new();
    let source = format!(
        "USE fd FROM COMMAND {program:?},\n\
         fd / find \"lib\" in [{root:?}] with {{ only [file] }},\n\
         USE ALL FROM fd,\n\
         any match of \"main\" in [{root:?}] with {{ max depth 2 }},\n"
    );
    let execution = shell.execute(&source).unwrap();
    assert_eq!(execution.error, None);
    assert_eq!(
        execution.output,
        format!(
            "answered [{:?}],\nanswered (true),\n",
            format!("{root}/src/lib.rs")
        )
    );
    // A call the schema rules out never reaches the program.
    let problem = shell
        .execute("find \"x\" with { hidden, hidden }")
        .unwrap_err();
    assert!(
        problem
            .message
            .contains("no record has exactly these fields"),
        "{}",
        problem.message
    );
    // The program's failures come back as values.
    let execution = shell.execute("find \"(\"").unwrap();
    assert_eq!(execution.error, None);
    assert_eq!(
        execution.output,
        "failed (invalid pattern \"unclosed group\")\n"
    );
}

#[test]
fn a_document_that_fails_keeps_no_tool_it_brought_in() {
    let program = env!("CARGO_BIN_EXE_anyx-fd");
    let mut shell = anyx_shell::Interpreter::new();
    let problem = shell
        .execute(&format!("USE fd FROM COMMAND {program:?},\nfd / nothing"))
        .unwrap_err();
    assert!(problem.message.contains("nothing"), "{}", problem.message);
    let problem = shell.execute("fd / find \"x\"").unwrap_err();
    assert!(problem.message.contains("fd"), "{}", problem.message);
    // Bringing in the same program under the same name again is allowed.
    let use_fd = format!("USE fd FROM COMMAND {program:?}");
    shell.execute(&use_fd).unwrap();
    shell.execute(&use_fd).unwrap();
    let problem = shell
        .execute("USE fd FROM COMMAND \"another-program\"")
        .unwrap_err();
    assert!(problem.message.contains("already"), "{}", problem.message);
}

#[test]
fn a_program_that_does_not_answer_is_not_brought_in() {
    let mut shell = anyx_shell::Interpreter::new();
    let problem = shell
        .execute("USE nothing FROM COMMAND \"anyx-no-such-program\"")
        .unwrap_err();
    assert!(
        problem.message.contains("couldn't run"),
        "{}",
        problem.message
    );
    // `false` runs and fails.
    let problem = shell
        .execute("USE nope FROM COMMAND \"false\"")
        .unwrap_err();
    assert!(problem.message.contains("failed"), "{}", problem.message);
}
