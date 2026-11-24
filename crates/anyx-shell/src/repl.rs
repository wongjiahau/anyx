#[path = "repl/editor.rs"]
mod editor;
#[path = "repl/terminal.rs"]
mod terminal;

use anyx_shell::Interpreter;
use crossterm::event::{self, Event, KeyEventKind};
use editor::{Action, Editor};
use std::io::{self, BufRead, IsTerminal};
use terminal::Terminal;

fn incomplete(source: &str) -> bool {
    use anyx::ErrorCode;
    let errors = anyx::parse(source).errors;
    !errors.is_empty()
        && errors.iter().all(|error| {
            matches!(
                error.code,
                ErrorCode::UnclosedBracket | ErrorCode::UnterminatedString
            ) || (error.code == ErrorCode::EmptyOperand && error.span.end == source.len())
        })
}

pub fn run(mut interpreter: Interpreter) -> Result<bool, String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return read_lines(&mut interpreter);
    }
    let mut history = Vec::new();
    loop {
        let mut editor = Editor::default();
        let mut terminal = Terminal::new().map_err(|error| error.to_string())?;
        let action = loop {
            editor.argument_info = interpreter.argument_info(&editor.text, editor.cursor);
            terminal.draw(&editor).map_err(|error| error.to_string())?;
            let action = match event::read().map_err(|error| error.to_string())? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    editor.key(key, &interpreter, &history)
                }
                Event::Paste(text) => {
                    editor.paste(&text);
                    Action::Edit
                }
                _ => Action::Edit,
            };
            if action != Action::Edit {
                break action;
            }
        };
        terminal
            .finish(&editor, action == Action::Cancel)
            .map_err(|error| error.to_string())?;
        drop(terminal);
        match action {
            Action::Exit => return Ok(true),
            Action::Submit if !editor.text.trim().is_empty() => {
                if history.last() != Some(&editor.text) {
                    if history.len() == 1000 {
                        history.remove(0);
                    }
                    history.push(editor.text.clone());
                }
                super::execute(&mut interpreter, &editor.text)?;
            }
            _ => {}
        }
    }
}

fn read_lines(interpreter: &mut Interpreter) -> Result<bool, String> {
    let mut source = String::new();
    for line in io::stdin().lock().lines() {
        source.push_str(&line.map_err(|error| error.to_string())?);
        source.push('\n');
        if !incomplete(&source) {
            super::execute(interpreter, &source)?;
            source.clear();
        }
    }
    if !source.trim().is_empty() {
        super::execute(interpreter, &source)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::incomplete;

    #[test]
    fn continues_only_incomplete_syntax() {
        for source in [
            "environment / read variable (",
            "environment / read variable \"HOME",
            "environment / read variable ⁽",
        ] {
            assert!(incomplete(source), "{source}");
        }
        for source in [
            "",
            "environment / list variables",
            "environment / unknown",
            "environment / read variable )",
            "environment / read variable (]",
            "environment / read variable \"bad\\q\"",
        ] {
            assert!(!incomplete(source), "{source}");
        }
    }
}
