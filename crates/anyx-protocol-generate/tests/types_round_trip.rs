//! The Rust module generated from `tests/fixtures/types.protocol.anyx`, compiled: a
//! client calls a server in the same process with every kind of type.

#[path = "generated/types.rs"]
mod types;

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyx_protocol::client::{CallError, Transport};
use types::{Client, Reminder, Schedule, Server, SnoozeForError};

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

struct Reminders;

impl Server for Reminders {
    type Error = std::convert::Infallible;

    async fn remind(&self, reminder: Reminder) -> Result<Schedule, Self::Error> {
        Ok(match reminder.every {
            types::Nullable::Value { item: period } => Schedule::Every { period },
            types::Nullable::Null => Schedule::OnceAt { at: reminder.at },
        })
    }

    async fn count(
        &self,
        reminders: BTreeMap<String, Reminder>,
    ) -> Result<BTreeMap<String, i64>, Self::Error> {
        Ok(reminders
            .into_iter()
            .map(|(name, reminder)| (name, reminder.times + reminder.seen.len() as i64))
            .collect())
    }

    async fn snooze_for(
        &self,
        reminder: Reminder,
        duration: Duration,
    ) -> Result<Result<Vec<Reminder>, SnoozeForError>, Self::Error> {
        if duration > Duration::from_secs(86_400) {
            return Ok(Err(SnoozeForError::OverdueSince { since: reminder.at }));
        }
        Ok(Ok(vec![Reminder {
            at: reminder.at + duration,
            ..reminder
        }]))
    }
}

struct InProcess;

impl Transport for InProcess {
    type Error = std::convert::Infallible;

    async fn send(&self, request: String) -> Result<String, Self::Error> {
        Ok(types::handle(&Reminders, &request, |_| {}).await)
    }
}

fn at(milliseconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(milliseconds)
}

#[test]
fn instants_and_durations_reach_the_server_and_come_back() {
    let client = Client::new(InProcess);
    let reminder = Reminder {
        note: "tea".into(),
        at: at(1_794_636_008_123),
        every: types::Nullable::Null,
        times: 3,
        seen: BTreeMap::new(),
    };
    assert_eq!(
        run(client.remind(reminder.clone())),
        Ok(Schedule::OnceAt {
            at: at(1_794_636_008_123)
        })
    );
    assert_eq!(
        run(client.remind(Reminder {
            every: types::Nullable::Value {
                item: Duration::from_secs(5400)
            },
            ..reminder.clone()
        })),
        Ok(Schedule::Every {
            period: Duration::from_secs(5400)
        })
    );
    assert_eq!(
        run(client.snooze_for(reminder.clone(), Duration::from_millis(90_001))),
        Ok(vec![Reminder {
            at: at(1_794_636_098_124),
            ..reminder.clone()
        }])
    );
    assert_eq!(
        run(client.snooze_for(reminder, Duration::from_secs(2 * 86_400))),
        Err(CallError::Method(SnoozeForError::OverdueSince {
            since: at(1_794_636_008_123)
        }))
    );
}

#[test]
fn a_field_left_out_reaches_the_server_with_its_default() {
    let response = run(types::handle(
        &Reminders,
        r#"snooze { note "tea", at "2026-11-14T06:00:08Z" } for (1 second)"#,
        |_| {},
    ));
    assert_eq!(
        response,
        "RETURNS [{ note \"tea\", at \"2026-11-14T06:00:09.000Z\", every: null, times 3, seen {} }]\n"
    );
}

#[test]
fn maps_reach_the_server_and_come_back() {
    let client = Client::new(InProcess);
    let reminder = Reminder {
        note: "tea".into(),
        at: at(1_794_636_008_123),
        every: types::Nullable::Null,
        times: 3,
        seen: BTreeMap::from([("ana".to_string(), at(0)), ("bo \"b\"".to_string(), at(1))]),
    };
    let reminders = BTreeMap::from([
        ("tea".to_string(), reminder.clone()),
        (
            "".to_string(),
            Reminder {
                times: 1,
                seen: BTreeMap::new(),
                ..reminder
            },
        ),
    ]);
    assert_eq!(
        run(client.count(reminders)),
        Ok(BTreeMap::from([
            ("".to_string(), 1),
            ("tea".to_string(), 5)
        ]))
    );
    assert_eq!(run(client.count(BTreeMap::new())), Ok(BTreeMap::new()));
}
