//! Editing and snippet state, independent of terminal rendering.
use std::ops::Range;

use anyx_shell::{ArgumentInfo, Completions, Interpreter, TabStop};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Default)]
pub struct Editor {
    pub text: String,
    pub cursor: usize,
    pub selection: Option<Range<usize>>,
    pub menu: Option<Menu>,
    pub snippet: Option<Snippet>,
    pub argument_info: Vec<ArgumentInfo>,
    history_index: Option<usize>,
    draft: String,
}

pub struct Menu {
    pub completions: Completions,
    pub selected: usize,
}

pub struct Snippet {
    pub stops: Vec<TabStop>,
    pub active: usize,
    end: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Edit,
    Submit,
    Cancel,
    Exit,
}

impl Editor {
    pub fn key(&mut self, key: KeyEvent, interpreter: &Interpreter, history: &[String]) -> Action {
        use KeyCode::*;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl {
            match key.code {
                Char('c') => return Action::Cancel,
                Char('d') if self.text.is_empty() => return Action::Exit,
                Char(' ') | Null => self.refresh(interpreter, true),
                Char('a') => self.move_to(self.line_start()),
                Char('e') => self.move_to(self.line_end()),
                Char('u') => self.replace(self.line_start()..self.cursor, ""),
                Char('k') => self.replace(self.cursor..self.line_end(), ""),
                Char('w') => {
                    let before = &self.text[..self.cursor];
                    let start = before
                        .trim_end()
                        .char_indices()
                        .rev()
                        .find(|(_, c)| c.is_whitespace())
                        .map_or(0, |(i, c)| i + c.len_utf8());
                    self.replace(start..self.cursor, "");
                }
                Char('d') => self.delete(false),
                _ => {}
            }
            return Action::Edit;
        }
        match key.code {
            Enter if key.modifiers.contains(KeyModifiers::ALT) => self.insert("\n"),
            Enter if self.menu.is_some() => self.accept(interpreter),
            Enter if super::incomplete(&self.text) => self.insert("\n"),
            Enter => return Action::Submit,
            Tab if self.menu.is_some() => self.accept(interpreter),
            Tab if self.snippet.is_some() => self.tab(false),
            Tab => {
                self.refresh(interpreter, true);
                if self
                    .menu
                    .as_ref()
                    .is_some_and(|menu| menu.completions.items.len() == 1)
                {
                    self.accept(interpreter);
                }
            }
            BackTab => self.tab(true),
            Esc => {
                if self.menu.take().is_none() {
                    self.snippet = None;
                    self.selection = None;
                }
            }
            Up | Down if self.menu.is_some() => {
                let menu = self.menu.as_mut().unwrap();
                let count = menu.completions.items.len();
                menu.selected = if key.code == Up {
                    (menu.selected + count - 1) % count
                } else {
                    (menu.selected + 1) % count
                };
            }
            Up | Down => {
                if !self.vertical(key.code == Up) {
                    self.history(history, key.code == Up);
                }
            }
            Left => self.move_to(
                self.selection
                    .as_ref()
                    .map_or_else(|| self.previous(), |range| range.start),
            ),
            Right => self.move_to(
                self.selection
                    .as_ref()
                    .map_or_else(|| self.next(), |range| range.end),
            ),
            Home => self.move_to(self.line_start()),
            End => self.move_to(self.line_end()),
            Backspace | Delete => {
                self.delete(key.code == Backspace);
                self.refresh(interpreter, false);
            }
            Char(character) if !character.is_control() => {
                self.insert(&character.to_string());
                self.refresh(interpreter, false);
            }
            _ => {}
        }
        Action::Edit
    }

    pub fn paste(&mut self, text: &str) {
        let text: String = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\t', "    ")
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .collect();
        self.insert(&text);
        self.menu = None;
    }

    pub fn refresh(&mut self, interpreter: &Interpreter, force: bool) {
        self.menu = None;
        if !force && (self.snippet.is_some() || self.text.trim().is_empty()) {
            return;
        }
        let completions = interpreter.completions(&self.text, self.cursor);
        if !completions.items.is_empty() {
            self.menu = Some(Menu {
                completions,
                selected: 0,
            });
        }
    }

