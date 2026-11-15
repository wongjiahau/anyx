//! The generated Rust module of overloads, compiled: each overload is a
//! method of its own, the server runs the one a call resolves to, and the
//! client annotates what its arguments' types leave open.

#[path = "generated/overloaded.rs"]
mod overloaded;

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use anyx_protocol::client::{CallError, Transport};
use overloaded::{Client, FileId, Server, Shade, TagWithListOfFileIdError, TagWithListOfTextError};

/// Runs a future that never waits, as every future here is.
fn run<T>(future: impl Future<Output = T>) -> T {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("the future waited"),
    }
}

/// Answers each overload differently, so a test sees which one ran.
struct Tagging;

impl Server for Tagging {
    type Error = std::convert::Infallible;

    async fn tag_with_list_of_text(
        &self,
        paths: Vec<String>,
    ) -> Result<Result<i64, TagWithListOfTextError>, Self::Error> {
        match paths.iter().find(|path| path.is_empty()) {
            Some(path) => Ok(Err(TagWithListOfTextError::Missing { path: path.clone() })),
            None => Ok(Ok(paths.len() as i64)),
        }
    }

    async fn tag_with_list_of_file_id(
        &self,
        ids: Vec<FileId>,
    ) -> Result<Result<i64, TagWithListOfFileIdError>, Self::Error> {
        match ids.iter().find(|id| id.0.is_empty()) {
            Some(id) => Ok(Err(TagWithListOfFileIdError::Unknown { id: id.clone() })),
            None => Ok(Ok(100 + ids.len() as i64)),
        }
    }

    async fn scale_with_float(&self, factor: f64) -> Result<f64, Self::Error> {
        Ok(factor * 2.0)
    }

    async fn scale_with_integer(&self, times: i64) -> Result<i64, Self::Error> {
        Ok(times * 3)
    }

    async fn hide_with_text(&self, _path: String) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn hide_with_file_id(&self, _id: FileId) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn show_with_text(&self, _path: String) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn show_with_file_id(&self, _id: FileId) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn paint(&self, _shade: Shade) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Sends each request straight to `handle`, keeping the requests.
struct InProcess {
    requests: std::cell::RefCell<Vec<String>>,
}

impl Transport for InProcess {
    type Error = std::convert::Infallible;

    async fn send(&self, request: String) -> Result<String, Self::Error> {
        self.requests.borrow_mut().push(request.clone());
        Ok(overloaded::handle(&Tagging, &request, |error| match error {}).await)
    }
}

fn client() -> Client<InProcess> {
    Client::new(InProcess {
        requests: std::cell::RefCell::new(Vec::new()),
    })
}

#[test]
fn the_server_runs_the_overload_a_call_resolves_to() {
    let response = |request: &str| {
        run(overloaded::handle(
            &Tagging,
            request,
            |error| match error {},
        ))
    };
    assert_eq!(response("tag [\"a\", \"b\"]"), "RETURNS 2\n");
    assert_eq!(response("tag ([\"a\"] AS: LIST: file id)"), "RETURNS 101\n");
    assert_eq!(response("scale 1.5"), "RETURNS 3.0\n");
    assert_eq!(response("scale 2"), "RETURNS 6\n");
    assert!(
        response("tag []").starts_with("INVALID REQUEST"),
        "`[]` has no type of its own"
    );
}

#[test]
fn the_client_annotates_what_its_arguments_leave_open() {
    let client = client();
    assert_eq!(run(client.tag_with_list_of_text(Vec::new())), Ok(0));
    assert_eq!(
        run(client.tag_with_list_of_file_id(vec![FileId("a".to_string())])),
        Ok(101)
    );
    assert_eq!(run(client.scale_with_float(3.0)), Ok(6.0));
    assert_eq!(run(client.scale_with_integer(3)), Ok(9));
    assert_eq!(run(client.paint(Shade::Dark)), Ok(()));
    assert_eq!(
        *client.transport.requests.borrow(),
        [
            "tag ([] AS: LIST: TEXT)\n",
            "tag ([\"a\"] AS: LIST file id)\n",
            "scale 3.0\n",
            "scale 3\n",
            "paint (dark)\n",
        ]
    );
}

#[test]
fn each_overload_has_its_errors() {
    assert_eq!(
        run(client().tag_with_list_of_file_id(vec![FileId(String::new())])),
        Err(CallError::Method(TagWithListOfFileIdError::Unknown {
            id: FileId(String::new())
        }))
    );
}

#[test]
fn each_overload_has_its_effects() {
    let scale = overloaded::SCALE_WITH_FLOAT_EFFECTS;
    assert!(scale.read_only && scale.may_retry());
    assert_eq!(
        overloaded::HIDE_WITH_FILE_ID_EFFECTS.undone_by,
        Some("show (id: file id)")
    );
}
