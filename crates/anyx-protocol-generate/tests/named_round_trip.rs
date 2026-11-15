//! The generated Rust module of named types, compiled: each is a struct of
//! its own around the type underneath, and the wire doesn't tell them apart
//! from it.

#[path = "generated/named.rs"]
mod named;

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use anyx_protocol::client::{CallError, Transport};
use named::{
    Access, Client, FileId, FileLink, Level, Link, OpenedAt, Openings, Server, ShareId,
    ShareTaggedError, Tags, Visit,
};

/// A visit at one second after 1970, by one person.
fn visit() -> Visit {
    let at = OpenedAt(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1));
    Visit {
        at: at.clone(),
        by_person: Openings([("ann".to_string(), at)].into()),
        label: ShareId("none".to_string()),
    }
}

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

struct Sharing;

impl Server for Sharing {
    type Error = std::convert::Infallible;

    async fn share_tagged(
        &self,
        id: FileId,
        tags: Tags,
    ) -> Result<Result<Link, ShareTaggedError>, Self::Error> {
        if id.0 == "gone" {
            return Ok(Err(ShareTaggedError::Missing { id }));
        }
        Ok(Ok(Link(FileLink {
            share_id: ShareId(format!("share of {}", id.0)),
            file_id: id,
            tags,
        })))
    }

    async fn revoke(&self, _id: ShareId) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn restore(&self, _id: ShareId) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn links(&self) -> Result<Vec<ShareId>, Self::Error> {
        Ok(vec![ShareId::from("a".to_string()), "b".to_string().into()])
    }

    async fn grant_on(&self, level: Level, _id: FileId) -> Result<Level, Self::Error> {
        Ok(level)
    }

    async fn visits(&self) -> Result<Vec<Visit>, Self::Error> {
        Ok(vec![visit()])
    }
}

/// Sends each request straight to `handle`, keeping the requests and the
/// responses.
struct InProcess {
    exchanged: std::cell::RefCell<Vec<(String, String)>>,
}

impl Transport for InProcess {
    type Error = std::convert::Infallible;

    async fn send(&self, request: String) -> Result<String, Self::Error> {
        let response = named::handle(&Sharing, &request, |error| match error {}).await;
        self.exchanged
            .borrow_mut()
            .push((request, response.clone()));
        Ok(response)
    }
}

fn client() -> Client<InProcess> {
    Client::new(InProcess {
        exchanged: std::cell::RefCell::new(Vec::new()),
    })
}

#[test]
fn named_types_cross_the_wire_as_the_types_underneath() {
    let client = client();
    let tags = Tags(vec!["hymn".to_string()]);
    assert_eq!(
        run(client.share_tagged(FileId("a".to_string()), tags.clone())),
        Ok(Link(FileLink {
            share_id: ShareId("share of a".to_string()),
            file_id: FileId("a".to_string()),
            tags,
        }))
    );
    assert_eq!(
        run(client.grant_on(Level(Access::Write), FileId("a".to_string()))),
        Ok(Level(Access::Write))
    );
    assert_eq!(
        run(client.links()),
        Ok(vec![ShareId("a".to_string()), ShareId("b".to_string())])
    );
    assert_eq!(
        String::from(FileId("a".to_string())),
        "a",
        "a named type converts back to the type underneath"
    );
    let exchanged = client.transport.exchanged.borrow();
    assert_eq!(
        exchanged[0],
        (
            "share (\"a\" AS File Id) tagged ([\"hymn\"] AS Tags)\n".to_string(),
            "RETURNS { share id \"share of a\", file id \"a\", tags [\"hymn\"] }\n".to_string()
        )
    );
    assert_eq!(exchanged[1].1, "RETURNS write\n");
}

#[test]
fn an_error_holds_its_named_type() {
    assert_eq!(
        run(client().share_tagged(FileId("gone".to_string()), Tags(Vec::new()))),
        Err(CallError::Method(ShareTaggedError::Missing {
            id: FileId("gone".to_string())
        }))
    );
}

#[test]
fn a_named_type_over_an_instant_or_a_map_crosses_the_wire() {
    let client = client();
    assert_eq!(run(client.visits()), Ok(vec![visit()]));
    assert_eq!(
        client.transport.exchanged.borrow()[0].1,
        "RETURNS [\n    { at \"1970-01-01T00:00:01.000Z\", by person { \"ann\" \"1970-01-01T00:00:01.000Z\" }, label \"none\" },\n]\n"
    );
}

#[test]
fn each_method_has_its_effects_as_a_constant() {
    let revoke = named::REVOKE_EFFECTS;
    assert!(!revoke.read_only);
    assert!(revoke.idempotent && revoke.may_retry());
    assert_eq!(revoke.undone_by, Some("restore (id: Share Id)"));
    assert!(named::LINKS_EFFECTS.read_only && named::LINKS_EFFECTS.may_retry());
    // No markers: the worst case, never retried on its own.
    let share = named::SHARE_TAGGED_EFFECTS;
    assert!(!share.read_only && !share.may_retry());
    assert_eq!(share.undone_by, None);
}
