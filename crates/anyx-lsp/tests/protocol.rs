#![cfg(feature = "protocol")]

use anyx_lsp::{Language, Protocol, Server};
use lsp_server::{Message, Notification};
use serde_json::{Value, json};

fn notify(server: &mut Server<'_>, method: &str, params: Value) -> Vec<Value> {
    server
        .handle(Message::Notification(Notification::new(
            method.into(),
            params,
        )))
        .into_iter()
        .map(|message| match message {
            Message::Notification(notification) => notification.params,
            _ => panic!("expected diagnostics"),
        })
        .collect()
}

fn open(server: &mut Server<'_>, uri: &str, text: &str) -> Vec<Value> {
    notify(
        server,
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": "anyx", "version": 1, "text": text}}),
    )
}

#[test]
fn imports_use_open_buffers_and_recheck_dependents_on_change_and_close() {
    let mut server = Server::new(&Protocol);
    let entry = "file:///notes/protocol.protocol.anyx";
    let library = "file:///shared.anyx";
    let messages = open(
        &mut server,
        entry,
        "PROTOCOL notes VERSION 1, USE \"../shared.anyx\", TYPE Result { value: Item },",
    );
    assert!(
        messages[0]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("open the imported file")
    );

    let messages = open(&mut server, library, "PUBLIC TYPE Item { value: Text },");
    assert_eq!(messages.len(), 2);
    assert!(
        messages
            .iter()
            .all(|message| message["diagnostics"] == json!([]))
    );

    let messages = notify(
        &mut server,
        "textDocument/didChange",
        json!({"textDocument": {"uri": library, "version": 2}, "contentChanges": [{"text": "TYPE Item { value: Text },"}]}),
    );
    let entry_diagnostics = messages
        .iter()
        .find(|message| message["uri"] == entry)
        .unwrap();
    assert!(
        entry_diagnostics["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("not visible")
    );

    let messages = notify(
        &mut server,
        "textDocument/didClose",
        json!({"textDocument": {"uri": library}}),
    );
    assert!(
        messages
            .iter()
            .any(|message| message["uri"] == library && message["diagnostics"] == json!([]))
    );
    let entry_diagnostics = messages
        .iter()
        .find(|message| message["uri"] == entry)
        .unwrap();
    assert!(
        entry_diagnostics["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("cannot read")
    );
}

#[test]
fn diagnostics_in_imported_libraries_are_published_at_their_own_locations() {
    let mut server = Server::new(&Protocol);
    open(
        &mut server,
        "file:///notes/entry.protocol.anyx",
        "PROTOCOL p VERSION 1, USE \"../shared.anyx\",",
    );
    let messages = open(
        &mut server,
        "file:///shared.anyx",
        "TYPE Item { value: missing },",
    );
    let library = messages
        .iter()
        .find(|message| message["uri"] == "file:///shared.anyx")
        .unwrap();
    assert_eq!(library["diagnostics"].as_array().unwrap().len(), 1);
    assert!(
        library["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("missing")
    );
    assert_eq!(library["diagnostics"][0]["range"]["start"]["character"], 19);
}

#[test]
fn checking_a_library_needs_no_header() {
    assert!(Protocol.check("TYPE Item { value: Text },").is_empty());
    assert!(
        Protocol
            .format("USE \"./shared.anyx\", TYPE Item {value: Text}")
            .unwrap()
            .contains("USE")
    );
}

#[test]
fn filenames_with_spaces_resolve_and_self_imports_are_read_once() {
    let mut server = Server::new(&Protocol);
    let entry = "file:///notes/Entry%20File.protocol.anyx";
    let library = "file:///shared%20types.protocol.anyx";
    open(
        &mut server,
        entry,
        "PROTOCOL notes VERSION 1, USE \"../shared types.protocol.anyx\",",
    );
    let messages = open(&mut server, library, "PUBLIC TYPE Item { value: Text },");
    assert!(
        messages
            .iter()
            .all(|message| message["diagnostics"] == json!([]))
    );
    let messages = notify(
        &mut server,
        "textDocument/didChange",
        json!({"textDocument": {"uri": library, "version": 2}, "contentChanges": [{"text": "USE \"./shared types.protocol.anyx\","}]}),
    );
    let diagnostics = &messages
        .iter()
        .find(|message| message["uri"] == library)
        .unwrap()["diagnostics"];
    assert_eq!(diagnostics.as_array().unwrap().len(), 1);
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .contains("/shared types.protocol.anyx → /shared types.protocol.anyx")
    );
}
