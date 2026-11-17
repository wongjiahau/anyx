use rustyline::{
    CompletionType, Config, Context, Editor, Helper,
    completion::Completer,
    error::ReadlineError,
    highlight::Highlighter,
    hint::Hinter,
    history::MemHistory,
    validate::{ValidationContext, ValidationResult, Validator},
};

use anyx_shell::Interpreter;

struct ShellHelper {
    interpreter: Interpreter,
}

impl Helper for ShellHelper {}
impl Highlighter for ShellHelper {}
impl Hinter for ShellHelper {
    type Hint = String;
}
impl Completer for ShellHelper {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<String>)> {
        Ok(self.interpreter.complete(line, pos))
    }
}
impl Validator for ShellHelper {
    fn validate(&self, context: &mut ValidationContext<'_>) -> rustyline::Result<ValidationResult> {
        Ok(if incomplete(context.input()) {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Valid(None)
        })
    }
}

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

pub fn run(interpreter: Interpreter) -> Result<bool, String> {
    let config = Config::builder()
        .completion_type(CompletionType::List)
        .build();
    let mut editor = Editor::<ShellHelper, MemHistory>::with_history(config, MemHistory::new())
        .map_err(|error| error.to_string())?;
    editor.set_helper(Some(ShellHelper { interpreter }));
    loop {
        match editor.readline("anyx> ") {
            Ok(source) => {
                if source.trim().is_empty() {
                    continue;
                }
                editor
                    .add_history_entry(source.as_str())
                    .map_err(|error| error.to_string())?;
                // A failed command reports its diagnostic but leaves the session alive.
                super::execute(&mut editor.helper_mut().unwrap().interpreter, &source)?;
            }
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => return Ok(true),
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::incomplete;

    #[test]
    fn continues_only_incomplete_syntax() {
        for source in [
            "environment / read variable (",
            "environment / read variable \"HOME",
            "environment / read variable:",
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
