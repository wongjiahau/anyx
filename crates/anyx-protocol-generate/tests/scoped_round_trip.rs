//! The flat client and server generated from a protocol split across files.

#[path = "generated/scoped.rs"]
mod notes;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use anyx_protocol::client::{CallError, Transport};
use notes::Client;
use notes::Diagnostic;
use notes::{File, ReadError, TrashError, WriteAtRevisionContentError};
use notes::{HtmlError, Page, PdfError};

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

/// Notes by path, with their revision and content.
struct Notes {
    notes: RefCell<BTreeMap<String, (i64, String)>>,
}

impl notes::Server for Notes {
    type Error = std::convert::Infallible;
    async fn list(&self) -> Result<Vec<String>, Self::Error> {
        Ok(self.notes.borrow().keys().cloned().collect())
    }

    async fn read(&self, path: String) -> Result<Result<File, ReadError>, Self::Error> {
        Ok(match path.as_str() {
            "old" => Err(ReadError::Trashed),
            _ => match self.notes.borrow().get(&path) {
                Some((revision, content)) => Ok(File {
                    path,
                    revision: *revision,
                    content: content.clone(),
                }),
                None => Err(ReadError::Missing),
            },
        })
    }

    async fn write_at_revision_content(
        &self,
        path: String,
        revision: i64,
        content: String,
    ) -> Result<Result<Vec<Diagnostic>, WriteAtRevisionContentError>, Self::Error> {
        if content.is_empty() {
            return Ok(Err(WriteAtRevisionContentError::Rejected {
                diagnostics: vec![Diagnostic {
                    line: 1,
                    message: "empty".into(),
                }],
            }));
        }
        self.notes
            .borrow_mut()
            .insert(path, (revision + 1, content));
        Ok(Ok(vec![]))
    }

    async fn trash(&self, path: String) -> Result<Result<(), TrashError>, Self::Error> {
        Ok(match self.notes.borrow_mut().remove(&path) {
            Some(_) => Ok(()),
            None => Err(TrashError::Missing),
        })
    }

    async fn empty_trash(&self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn html(&self, path: String) -> Result<Result<Vec<Page>, HtmlError>, Self::Error> {
        Ok(match self.notes.borrow().get(&path) {
            Some((_, content)) => Ok(vec![Page {
                html: format!("<p>{content}</p>"),
                problem: notes::Nullable::Null,
            }]),
            None => Err(HtmlError::Missing),
        })
    }

    async fn pdf(&self, path: String) -> Result<Result<Vec<u8>, PdfError>, Self::Error> {
        Ok(match self.notes.borrow().get(&path) {
            Some((_, content)) => Ok(content.as_bytes().to_vec()),
            None => Err(PdfError::Unreadable {
                diagnostics: vec![],
            }),
        })
    }
}

struct InProcess {
    server: Notes,
}

impl Transport for InProcess {
    type Error = std::convert::Infallible;

    async fn send(&self, request: String) -> Result<String, Self::Error> {
        Ok(notes::handle(&self.server, &request, |error| match error {}).await)
    }
}

fn client() -> Client<InProcess> {
    Client::new(InProcess {
        server: Notes {
            notes: RefCell::new(BTreeMap::new()),
        },
    })
}

#[test]
fn a_client_calls_methods_from_each_file() {
    let client = client();
    assert_eq!(
        run(client.write_at_revision_content("a".into(), 0, "hi".into())),
        Ok(vec![])
    );
    assert_eq!(run(client.list()), Ok(vec!["a".to_string()]));
    assert_eq!(
        run(client.read("a".into())),
        Ok(File {
            path: "a".into(),
            revision: 1,
            content: "hi".into()
        })
    );
    assert_eq!(
        run(client.html("a".into())),
        Ok(vec![Page {
            html: "<p>hi</p>".into(),
            problem: notes::Nullable::Null
        }])
    );
    assert_eq!(run(client.pdf("a".into())), Ok(b"hi".to_vec()));
}

#[test]
fn imported_errors_reach_the_client() {
    let client = client();
    assert_eq!(
        run(client.read("old".into())),
        Err(CallError::Method(ReadError::Trashed))
    );
    assert_eq!(
        run(client.html("b".into())),
        Err(CallError::Method(HtmlError::Missing))
    );
    assert_eq!(
        run(client.write_at_revision_content("a".into(), 0, String::new())),
        Err(CallError::Method(WriteAtRevisionContentError::Rejected {
            diagnostics: vec![Diagnostic {
                line: 1,
                message: "empty".into()
            }]
        }))
    );
}

#[test]
fn a_request_calls_each_method_by_its_full_name() {
    let server = Notes {
        notes: RefCell::new(BTreeMap::from([("a".to_string(), (1, "hi".to_string()))])),
    };
    let response = run(notes::handle(
        &server,
        "list, pdf \"a\"",
        |error| match error {},
    ));
    assert_eq!(response, "RETURNS [\"a\"],\nRETURNS \"aGk=\",\n");
}

#[test]
fn a_method_that_returns_nothing_answers_returns_nothing() {
    let server = Notes {
        notes: RefCell::new(BTreeMap::from([("a".to_string(), (1, "hi".to_string()))])),
    };
    let response = run(notes::handle(
        &server,
        "trash \"a\", trash \"a\", empty trash",
        |error| match error {},
    ));
    assert_eq!(
        response,
        "RETURNS NOTHING,\nERROR missing,\nRETURNS NOTHING,\n"
    );
}

#[test]
fn a_client_gets_unit_from_a_method_that_returns_nothing() {
    let client = client();
    run(client.write_at_revision_content("a".into(), 0, "hi".into())).unwrap();
    assert_eq!(run(client.trash("a".into())), Ok(()));
    assert_eq!(
        run(client.trash("a".into())),
        Err(CallError::Method(TrashError::Missing))
    );
    assert_eq!(run(client.empty_trash()), Ok(()));
}

#[test]
fn the_client_reads_the_guide_through_schema() {
    let guide = run(client().schema()).unwrap();
    assert_eq!(guide, anyx_protocol::guide(notes::schema(), ""));
    assert!(guide.contains("pdf"), "{guide}");
}
