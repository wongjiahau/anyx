//! Ratatui widgets in an inline viewport; command output stays in scrollback.
use std::io::{self, Write};

use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{self, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal as TuiTerminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::editor::Editor;

struct RawMode;
impl RawMode {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let guard = Self;
        execute!(io::stdout(), EnableBracketedPaste)?;
        Ok(guard)
    }
}
impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableBracketedPaste, crossterm::cursor::Show);
        let _ = disable_raw_mode();
    }
}

pub struct Terminal {
    tui: TuiTerminal<CrosstermBackend<io::Stdout>>,
    _raw: RawMode,
}

impl Terminal {
    pub fn new() -> io::Result<Self> {
        let raw = RawMode::new()?;
        let backend = CrosstermBackend::new(io::stdout());
        let height = terminal::size()?.1.min(12);
        let tui = TuiTerminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(height),
            },
        )?;
        Ok(Self { tui, _raw: raw })
    }

    pub fn draw(&mut self, editor: &Editor) -> io::Result<()> {
        self.tui.draw(|frame| render(frame, editor))?;
        Ok(())
    }

    pub fn finish(&mut self, editor: &Editor, cancelled: bool) -> io::Result<()> {
        let area = self.tui.get_frame().area();
        self.tui.clear()?;
        self.tui.set_cursor_position((area.x, area.y))?;
        self.tui.show_cursor()?;
        // Commit the submitted input to the normal terminal, releasing the
        // viewport before the interpreter prints its results.
        write!(
            self.tui.backend_mut(),
            "anyx> {}{}\r\n",
            editor.text.replace('\n', "\r\n ...> "),
            if cancelled { "^C" } else { "" }
        )?;
        self.tui.backend_mut().flush()
    }
}

fn clean(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control() || *character == '\n')
        .collect()
}

fn input(editor: &Editor) -> Text<'_> {
    let mut offset = 0;
    let lines = editor
        .text
        .split('\n')
        .enumerate()
        .map(|(index, line)| {
            let mut spans = vec![Span::styled(
                if index == 0 { "anyx> " } else { " ...> " },
                Style::default().fg(Color::Cyan),
            )];
            if let Some(selection) = &editor.selection {
                let start = selection.start.saturating_sub(offset).min(line.len());
                let end = selection.end.saturating_sub(offset).min(line.len());
                spans.extend([
                    Span::raw(&line[..start]),
                    Span::styled(
                        &line[start..end],
                        Style::default().add_modifier(Modifier::REVERSED),
                    ),
                    Span::raw(&line[end..]),
                ]);
            } else {
                spans.push(Span::raw(line));
            }
            offset += line.len() + 1;
            Line::from(spans)
        })
        .collect::<Vec<_>>();
    Text::from(lines)
}

fn render(frame: &mut Frame<'_>, editor: &Editor) {
    let area = frame.area();
    if area.is_empty() {
        return;
    }
    let line_count = editor.text.lines().count() + usize::from(editor.text.ends_with('\n'));
    let input_height = (line_count as u16)
        .clamp(1, 4)
        .min(area.height.saturating_sub(2).max(1));
    let [input_area, menu_area, footer_area] = Layout::vertical([
        Constraint::Length(input_height),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    let before = &editor.text[..editor.cursor];
    let row = before.bytes().filter(|byte| *byte == b'\n').count();
    let column = 6 + before.rsplit('\n').next().unwrap_or("").width();
    let scroll_y = row.saturating_sub(input_area.height.saturating_sub(1) as usize);
    let scroll_x = column.saturating_sub(input_area.width.saturating_sub(1) as usize);
    frame.render_widget(
        Paragraph::new(input(editor)).scroll((scroll_y as u16, scroll_x as u16)),
        input_area,
    );
    frame.set_cursor_position((
        input_area
            .x
            .saturating_add(column.saturating_sub(scroll_x) as u16)
            .min(area.right().saturating_sub(1)),
        input_area
            .y
            .saturating_add(row.saturating_sub(scroll_y) as u16)
            .min(area.bottom().saturating_sub(1)),
    ));

    let footer = if let Some(menu) = &editor.menu {
        render_menu(frame, menu_area, menu);
        "↑↓ choose · Enter/Tab insert · Esc close".into()
    } else if let Some(snippet) = &editor.snippet {
        format!(
            "{}/{} {} · Tab next · Shift-Tab previous · Esc finish",
            snippet.active + 1,
            snippet.stops.len(),
            snippet.stops[snippet.active].label
        )
    } else {
        "Ctrl-Space completions · Ctrl-C cancel · Ctrl-D exit".into()
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::Cyan)),
        footer_area,
    );
}

