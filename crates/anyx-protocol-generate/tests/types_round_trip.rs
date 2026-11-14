//! The Rust module generated from `tests/fixtures/types.anyx`, compiled: a
//! client calls a server in the same process with every kind of type.

#[path = "generated/types.rs"]
mod types;

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
            Some(period) => Schedule::Every { period },
            None => Schedule::OnceAt { at: reminder.at },
        })
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
        every: None,
    };
    assert_eq!(
        run(client.remind(reminder.clone())),
        Ok(Schedule::OnceAt {
            at: at(1_794_636_008_123)
        })
    );
    assert_eq!(
        run(client.remind(Reminder {
            every: Some(Duration::from_secs(5400)),
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
