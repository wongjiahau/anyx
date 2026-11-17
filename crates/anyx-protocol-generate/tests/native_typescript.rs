//! End-to-end exercise of the generated TypeScript client and Rust server.
#[path = "generated/types.rs"]
mod types;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::pin::pin;
use std::process::{Command, Stdio};
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use types::{Reminder, Schedule, Server, SnoozeForError};

struct Handler;
impl Server for Handler {
    type Error = std::convert::Infallible;
    async fn remind(&self, reminder: Reminder) -> Result<Schedule, Self::Error> {
        Ok(Schedule::OnceAt { at: reminder.at })
    }
    async fn count(
        &self,
        reminders: BTreeMap<String, Reminder>,
    ) -> Result<BTreeMap<String, i64>, Self::Error> {
        Ok(reminders
            .into_iter()
            .map(|(key, value)| (key, value.times))
            .collect())
    }
    async fn snooze_for(
        &self,
        reminder: Reminder,
        _: Duration,
    ) -> Result<Result<Vec<Reminder>, SnoozeForError>, Self::Error> {
        Ok(Err(SnoozeForError::OverdueSince { since: reminder.at }))
    }
}
fn run<T>(future: impl std::future::Future<Output = T>) -> T {
    match pin!(future)
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("server unexpectedly waited"),
    }
}

#[test]
fn generated_client_exchanges_text_with_generated_server() {
    let dir = std::env::temp_dir().join(format!("anyx-native-ts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/generated/types.ts");
    let compile = Command::new("tsc")
        .args([
            "--skipLibCheck",
            "--target",
            "es2022",
            "--module",
            "commonjs",
            "--outDir",
        ])
        .arg(&dir)
        .arg(source)
        .output()
        .expect("tsc is required for the TypeScript integration test");
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stdout)
    );
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native_typescript.cjs");
    let mut child = Command::new("node")
        .arg(script)
        .arg(dir.join("types.js"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("node is required");
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    loop {
        let mut line = String::new();
        if output.read_line(&mut line).unwrap() == 0 {
            break;
        }
        let request: String = serde_json::from_str(&line).expect("request JSON from Node");
        let response = run(types::handle(&Handler, &request, |_| {}));
        writeln!(input, "{}", serde_json::to_string(&response).unwrap()).unwrap();
        input.flush().unwrap();
    }
    assert!(child.wait().unwrap().success());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_codec_conforms_with_rust_for_composite_values() {
    use anyx_protocol::{Outcome, Response, Schema, decode_request, encode_response};
    let schema = Schema::parse(include_str!("fixtures/native.protocol.anyx")).unwrap();
    let dir = std::env::temp_dir().join(format!("anyx-conformance-ts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("native.ts"),
        anyx_protocol_generate::typescript(&schema).unwrap(),
    )
    .unwrap();
    let compile = Command::new("tsc")
        .args([
            "--skipLibCheck",
            "--target",
            "es2022",
            "--module",
            "commonjs",
            "--outDir",
        ])
        .arg(&dir)
        .arg(dir.join("native.ts"))
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stdout)
    );
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native_conformance.cjs");
    let mut child = Command::new("node")
        .arg(script)
        .arg(dir.join("native.js"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    loop {
        let mut line = String::new();
        if output.read_line(&mut line).unwrap() == 0 {
            break;
        }
        let request: String = serde_json::from_str(&line).unwrap();
        let calls = decode_request(&schema, &request).unwrap();
        let values = calls
            .iter()
            .map(|call| Outcome::Value(call.arguments[0].clone()))
            .collect();
        let response = encode_response(&schema, &calls, &Response::Outcomes(values)).unwrap();
        writeln!(input, "{}", serde_json::to_string(&response).unwrap()).unwrap();
        input.flush().unwrap();
    }
    assert!(child.wait().unwrap().success());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_codec_handles_recursive_generic_types_and_defaults() {
    let dir = std::env::temp_dir().join(format!("anyx-generic-ts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/generated/generic.ts");
    let compile = Command::new("tsc")
        .args([
            "--skipLibCheck",
            "--target",
            "es2022",
            "--module",
            "commonjs",
            "--outDir",
        ])
        .arg(&dir)
        .arg(source)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stdout)
    );
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native_generic.cjs");
    let result = Command::new("node")
        .arg(script)
        .arg(dir.join("generic.js"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn standalone_es_module_loads_in_node() {
    let dir = std::env::temp_dir().join(format!("anyx-esm-ts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/generated/types.ts");
    let compile = Command::new("tsc")
        .args([
            "--skipLibCheck",
            "--target",
            "es2022",
            "--module",
            "es2022",
            "--outDir",
        ])
        .arg(&dir)
        .arg(source)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stdout)
    );
    std::fs::rename(dir.join("types.js"), dir.join("types.mjs")).unwrap();
    let result = Command::new("node").args(["--input-type=module", "-e", "const m = await import(process.argv[1]); const x = await new m.Client(async () => 'RETURNS \"ok\"').schema(); if (x.value !== 'ok') throw Error('ESM smoke failed');"]).arg(dir.join("types.mjs")).output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::remove_dir_all(dir).unwrap();
}
