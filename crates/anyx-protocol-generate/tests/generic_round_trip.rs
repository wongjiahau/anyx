//! The Rust module generated from `tests/fixtures/generic.anyx`, compiled: a
//! client calls a server in the same process with generic records and
//! choices, nested uses of them and a named type over one.

#[path = "generated/generic.rs"]
mod generic;

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyx_protocol::client::Transport;
use generic::{Attempt, Client, Dated, Decline, Page, Payment, Payments, Server, Tree};

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

fn at(milliseconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(milliseconds)
}

fn payment(amount: i64) -> Payment {
    Payment {
        amount,
        at: at(1_794_636_008_123),
    }
}

fn attempt(amount: i64) -> Attempt<Payment, Decline> {
    match amount {
        amount if amount > 100 => Attempt::Failed {
            problem: Decline::InsufficientFunds,
        },
        amount => Attempt::Done {
            value: payment(amount),
        },
    }
}

struct Shop;

impl Server for Shop {
    type Error = std::convert::Infallible;

    async fn pay(&self, amount: i64) -> Result<Attempt<Payment, Decline>, Self::Error> {
        Ok(attempt(amount))
    }

    async fn pay_all(
        &self,
        amounts: Vec<i64>,
    ) -> Result<Page<Attempt<Payment, Decline>>, Self::Error> {
        Ok(Page {
            total: amounts.len() as i64,
            items: amounts.into_iter().map(attempt).collect(),
            next: None,
        })
    }

    async fn payments(&self) -> Result<Payments, Self::Error> {
        Ok(Payments(Page {
            items: vec![payment(3)],
            next: Some("2".to_string()),
            total: 7,
        }))
    }

    async fn count(&self, payments: Page<Payment>) -> Result<i64, Self::Error> {
        Ok(payments.items.iter().map(|payment| payment.amount).sum())
    }

    async fn categories(&self) -> Result<Tree<String>, Self::Error> {
        Ok(Tree {
            value: "food".to_string(),
            children: vec![Tree {
                value: "fruit".to_string(),
                children: vec![],
            }],
        })
    }

    async fn latest(&self) -> Result<Dated<String>, Self::Error> {
        Ok(Dated {
            item: "tea".to_string(),
            at: at(0),
        })
    }
}

struct InProcess;

impl Transport for InProcess {
    type Error = std::convert::Infallible;

    async fn send(&self, request: String) -> Result<String, Self::Error> {
        Ok(generic::handle(&Shop, &request, |_| {}).await)
    }
}

#[test]
fn uses_of_generic_types_reach_the_server_and_come_back() {
    let client = Client::new(InProcess);
    assert_eq!(run(client.pay(3)), Ok(attempt(3)));
    assert_eq!(run(client.pay(300)), Ok(attempt(300)));
    assert_eq!(
        run(client.pay_all(vec![3, 300])),
        Ok(Page {
            items: vec![attempt(3), attempt(300)],
            next: None,
            total: 2,
        })
    );
    assert_eq!(
        run(client.count(Page {
            items: vec![payment(3), payment(4)],
            next: None,
            total: 0,
        })),
        Ok(7)
    );
    assert_eq!(
        run(client.categories()),
        Ok(Tree {
            value: "food".to_string(),
            children: vec![Tree {
                value: "fruit".to_string(),
                children: vec![],
            }],
        })
    );
    assert_eq!(
        run(client.latest()),
        Ok(Dated {
            item: "tea".to_string(),
            at: at(0),
        })
    );
}

#[test]
fn a_named_type_over_a_generic_type_is_written_like_it() {
    let client = Client::new(InProcess);
    assert_eq!(
        run(client.payments()).map(|payments| payments.0.items),
        Ok(vec![payment(3)])
    );
    let response = run(generic::handle(&Shop, "payments", |_| {}));
    assert_eq!(
        response,
        "RETURNS { items [{ amount 3, at \"2026-11-14T06:00:08.123Z\" }], next \"2\", total 7 }\n"
    );
}

#[test]
fn a_field_left_out_of_a_generic_record_reaches_the_server_with_its_default() {
    let response = run(generic::handle(
        &Shop,
        "count { items [{ amount 3, at \"2026-11-14T06:00:08Z\" }] }",
        |_| {},
    ));
    assert_eq!(response, "RETURNS 3\n");
}