    fn accept(&mut self, interpreter: &Interpreter) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        let item = &menu.completions.items[menu.selected];
        let range = menu.completions.replace;
        let start = range.start;
        self.replace(range, &item.insert_text);
        if !item.tab_stops.is_empty() {
            let mut end = start + item.insert_text.len();
            let mut stops = self.snippet.take().map_or(Vec::new(), |snippet| {
                end = end.max(snippet.end);
                snippet
                    .stops
                    .into_iter()
                    .filter(|stop| {
                        !(stop.range.start <= start
                            && start + item.insert_text.len() <= stop.range.end)
                    })
                    .collect()
            });
            let first = start + item.tab_stops[0].range.start;
            stops.extend(item.tab_stops.iter().map(|stop| TabStop {
                range: start + stop.range.start..start + stop.range.end,
                label: stop.label.clone(),
            }));
            stops.sort_by_key(|stop| stop.range.start);
            let active = stops
                .iter()
                .position(|stop| stop.range.start == first)
                .unwrap();
            end = end.max(stops.last().unwrap().range.end);
            self.snippet = Some(Snippet { stops, active, end });
            self.select_stop();
        } else {
            self.refresh(interpreter, false);
        }
    }

    fn tab(&mut self, backwards: bool) {
        let Some(snippet) = self.snippet.as_mut() else {
            return;
        };
        if backwards {
            snippet.active = snippet.active.saturating_sub(1);
        } else if snippet.active + 1 < snippet.stops.len() {
            snippet.active += 1;
        } else {
            let end = snippet.end;
            self.snippet = None;
            self.selection = None;
            self.cursor = end;
            return;
        }
        self.select_stop();
    }

    fn select_stop(&mut self) {
        let snippet = self.snippet.as_ref().unwrap();
        let range = snippet.stops[snippet.active].range.clone();
        self.cursor = range.end;
        self.selection = Some(range);
        self.menu = None;
    }

    fn insert(&mut self, text: &str) {
        let range = self.selection.clone().unwrap_or(self.cursor..self.cursor);
        self.replace(range, text);
    }

    fn replace(&mut self, range: Range<usize>, value: &str) {
        let delta = value.len() as isize - range.len() as isize;
        let shift = |index: usize| index.saturating_add_signed(delta);
        if let Some(snippet) = &mut self.snippet {
            let edited = snippet
                .stops
                .iter()
                .position(|stop| stop.range.start <= range.start && range.end <= stop.range.end);
            if let Some(edited) = edited {
                snippet.active = edited;
                for (index, stop) in snippet.stops.iter_mut().enumerate() {
                    if index == edited {
                        stop.range.end = shift(stop.range.end);
                    } else if stop.range.start >= range.end {
                        stop.range = shift(stop.range.start)..shift(stop.range.end);
                    }
                }
                snippet.end = shift(snippet.end);
            } else {
                // Moving outside a template ends its navigation session.
                self.snippet = None;
            }
        }
        self.text.replace_range(range.clone(), value);
        self.cursor = range.start + value.len();
        self.selection = None;
        self.menu = None;
    }

    fn move_to(&mut self, cursor: usize) {
        self.cursor = cursor;
        self.selection = None;
        self.menu = None;
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next(&self) -> usize {
        self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map_or(self.cursor, |g| self.cursor + g.len())
    }

    fn delete(&mut self, backwards: bool) {
        let range = self.selection.clone().unwrap_or_else(|| {
            if backwards {
                self.previous()..self.cursor
            } else {
                self.cursor..self.next()
            }
        });
        self.replace(range, "");
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor]
            .rfind('\n')
            .map_or(0, |index| index + 1)
    }
    fn line_end(&self) -> usize {
        self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |index| self.cursor + index)
    }

    fn vertical(&mut self, up: bool) -> bool {
        use unicode_width::UnicodeWidthStr;
        let start = self.line_start();
        let end = self.line_end();
        let target = if up && start > 0 {
            let previous = self.text[..start - 1]
                .rfind('\n')
                .map_or(0, |index| index + 1);
            previous..start - 1
        } else if !up && end < self.text.len() {
            let next = end + 1;
            next..self.text[next..]
                .find('\n')
                .map_or(self.text.len(), |index| next + index)
        } else {
            return false;
        };
        let column = self.text[start..self.cursor].width();
        let mut width = 0;
        let mut cursor = target.start;
        for grapheme in self.text[target].graphemes(true) {
            if width + grapheme.width() > column {
                break;
            }
            width += grapheme.width();
            cursor += grapheme.len();
        }
        self.move_to(cursor);
        true
    }

    fn history(&mut self, history: &[String], previous: bool) {
        if history.is_empty() {
            return;
        }
        let index = match (self.history_index, previous) {
            (None, true) => {
                self.draft = self.text.clone();
                history.len() - 1
            }
            (Some(index), true) => index.saturating_sub(1),
            (Some(index), false) => index + 1,
            (None, false) => return,
        };
        self.text = history.get(index).unwrap_or(&self.draft).clone();
        self.history_index = (index < history.len()).then_some(index);
        self.cursor = self.text.len();
        self.selection = None;
        self.snippet = None;
        self.menu = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyx_shell::environment::Environment;

    fn shell() -> Interpreter {
        let mut shell = Interpreter::new();
        shell.register("environment", Environment::new()).unwrap();
        shell
    }
    fn press(editor: &mut Editor, shell: &Interpreter, code: KeyCode) -> Action {
        editor.key(KeyEvent::new(code, KeyModifiers::NONE), shell, &[])
    }
    fn type_text(editor: &mut Editor, shell: &Interpreter, text: &str) {
        for character in text.chars() {
            press(editor, shell, KeyCode::Char(character));
        }
    }
    fn selection(editor: &Editor) -> &str {
        &editor.text[editor.selection.clone().unwrap()]
    }
    fn set_template(editor: &mut Editor, shell: &Interpreter) {
        type_text(editor, shell, "env");
        press(editor, shell, KeyCode::Tab);
        type_text(editor, shell, "set");
        assert_eq!(press(editor, shell, KeyCode::Enter), Action::Edit);
        assert_eq!(
            editor.text,
            "environment / set variable \"name\" to \"value\""
        );
    }

    #[test]
    fn menu_acceptance_inserts_a_template_without_executing_it() {
        let shell = shell();
        let mut editor = Editor::default();
        type_text(&mut editor, &shell, "environment / re");
        assert_eq!(editor.menu.as_ref().unwrap().completions.items.len(), 2);
        press(&mut editor, &shell, KeyCode::Down);
        press(&mut editor, &shell, KeyCode::Enter);
        assert_eq!(editor.text, "environment / remove variable \"name\"");
        assert_eq!(selection(&editor), "name");
        assert!(editor.menu.is_none());
    }

    #[test]
    fn replaces_selected_slots_and_navigates_both_directions_after_unicode_edits() {
        let shell = shell();
        let mut editor = Editor::default();
        set_template(&mut editor, &shell);
        assert_eq!(selection(&editor), "name");
        type_text(&mut editor, &shell, "日本語");
        press(&mut editor, &shell, KeyCode::Tab);
        assert_eq!(selection(&editor), "value");
        type_text(&mut editor, &shell, "hello 🙂");
        press(&mut editor, &shell, KeyCode::BackTab);
        assert_eq!(selection(&editor), "日本語");
        type_text(&mut editor, &shell, "KEY");
        press(&mut editor, &shell, KeyCode::Tab);
        assert_eq!(selection(&editor), "hello 🙂");
        press(&mut editor, &shell, KeyCode::Tab);
        assert!(editor.snippet.is_none());
        assert_eq!(editor.cursor, editor.text.len());
        assert_eq!(press(&mut editor, &shell, KeyCode::Enter), Action::Submit);
        let result = self::shell().execute(&editor.text).unwrap();
        assert_eq!(result.output, "{ name \"KEY\", value \"hello 🙂\" }\n");
    }

    #[test]
    fn deleting_a_selection_keeps_its_empty_stop_editable() {
        let shell = shell();
        let mut editor = Editor::default();
        set_template(&mut editor, &shell);
        press(&mut editor, &shell, KeyCode::Backspace);
        assert!(editor.text.contains("\"\" to"));
        type_text(&mut editor, &shell, "LONGER_NAME");
        press(&mut editor, &shell, KeyCode::Tab);
        assert_eq!(selection(&editor), "value");
    }

    #[test]
    fn escape_dismisses_the_menu_then_leaves_plain_editing_and_history() {
        let shell = shell();
        let mut editor = Editor::default();
        type_text(&mut editor, &shell, "env");
        press(&mut editor, &shell, KeyCode::Esc);
        assert!(editor.menu.is_none());
        let history = vec!["environment / list variables".into()];
        editor.key(
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            &shell,
            &history,
        );
        assert_eq!(editor.text, history[0]);
        editor.key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &shell,
            &history,
        );
        assert_eq!(editor.text, "env");
        editor.key(
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL),
            &shell,
            &history,
        );
        assert!(editor.menu.is_some());
        assert_eq!(press(&mut editor, &shell, KeyCode::Enter), Action::Edit);
    }

    #[test]
    fn paste_replaces_the_active_slot_without_submitting_and_filters_terminal_controls() {
        let shell = shell();
        let mut editor = Editor::default();
        set_template(&mut editor, &shell);
        editor.paste("KEY\u{1b}\u{7}");
        assert_eq!(
            editor.text,
            "environment / set variable \"KEY\" to \"value\""
        );
        press(&mut editor, &shell, KeyCode::Tab);
        assert_eq!(selection(&editor), "value");
        editor.paste("first\r\nsecond");
        assert!(editor.text.contains("first\nsecond"));
        assert!(editor.menu.is_none());
    }

    #[test]
    fn cancellation_and_end_of_input_are_distinct() {
        let shell = shell();
        let mut editor = Editor::default();
        assert_eq!(
            editor.key(
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
                &shell,
                &[]
            ),
            Action::Exit
        );
        type_text(&mut editor, &shell, "env");
        assert_eq!(
            editor.key(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                &shell,
                &[]
            ),
            Action::Cancel
        );
    }
    #[test]
    fn edits_unicode_whitespace_and_navigates_multiline_input() {
        let shell = shell();
        let mut editor = Editor::default();
        editor.paste("日本\u{3000}word");
        editor.key(
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
            &shell,
            &[],
        );
        assert_eq!(editor.text, "日本\u{3000}");
        editor.paste("\n🙂hello");
        press(&mut editor, &shell, KeyCode::Up);
        assert_eq!(editor.cursor, "日本\u{3000}".len());
        press(&mut editor, &shell, KeyCode::Down);
        assert_eq!(&editor.text[..editor.cursor], "日本\u{3000}\n🙂hell");
    }
    #[test]
    fn inner_snippet_slots_precede_remaining_outer_arguments() {
        use anyx_protocol::{Call, Outcome, Schema};
        use anyx_shell::Tool;
        struct Math(Schema);
        impl Tool for Math {
            fn schema(&self) -> &Schema {
                &self.0
            }
            fn invoke(&mut self, _: &Call) -> Outcome {
                panic!("editing cannot invoke a tool")
            }
        }
        let mut shell = Interpreter::new();
        shell.register("math", Math(Schema::parse("PROTOCOL math VERSION 1, METHOD plus (a ⁽Integer) with (b ⁽Integer) RETURNS (value ⁽Integer) ERRORS [| |], METHOD number (seed ⁽Integer) RETURNS (value ⁽Integer) ERRORS [| |]").unwrap())).unwrap();
        let mut editor = Editor::default();
        type_text(&mut editor, &shell, "math / pl");
        press(&mut editor, &shell, KeyCode::Enter);
        assert_eq!(selection(&editor), "0");
        type_text(&mut editor, &shell, "(math / nu)");
        press(&mut editor, &shell, KeyCode::Left);
        editor.key(
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL),
            &shell,
            &[],
        );
        press(&mut editor, &shell, KeyCode::Enter);
        assert_eq!(editor.snippet.as_ref().unwrap().stops.len(), 2);
        type_text(&mut editor, &shell, "7");
        press(&mut editor, &shell, KeyCode::Tab);
        assert_eq!(selection(&editor), "0");
        assert!(
            editor.snippet.as_ref().unwrap().stops[1]
                .label
                .starts_with("b:")
        );
        type_text(&mut editor, &shell, "9");
        press(&mut editor, &shell, KeyCode::Tab);
        assert_eq!(editor.text, "math / plus (math / number 7) with 9");
        assert_eq!(editor.cursor, editor.text.len());
    }
}