fn render_menu(frame: &mut Frame<'_>, area: Rect, menu: &super::editor::Menu) {
    let (list_area, details_area) = if area.width >= 80 {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(area);
        (left, right)
    } else if area.height >= 6 {
        let [top, bottom] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(area);
        (top, bottom)
    } else {
        (area, Rect::default())
    };
    let items = menu
        .completions
        .items
        .iter()
        .map(|item| ListItem::new(clean(&item.label)))
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(Block::bordered().title(" Completions "))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");
    let mut state = ListState::default().with_selected(Some(menu.selected));
    frame.render_stateful_widget(list, list_area, &mut state);
    if !details_area.is_empty() {
        let item = &menu.completions.items[menu.selected];
        let details = format!(
            "{}\n\n{}",
            clean(&item.detail),
            clean(item.documentation.as_deref().unwrap_or(""))
        );
        frame.render_widget(
            Paragraph::new(details)
                .block(Block::bordered().title(" Details "))
                .wrap(Wrap { trim: false }),
            details_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyx_shell::{Interpreter, environment::Environment};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::{Backend, TestBackend};

    fn screen(editor: &Editor, width: u16, height: u16) -> TuiTerminal<TestBackend> {
        let mut terminal = TuiTerminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render(frame, editor)).unwrap();
        terminal
    }

    fn content(terminal: &TuiTerminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn popup_displays_signatures_documentation_and_selection() {
        let mut shell = Interpreter::new();
        shell.register("environment", Environment::new()).unwrap();
        let mut editor = Editor::default();
        editor.paste("environment / set");
        editor.refresh(&shell, true);
        let terminal = screen(&editor, 120, 12);
        let text = content(&terminal);
        assert!(text.contains("Completions"));
        assert!(text.contains("set variable (name: Text) to (value: Text)"));
        assert!(text.contains("returns Variable"));
        assert!(text.contains("ERRORS ["));
        assert!(text.contains("invalid variable value"));
        assert!(text.contains("invalid variable name"));
        assert!(text.contains("Values may be empty"));
        editor.key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &shell,
            &[],
        );
        let terminal = screen(&editor, 120, 12);
        assert!(content(&terminal).contains("1/2 name: Text"));
        let selected = editor.selection.unwrap();
        for x in selected {
            assert!(
                terminal.backend().buffer()[(x as u16 + 6, 0)]
                    .modifier
                    .contains(Modifier::REVERSED)
            );
        }
    }

    #[test]
    fn small_and_resized_viewports_keep_unicode_input_and_cursor_in_bounds() {
        let mut editor = Editor::default();
        editor.paste("日本e\u{301}\n🙂hello");
        let mut terminal = screen(&editor, 80, 12);
        for (width, height) in [(7, 4), (1, 2), (120, 20)] {
            terminal.backend_mut().resize(width, height);
            terminal.resize(Rect::new(0, 0, width, height)).unwrap();
            terminal.draw(|frame| render(frame, &editor)).unwrap();
            let cursor = terminal.backend_mut().get_cursor_position().unwrap();
            assert!(cursor.x < width && cursor.y < height);
        }
    }
}
