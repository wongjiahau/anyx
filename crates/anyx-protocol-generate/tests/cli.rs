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
        .args(["typescript", &fixture("protocol.anyx")])
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
    for path in [fixture("common.anyx"), fixture("")] {
        let output = Command::new(env!("CARGO_BIN_EXE_anyx-protocol-generate"))
            .args(["rust", &path])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8(output.stderr).unwrap();
        if path.ends_with("common.anyx") {
            assert!(error.contains("a library has no endpoint"), "{error}");
        } else {
            assert!(error.contains("cannot read"), "{error}");
        }
    }
}
