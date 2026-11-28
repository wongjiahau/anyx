//! Spelling anyx names as identifiers: a name's words, without its slots.

/// The words of a name, lowercased, without slots or punctuation.
fn words(name: &str) -> Vec<String> {
    name.split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

pub fn snake_case(name: &str) -> String {
    let snake = words(name).join("_");
    match snake.as_str() {
        "as" | "async" | "await" | "break" | "const" | "continue" | "crate" | "dyn" | "else"
        | "enum" | "extern" | "false" | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop"
        | "match" | "mod" | "move" | "mut" | "pub" | "ref" | "return" | "self" | "static"
        | "struct" | "super" | "trait" | "true" | "type" | "unsafe" | "use" | "where" | "while" => {
            format!("r#{snake}")
        }
        _ => snake,
    }
}

pub fn pascal_case(name: &str) -> String {
    words(name).iter().map(|word| capitalize(word)).collect()
}

/// A data type's name. A family is named by the words before its first
/// slot, without the word that leads into it: `Reply` for `Reply of _ or _`,
/// `Endpoint` for `Endpoint returning _ ...`.
pub fn type_name(name: &str) -> String {
    let Some((head, _)) = name.split_once(" _") else {
        return pascal_case(name);
    };
    let mut words: Vec<&str> = head.split_whitespace().collect();
    if words.len() > 1 {
        words.pop();
    }
    pascal_case(&words.join(" "))
}

pub fn camel_case(name: &str) -> String {
    let words = words(name);
    let mut camel = String::new();
    for (position, word) in words.iter().enumerate() {
        if position == 0 {
            camel.push_str(word);
        } else {
            camel.push_str(&capitalize(word));
        }
    }
    camel
}

fn capitalize(word: &str) -> String {
    let mut characters = word.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

/// A string literal in Rust and TypeScript alike.
pub fn quoted(text: &str) -> String {
    format!("{text:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spells_names_without_slots() {
        assert_eq!(
            snake_case("write file _ at revision _ content _"),
            "write_file_at_revision_content"
        );
        assert_eq!(
            camel_case("write file _ at revision _ content _"),
            "writeFileAtRevisionContent"
        );
        assert_eq!(pascal_case("Sign in error"), "SignInError");
        assert_eq!(snake_case("type"), "r#type");
    }
}
