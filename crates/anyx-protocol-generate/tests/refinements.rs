#[path = "generated/refined.rs"]
mod refined;
use anyx_protocol::{FromValue, IntoValue, Outcome, Response, Schema, Value};
use refined::{BoxOf, Names, Probability, ScorePath, Server, Summary};
use std::{
    cell::Cell,
    pin::pin,
    process::Command,
    task::{Context, Poll, Waker},
};

#[test]
fn rust_newtypes_have_checked_constructors_and_checked_value_conversions() {
    assert_eq!(
        ScorePath::new("a.poentaro".into()).unwrap().get(),
        "a.poentaro"
    );
    assert!(ScorePath::try_from("bad.txt".to_string()).is_err());
    let path = |text: &str| Value::Variant {
        shape: "Score Path _".to_string(),
        arguments: vec![Value::Text(text.to_string())],
    };
    assert!(ScorePath::from_value(path("bad.txt")).is_none());
    assert_eq!(
        ScorePath::new("a.poentaro".into()).unwrap().into_value(),
        path("a.poentaro")
    );
    assert_eq!(
        ScorePath::new("a.poentaro".into()).unwrap().into_inner(),
        "a.poentaro"
    );
    assert!(ScorePath::new("bad.txt".into()).is_err());
    assert!(Probability::new(0.5).is_ok());
    assert!(Probability::new(1.1).is_err());
    assert!(Names::new(vec!["a".into()]).is_ok());
    assert!(Names::new(vec![]).is_err());
}
struct Handler {
    calls: Cell<usize>,
    bad_return: bool,
}
impl Server for Handler {
    type Error = std::convert::Infallible;
    async fn write_at(&self, _: ScorePath, revision: i64) -> Result<i64, Self::Error> {
        self.calls.set(self.calls.get() + 1);
        Ok(if self.bad_return { 9 } else { revision })
    }
    async fn limit(&self, value: i64) -> Result<i64, Self::Error> {
        self.calls.set(self.calls.get() + 1);
        Ok(if self.bad_return { 30 } else { value })
    }
    async fn summary(&self, value: Summary) -> Result<Summary, Self::Error> {
        Ok(value)
    }
    async fn boxed(&self, value: BoxOf<i64>) -> Result<BoxOf<i64>, Self::Error> {
        Ok(value)
    }
    async fn score(&self, value: Probability) -> Result<Probability, Self::Error> {
        Ok(value)
    }
    async fn names(&self, value: Names) -> Result<Names, Self::Error> {
        Ok(value)
    }
}
fn run<T>(future: impl std::future::Future<Output = T>) -> T {
    match pin!(future)
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(x) => x,
        Poll::Pending => panic!("unexpected wait"),
    }
}
#[test]
fn generated_server_rejects_the_whole_invalid_batch_before_running_any_call() {
    let server = Handler {
        calls: Cell::new(0),
        bad_return: false,
    };
    let request =
        "write (Score Path \"a.poentaro\") at 1, limit (30 AS: (Integer) WHERE [10 <= _ < 30])";
    let response = run(refined::handle(&server, request, |e| match e {}));
    assert!(response.starts_with("INVALID REQUEST"), "{response}");
    assert_eq!(server.calls.get(), 0);
}
#[test]
fn a_broken_returned_refinement_stops_the_generated_server() {
    let server = Handler {
        calls: Cell::new(0),
        bad_return: true,
    };
    let request =
        "limit (10 AS: (Integer) WHERE [10 <= _ < 30]), write (Score Path \"a.poentaro\") at 1";
    let response = run(refined::handle(&server, request, |e| match e {}));
    assert_eq!(
        anyx_protocol::decode_response(refined::schema(), &[1, 0], &response).unwrap(),
        Response::Outcomes(vec![Outcome::InternalError, Outcome::NotRun])
    );
    assert_eq!(server.calls.get(), 1);
}
#[test]
fn method_contracts_gate_generated_handler_execution() {
    let server = Handler {
        calls: Cell::new(0),
        bad_return: true,
    };
    let valid = "write (Score Path \"a.poentaro\") at 1";
    let invalid = "write (Score Path \"a.poentaro\") at 10";
    let rejected = run(refined::handle(
        &server,
        &format!("{valid}, {invalid}"),
        |e| match e {},
    ));
    assert!(rejected.starts_with("INVALID REQUEST"), "{rejected}");
    assert!(rejected.contains("REQUIRES"), "{rejected}");
    assert_eq!(server.calls.get(), 0);

    let response = run(refined::handle(
        &server,
        &format!("{valid}, {valid}"),
        |e| match e {},
    ));
    assert_eq!(
        anyx_protocol::decode_response(refined::schema(), &[0, 0], &response).unwrap(),
        Response::Outcomes(vec![Outcome::InternalError, Outcome::NotRun])
    );
    assert_eq!(server.calls.get(), 1);
}
#[test]
fn native_typescript_checks_constructors_requests_and_responses() {
    let schema = Schema::parse(include_str!("fixtures/refined.protocol.anyx")).unwrap();
    let dir = std::env::temp_dir().join(format!("anyx-refined-ts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("refined.ts");
    std::fs::write(
        &source,
        anyx_protocol_generate::typescript(&schema).unwrap(),
    )
    .unwrap();
    let compile = Command::new("tsc")
        .args([
            "--strict",
            "--skipLibCheck",
            "--target",
            "es2022",
            "--module",
            "commonjs",
            "--outDir",
        ])
        .arg(&dir)
        .arg(&source)
        .output()
        .expect("tsc required");
    assert!(
        compile.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let result = Command::new("node")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/refined_typescript.cjs"
        ))
        .arg(dir.join("refined.js"))
        .output()
        .expect("node required");
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let requests: Vec<String> = serde_json::from_slice(&result.stdout).unwrap();
    assert!(!requests.is_empty());
    for request in requests {
        anyx_protocol::decode_request(&schema, &request)
            .unwrap_or_else(|e| panic!("{request}: {e}"));
    }
    std::fs::remove_dir_all(dir).unwrap();
}
