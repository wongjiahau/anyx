mod common;

use anyx::{CustomBracket, ErrorCode, NodeKind, format};
use common::{errors, nodes, parse_round_trip};

#[test]
fn custom_brackets_keep_their_delimiters_and_children() {
    let nodes = nodes("[|> (* a *), {< b >}, [§\t§] <|]");
    let NodeKind::CustomBracket(bracket, children) = &nodes[0].kind else {
        panic!("{nodes:?}");
    };
    assert_eq!(bracket.open(), "[|>");
    assert_eq!(bracket.close(), "<|]");
    assert_eq!(children.len(), 3);
    for (child, (opener, count)) in children.iter().zip([("(*", 1), ("{<", 1), ("[§", 0)]) {
        let NodeKind::CustomBracket(bracket, children) = &child.kind else {
            panic!("{child:?}");
        };
        assert_eq!(bracket.open(), opener);
        assert_eq!(children.len(), count);
    }
}

#[test]
fn whitespace_distinguishes_custom_and_ordinary_brackets() {
    for source in ["(-1)", "(@by name)", "[ <= 3]", "[||]", "[a | ]", "[a|]"] {
        assert!(!matches!(
            nodes(source)[0].kind,
            NodeKind::CustomBracket(..)
        ));
    }
    assert!(matches!(
        nodes("[<= 3 =>]")[0].kind,
        NodeKind::CustomBracket(..)
    ));
    for opener in ["[", "[.", "[:", "[a", "[| ", "|]", "[||]"] {
        assert_eq!(CustomBracket::from_opener(opener), None, "{opener}");
    }
}

#[test]
fn mismatches_use_the_whole_closer_and_recover_at_outer_brackets() {
    let parsed = parse_round_trip("[| a ||]");
    assert_eq!(parsed.errors[0].code, ErrorCode::MismatchedBracket);
    assert!(parsed.errors[0].message.contains("expected `|]`"));
    assert_eq!(parsed.errors[0].span.start, 5);
    assert_eq!(parsed.errors[0].span.end, 8);

    let parsed = parse_round_trip("(* a)");
    assert_eq!(parsed.errors[0].code, ErrorCode::MismatchedBracket);
    assert!(parsed.errors[0].message.contains("expected `*)`"));
    assert_eq!(errors("[| (* a |]"), [(ErrorCode::UnclosedBracket, 3, 5)]);
    assert_eq!(errors(" |]"), [(ErrorCode::UnexpectedCloser, 1, 3)]);
}

#[test]
fn formatting_preserves_whitespace_at_delimiters() {
    for (source, expected) in [
        ("[|\t|]", "[| |]\n"),
        ("[|>\n(* a *), [||] \n<|]", "[|> (* a *), [||] <|]\n"),
        ("[a | ]", "[a | ]\n"),
        ("[ <= 3]", "[ <= 3]\n"),
    ] {
        let formatted = format(source).unwrap();
        assert_eq!(formatted, expected);
        assert_eq!(format(&formatted).unwrap(), formatted);
    }

    let source = format!("[| ## opening\n {},\n ## closing\n |]", "a".repeat(110));
    let formatted = format(&source).unwrap();
    assert!(formatted.starts_with("[| ## opening\n"), "{formatted}");
    assert!(formatted.ends_with("## closing\n|]\n"), "{formatted}");
    assert_eq!(format(&formatted).unwrap(), formatted);
}
