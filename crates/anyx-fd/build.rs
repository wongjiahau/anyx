//! Generates fd's server from its core schema `fd.anyx`
//! (`anyx-core-generate`), as the module `protocol` in
//! `OUT_DIR/protocol.rs`, which `lib.rs` includes.

use std::path::{Path, PathBuf};

fn main() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fd.anyx");
    println!("cargo::rerun-if-changed={}", path.display());
    let source = std::fs::read_to_string(&path).expect("fd.anyx is readable");
    let schema =
        anyx_core_generate::load(&source, "fd.anyx").unwrap_or_else(|error| panic!("{error}"));
    let rust = anyx_core_generate::rust(&schema).unwrap_or_else(|error| panic!("fd.anyx: {error}"));
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    std::fs::write(
        out_dir.join("protocol.rs"),
        format!(
            "#[rustfmt::skip]\n#[allow(dead_code, unused_imports, clippy::all, clippy::pedantic)]\npub mod protocol {{\n{rust}\n}}\n"
        ),
    )
    .expect("OUT_DIR is writable");
}
