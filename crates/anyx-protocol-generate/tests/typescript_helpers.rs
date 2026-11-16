use anyx_protocol::Schema;

#[test]
fn clients_without_instant_results_do_not_emit_an_unused_date_helper() {
    for declarations in [
        "METHOD count RETURNS (count: Integer),",
        "TYPE Timestamp: Instant, METHOD count RETURNS (count: Integer),",
        "METHOD save (at: Instant) RETURNS NOTHING,",
    ] {
        let schema = Schema::parse(&format!("PROTOCOL example VERSION 1, {declarations}")).unwrap();
        let generated = anyx_protocol_generate::typescript(&schema).unwrap();
        assert!(!generated.contains("function reviveInstant("));
    }
}

#[test]
fn direct_and_nested_instant_results_keep_the_date_helper() {
    for declarations in [
        "METHOD now RETURNS (at: Instant),",
        "RECORD Entry { at: Instant }, METHOD read RETURNS (entry: Entry),",
        "METHOD read RETURNS (text: Text) ERRORS [expired at (at: Instant)],",
    ] {
        let schema = Schema::parse(&format!("PROTOCOL example VERSION 1, {declarations}")).unwrap();
        let generated = anyx_protocol_generate::typescript(&schema).unwrap();
        assert!(generated.contains("function reviveInstant("));
    }
}
