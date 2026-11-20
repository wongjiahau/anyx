//! `WHY.md`'s examples: each ```` ```anyx schema ```` block is checked, and
//! each ```` ```anyx value <Type> ```` block is decoded as that type, in the
//! schema block that follows it.

use anyx_protocol::{Schema, decode_type, decode_value};

struct Block {
    info: String,
    body: String,
}

struct Example {
    type_name: String,
    value: String,
    schema: String,
}

fn blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut lines = markdown.lines();
    while let Some(line) = lines.next() {
        if let Some(info) = line.strip_prefix("```").filter(|info| !info.is_empty()) {
            let body = lines
                .by_ref()
                .take_while(|line| *line != "```")
                .collect::<Vec<_>>()
                .join("\n");
            blocks.push(Block {
                info: info.to_string(),
                body,
            });
        }
    }
    blocks
}

fn examples(markdown: &str) -> Vec<Example> {
    let blocks = blocks(markdown);
    blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| {
            let type_name = block.info.strip_prefix("anyx value ")?;
            let schema = blocks[index + 1..]
                .iter()
                .find(|block| block.info == "anyx schema")
                .unwrap_or_else(|| panic!("no schema block after the value of `{type_name}`"));
            Some(Example {
                type_name: type_name.to_string(),
                value: block.body.clone(),
                schema: schema.body.clone(),
            })
        })
        .collect()
}

#[test]
fn every_value_in_why_md_decodes_as_its_record() {
    let markdown = include_str!("../WHY.md");
    let examples = examples(markdown);
    let schemas = blocks(markdown)
        .into_iter()
        .filter(|block| block.info == "anyx schema")
        .count();
    assert_eq!(examples.len(), schemas, "each schema block has one value");
    for example in examples {
        let source = format!("PROTOCOL why VERSION 1,\n{}", example.schema);
        let schema = Schema::parse(&source)
            .unwrap_or_else(|error| panic!("the schema of `{}`: {error:#?}", example.type_name));
        let expected = decode_type(&schema, &example.type_name)
            .unwrap_or_else(|error| panic!("`{}`: {error}", example.type_name));
        decode_value(&schema, &expected, &example.value)
            .unwrap_or_else(|error| panic!("the value of `{}`: {error}", example.type_name));
    }
}
