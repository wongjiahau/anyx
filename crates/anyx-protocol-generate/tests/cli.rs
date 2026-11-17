use std::process::Command;

fn fixture(path: &str) -> String {
    format!(
        "{}/../anyx-protocol/tests/fixtures/scoped/{path}",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn entry_file_follows_imports_and_emits_a_flat_client() {
    let output = Command::new(env!("CARGO_BIN_EXE_anyx-protocol-generate"))
        .args(["typescript", &fixture("protocol.protocol.anyx")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let code = String::from_utf8(output.stdout).unwrap();
    assert!(code.contains("export interface Diagnostic"));
    assert!(code.contains("  pdf(path: string)"));
    assert!(!code.contains("FilesExport"));
    assert!(!code.contains("readonly files"));
}

#[test]
fn libraries_and_folder_input_are_rejected() {
    for path in [fixture("common.protocol.anyx"), fixture("")] {
        let output = Command::new(env!("CARGO_BIN_EXE_anyx-protocol-generate"))
            .args(["rust", &path])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8(output.stderr).unwrap();
        if path.ends_with("common.protocol.anyx") {
            assert!(error.contains("a library has no endpoint"), "{error}");
        } else {
            assert!(error.contains("ending in `.protocol.anyx`"), "{error}");
        }
    }
}

#[test]
fn generation_requires_the_exact_protocol_suffix_for_entries_and_imports() {
    let directory =
        std::env::temp_dir().join(format!("anyx-protocol-extensions-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    for name in [
        "input.anyx",
        "input.grammar.anyx",
        "input.PROTOCOL.anyx",
        "input.protocol.anyx.bak",
    ] {
        let path = directory.join(name);
        std::fs::write(&path, "PROTOCOL example VERSION 1").unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_anyx-protocol-generate"))
            .arg("typescript")
            .arg(&path)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("ending in `.protocol.anyx`"));
        let entry = directory.join("entry.protocol.anyx");
        std::fs::write(
            &entry,
            format!("PROTOCOL example VERSION 1, USE \"./{name}\""),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_anyx-protocol-generate"))
            .arg("typescript")
            .arg(&entry)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("ending in `.protocol.anyx`"));
    }
    std::fs::remove_dir_all(directory).unwrap();
}
