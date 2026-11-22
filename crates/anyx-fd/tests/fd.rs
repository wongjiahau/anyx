//! fd's methods, called as the anyx shell calls them.
use std::fs;
use std::path::Path;

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

fn answer(directory: &Path, request: &str) -> String {
    anyx_fd::answer(request, directory)
}

#[test]
fn find_skips_ignored_and_hidden_files_in_smart_case() {
    let directory = tree();
    assert_eq!(
        answer(directory.path(), r#"find "\\.rs$""#),
        "RETURNS [\"src/deep/inner.rs\", \"src/lib.rs\", \"src/main.rs\"]\n"
    );
    assert_eq!(
        answer(directory.path(), r#"find "notes""#),
        "RETURNS [\"src/Notes.md\"]\n"
    );
    assert_eq!(
        answer(directory.path(), r#"find "Notes""#),
        "RETURNS [\"src/Notes.md\"]\n"
    );
    assert_eq!(answer(directory.path(), r#"find "NOTES""#), "RETURNS []\n");
}

#[test]
fn a_search_changes_how_fd_searches() {
    let directory = tree();
    for (request, expected) in [
        (
            r#"find "\\.rs$" with { `hidden, `no ignore }"#,
            r#"[".hidden.rs", "src/deep/inner.rs", "src/lib.rs", "src/main.rs", "target/build.rs"]"#,
        ),
        (
            r#"find "*.md" with { `glob pattern }"#,
            r#"["README.md", "src/Notes.md"]"#,
        ),
        (r#"find "NOTES" with { `any case }"#, r#"["src/Notes.md"]"#),
        (r#"find "notes" with { `exact case }"#, "[]"),
        (
            r#"find "." with { `fixed text pattern, max depth 1 }"#,
            r#"["README.md"]"#,
        ),
        (
            r#"find "" with { only [`directory] }"#,
            r#"["src", "src/deep"]"#,
        ),
        (
            r#"find "" with { extensions ["md"] }"#,
            r#"["README.md", "src/Notes.md"]"#,
        ),
        (
            r#"find "" with { min depth 2, max depth 2, only [`file] }"#,
            r#"["src/Notes.md", "src/lib.rs", "src/main.rs"]"#,
        ),
        (
            r#"find "" with { min depth 3 }"#,
            r#"["src/deep/inner.rs"]"#,
        ),
        (
            r#"find "" with { only [`empty] }"#,
            r#"["src/deep/inner.rs"]"#,
        ),
        (r#"find "" with { excluding ["src"] }"#, r#"["README.md"]"#),
        (
            r#"find "\\.rs$" with { also matching ["^m"] }"#,
            r#"["src/main.rs"]"#,
        ),
        (
            r#"find "src/d" with { `full path }"#,
            r#"["src/deep", "src/deep/inner.rs"]"#,
        ),
        (r#"find "" with { at least 14 bytes }"#, r#"["src/lib.rs"]"#),
        (r#"find "" with { at most 1 results }"#, r#"["README.md"]"#),
        (r#"find "^s" with { `prune }"#, r#"["src"]"#),
        (
            r#"find "" with { changed within (1 hours), only [`file], at most 1 results }"#,
            r#"["README.md"]"#,
        ),
        (r#"find "" with { changed more than (1 hours) ago }"#, "[]"),
    ] {
        assert_eq!(
            answer(directory.path(), request),
            format!("RETURNS {expected}\n"),
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
        "RETURNS [\"src/deep/inner.rs\", \"src/lib.rs\", \"src/main.rs\"]\n"
    );
    assert_eq!(
        answer(directory.path(), r#"find "x" in ["missing"]"#),
        "ERROR `\"missing\" does not exist\n"
    );
}

#[test]
fn details_and_any_match() {
    let directory = tree();
    let details = answer(directory.path(), r#"details of "lib""#);
    assert!(
        details.starts_with("RETURNS [{ path \"src/lib.rs\", kind: `file, size 14, modified \""),
        "{details}"
    );
    assert_eq!(
        answer(directory.path(), r#"any match of "main""#),
        "RETURNS `true\n"
    );
    assert_eq!(
        answer(directory.path(), r#"any match of "main" in ["src/deep"]"#),
        "RETURNS `false\n"
    );
}

#[test]
fn mistakes_are_errors_or_invalid_requests() {
    let directory = tree();
    assert_eq!(
        answer(directory.path(), r#"find "(""#),
        "ERROR `invalid pattern \"unclosed group\"\n"
    );
    assert!(
        answer(directory.path(), r#"find "[" with { `glob pattern }"#)
            .starts_with("ERROR `invalid glob \"")
    );
    // A search says each thing once, so it can't contradict itself.
    for (request, expected) in [
        (
            r#"find "x" with { `glob pattern, `fixed text pattern }"#,
            "pattern",
        ),
        (r#"find "x" with { `hidden, `hidden }"#, "hidden"),
        (r#"find "x" with { glob }"#, "expected a field"),
        (r#"find "x" with { at most 0 results }"#, "at most"),
    ] {
        let response = answer(directory.path(), request);
        assert!(
            response.starts_with("INVALID REQUEST"),
            "{request}: {response}"
        );
        assert!(response.contains(expected), "{request}: {response}");
    }
}

#[test]
fn every_call_of_a_request_runs() {
    let directory = tree();
    assert_eq!(
        answer(directory.path(), r#"find "(", any match of "lib""#),
        "ERROR `invalid pattern \"unclosed group\",\nRETURNS `true,\n"
    );
}

#[test]
fn the_examples_and_built_ins_answer() {
    let directory = tree();
    let guide = answer(directory.path(), "SCHEMA");
    assert!(guide.contains("find (pattern: Text)"), "{guide}");
    let files = answer(directory.path(), "SCHEMA FILES");
    assert!(files.contains("path \"fd.protocol.anyx\""), "{files}");
}

#[test]
fn the_examples_hold() {
    use anyx_protocol::{Call, Response};
    // The examples search a crate with a readme, a library and a program.
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("src")).unwrap();
    fs::write(directory.path().join("src/lib.rs"), "").unwrap();
    fs::write(directory.path().join("src/main.rs"), "").unwrap();
    fs::write(directory.path().join("README.md"), "").unwrap();
    let schema = anyx_fd::schema();
    let mut count = 0;
    for (index, method) in schema.methods.iter().enumerate() {
        for example in &method.examples {
            let call = Call {
                method: index,
                arguments: example.arguments.clone(),
            };
            let expected = anyx_protocol::encode_response(
                schema,
                &[call],
                &Response::Outcomes(vec![example.outcome.clone()]),
            )
            .unwrap();
            assert_eq!(
                answer(directory.path(), &example.call),
                expected,
                "{}",
                example.text
            );
            count += 1;
        }
    }
    assert_eq!(count, 3);
}

#[test]
fn the_shell_brings_in_the_program_and_calls_it() {
    let directory = tree();
    let program = env!("CARGO_BIN_EXE_anyx-fd");
    let root = directory.path().to_str().unwrap();
    let mut shell = anyx_shell::Interpreter::new();
    let source = format!(
        "USE fd FROM COMMAND {program:?},\n\
         fd / find \"lib\" in [{root:?}] with {{ only [`file] }},\n\
         USE ALL FROM fd,\n\
         any match of \"main\" in [{root:?}] with {{ max depth 2 }},\n"
    );
    let execution = shell.execute(&source).unwrap();
    assert_eq!(execution.error, None);
    assert_eq!(
        execution.output,
        format!("[{:?}],\n`true,\n", format!("{root}/src/lib.rs"))
    );
    // A call the schema rules out never reaches the program.
    let problem = shell
        .execute("find \"x\" with { `hidden, `hidden }")
        .unwrap_err();
    assert!(
        problem.message.contains("duplicate field `hidden`"),
        "{}",
        problem.message
    );
    // The program's errors come back as they are.
    let execution = shell.execute("find \"(\"").unwrap();
    assert_eq!(
        execution.error.as_deref(),
        Some("ERROR `invalid pattern \"unclosed group\"\n")
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
