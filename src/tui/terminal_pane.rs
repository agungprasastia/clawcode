use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::chat::strip_ansi_codes;
use super::theme::Theme;

pub const MAX_TERMINAL_LINES: usize = 500;
pub const MAX_TERMINAL_BYTES: usize = 64 * 1024;
pub const TERMINAL_PANE_HEIGHT: u16 = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalLineKind {
    Command,
    Output,
    Error,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalLine {
    pub kind: TerminalLineKind,
    pub text: String,
}

pub enum TerminalEvent {
    Output(String),
    Error(String),
    Exited(Option<i32>),
}

pub struct TerminalPaneState {
    visible: bool,
    focused: bool,
    cwd: PathBuf,
    input: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    draft_input: String,
    lines: VecDeque<TerminalLine>,
    total_bytes: usize,
    scroll_offset: u16,
    running_command: Option<String>,
    last_exit_code: Option<i32>,
    active_child: Arc<Mutex<Option<Child>>>,
    event_rx: Option<mpsc::Receiver<TerminalEvent>>,
}

impl Default for TerminalPaneState {
    fn default() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::new(cwd)
    }
}

impl TerminalPaneState {
    pub fn new(cwd: PathBuf) -> Self {
        let mut state = Self {
            visible: false,
            focused: false,
            cwd,
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            draft_input: String::new(),
            lines: VecDeque::new(),
            total_bytes: 0,
            scroll_offset: 0,
            running_command: None,
            last_exit_code: None,
            active_child: Arc::new(Mutex::new(None)),
            event_rx: None,
        };
        state.push_line(
            TerminalLineKind::System,
            "Clawcode embedded terminal ready (type commands, `clear`, `cd <dir>`, or `exit`).",
        );
        state
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn is_focused(&self) -> bool {
        self.visible && self.focused
    }

    pub fn is_running(&self) -> bool {
        self.running_command.is_some()
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn lines(&self) -> &VecDeque<TerminalLine> {
        &self.lines
    }

    pub fn last_exit_code(&self) -> Option<i32> {
        self.last_exit_code
    }

    pub fn running_command(&self) -> Option<&str> {
        self.running_command.as_deref()
    }

    pub fn scroll_offset(&self) -> u16 {
        self.scroll_offset
    }

    pub fn open_and_focus(&mut self) {
        self.visible = true;
        self.focused = true;
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.focused = false;
    }

    pub fn toggle(&mut self) -> bool {
        if self.visible {
            self.close();
            false
        } else {
            self.open_and_focus();
            true
        }
    }

    pub fn set_focused(&mut self, focused: bool) {
        if self.visible {
            self.focused = focused;
        } else if focused {
            self.visible = true;
            self.focused = true;
        }
    }

    pub fn clear_scrollback(&mut self) {
        self.lines.clear();
        self.total_bytes = 0;
        self.scroll_offset = 0;
    }

    pub fn scroll_up(&mut self, delta: u16) {
        let max_scroll = self.lines.len().saturating_sub(1) as u16;
        self.scroll_offset = self.scroll_offset.saturating_add(delta).min(max_scroll);
    }

    pub fn scroll_down(&mut self, delta: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(delta);
    }

    pub fn push_line(&mut self, kind: TerminalLineKind, raw: impl Into<String>) {
        let cleaned = strip_ansi_codes(&raw.into());
        let split_lines: Vec<&str> = if cleaned.is_empty() {
            vec![""]
        } else {
            cleaned.lines().collect()
        };
        for line_str in split_lines {
            let mut bounded = line_str.replace('\r', "");
            if bounded.len() > 4096 {
                let mut end = 4096;
                while end > 0 && !bounded.is_char_boundary(end) {
                    end -= 1;
                }
                bounded.truncate(end);
            }
            self.total_bytes = self.total_bytes.saturating_add(bounded.len());
            self.lines.push_back(TerminalLine {
                kind,
                text: bounded,
            });
            while self.lines.len() > MAX_TERMINAL_LINES || self.total_bytes > MAX_TERMINAL_BYTES {
                if let Some(removed) = self.lines.pop_front() {
                    self.total_bytes = self.total_bytes.saturating_sub(removed.text.len());
                } else {
                    self.total_bytes = 0;
                    break;
                }
            }
        }
    }

    pub fn insert_char(&mut self, character: char) {
        let byte_idx = self
            .input
            .char_indices()
            .nth(self.cursor)
            .map(|(idx, _)| idx)
            .unwrap_or(self.input.len());
        self.input.insert(byte_idx, character);
        self.cursor += 1;
        self.history_index = None;
    }

    pub fn insert_str(&mut self, text: &str) {
        for ch in text.chars().filter(|c| *c != '\r' && *c != '\n') {
            self.insert_char(ch);
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.cursor - 1;
            if let Some((byte_idx, _)) = self.input.char_indices().nth(prev) {
                self.input.remove(byte_idx);
                self.cursor = prev;
            }
        }
        self.history_index = None;
    }

    pub fn delete(&mut self) {
        if self.cursor < self.input.chars().count()
            && let Some((byte_idx, _)) = self.input.char_indices().nth(self.cursor)
        {
            self.input.remove(byte_idx);
        }
        self.history_index = None;
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.history_index = None;
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.input.chars().count());
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.input.chars().count();
    }

    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        match self.history_index {
            None => {
                self.draft_input = self.input.clone();
                let idx = self.history.len() - 1;
                self.history_index = Some(idx);
                self.input = self.history[idx].clone();
                self.cursor = self.input.chars().count();
            }
            Some(idx) => {
                let next_idx = idx.saturating_sub(1);
                self.history_index = Some(next_idx);
                if let Some(entry) = self.history.get(next_idx) {
                    self.input = entry.clone();
                    self.cursor = self.input.chars().count();
                }
            }
        }
    }

    pub fn history_down(&mut self) {
        let Some(idx) = self.history_index else {
            return;
        };
        if idx + 1 < self.history.len() {
            let next_idx = idx + 1;
            self.history_index = Some(next_idx);
            self.input = self.history[next_idx].clone();
            self.cursor = self.input.chars().count();
        } else {
            self.history_index = None;
            self.input = std::mem::take(&mut self.draft_input);
            self.cursor = self.input.chars().count();
        }
    }

    pub fn interrupt_running(&mut self) -> bool {
        let mut child_guard = self.active_child.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(mut child) = child_guard.take() {
            let _ = child.kill();
            let _ = child.wait();
            drop(child_guard);
            self.running_command = None;
            self.event_rx = None;
            self.last_exit_code = Some(130);
            self.push_line(TerminalLineKind::System, "^C [command interrupted]");
            true
        } else {
            false
        }
    }

    pub fn submit_input(&mut self) -> Option<String> {
        let cmd = std::mem::take(&mut self.input);
        self.cursor = 0;
        self.history_index = None;
        self.draft_input.clear();
        let trimmed = cmd.trim();
        if trimmed.is_empty() {
            return None;
        }
        if self.history.last().map(String::as_str) != Some(trimmed) {
            self.history.push(trimmed.to_string());
            if self.history.len() > 100 {
                self.history.remove(0);
            }
        }
        Some(self.run_command(trimmed))
    }

    pub fn run_command(&mut self, raw_cmd: &str) -> String {
        let cmd = raw_cmd.trim();
        if cmd.is_empty() {
            return "empty command".to_string();
        }
        self.scroll_offset = 0;
        self.push_line(TerminalLineKind::Command, format!("$ {cmd}"));

        if cmd == "clear" || cmd == "cls" {
            self.clear_scrollback();
            self.last_exit_code = Some(0);
            return "terminal cleared".to_string();
        }

        if cmd == "exit" {
            self.last_exit_code = Some(0);
            self.close();
            return "Terminal pane closed".to_string();
        }

        if cmd == "pwd" {
            let path_display = self.cwd.display().to_string();
            self.push_line(TerminalLineKind::Output, path_display.clone());
            self.last_exit_code = Some(0);
            return format!("cwd: {path_display}");
        }

        if cmd == "cd" || cmd.starts_with("cd ") {
            let arg = cmd.strip_prefix("cd").unwrap_or("").trim();
            let target = if arg.is_empty() || arg == "~" {
                std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| self.cwd.clone())
            } else {
                let candidate = PathBuf::from(arg);
                if candidate.is_absolute() {
                    candidate
                } else {
                    self.cwd.join(candidate)
                }
            };
            match std::fs::canonicalize(&target) {
                Ok(canon) if canon.is_dir() => {
                    let cleaned = strip_windows_verbatim(&canon);
                    self.cwd = cleaned.clone();
                    self.last_exit_code = Some(0);
                    let display = cleaned.display().to_string();
                    self.push_line(TerminalLineKind::System, format!("cwd -> {display}"));
                    return format!("directory changed to {display}");
                }
                _ => {
                    let msg = format!("cd: no such directory: {}", target.display());
                    self.push_line(TerminalLineKind::Error, msg.clone());
                    self.last_exit_code = Some(1);
                    return msg;
                }
            }
        }

        if self.is_running() {
            self.interrupt_running();
        }

        let mut command = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", cmd]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", cmd]);
            c
        };
        command
            .current_dir(&self.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        match command.spawn() {
            Ok(mut child) => {
                let stdout = child.stdout.take();
                let stderr = child.stderr.take();
                let child_slot = Arc::clone(&self.active_child);
                *child_slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(child);

                let (tx, rx) = mpsc::channel();
                self.event_rx = Some(rx);
                self.running_command = Some(cmd.to_string());
                self.last_exit_code = None;

                let out_tx = tx.clone();
                let out_handle = stdout.map(|pipe| {
                    thread::spawn(move || {
                        let reader = BufReader::new(pipe);
                        for line in reader.lines().map_while(Result::ok) {
                            let _ = out_tx.send(TerminalEvent::Output(line));
                        }
                    })
                });

                let err_tx = tx.clone();
                let err_handle = stderr.map(|pipe| {
                    thread::spawn(move || {
                        let reader = BufReader::new(pipe);
                        for line in reader.lines().map_while(Result::ok) {
                            let _ = err_tx.send(TerminalEvent::Error(line));
                        }
                    })
                });

                thread::spawn(move || {
                    if let Some(h) = out_handle {
                        let _ = h.join();
                    }
                    if let Some(h) = err_handle {
                        let _ = h.join();
                    }
                    let code = {
                        let mut guard = child_slot.lock().unwrap_or_else(|p| p.into_inner());
                        guard
                            .as_mut()
                            .and_then(|c| c.wait().ok())
                            .and_then(|status| status.code())
                    };
                    *child_slot.lock().unwrap_or_else(|p| p.into_inner()) = None;
                    let _ = tx.send(TerminalEvent::Exited(code));
                });

                format!("Terminal running: {cmd}")
            }
            Err(error) => {
                let msg = format!("failed to spawn command: {error}");
                self.push_line(TerminalLineKind::Error, msg.clone());
                self.last_exit_code = Some(127);
                msg
            }
        }
    }

    pub fn poll(&mut self) -> bool {
        let Some(rx) = self.event_rx.as_ref() else {
            return false;
        };
        let mut pending = Vec::new();
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok(event) => pending.push(event),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if pending.is_empty() && !disconnected {
            return false;
        }
        let mut finished = false;
        for event in pending {
            match event {
                TerminalEvent::Output(line) => self.push_line(TerminalLineKind::Output, line),
                TerminalEvent::Error(line) => self.push_line(TerminalLineKind::Error, line),
                TerminalEvent::Exited(code) => {
                    self.last_exit_code = Some(code.unwrap_or(0));
                    self.running_command = None;
                    finished = true;
                }
            }
        }
        if finished || disconnected {
            self.running_command = None;
            self.event_rx = None;
        }
        true
    }

    pub fn wait_for_completion(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while self.is_running() && Instant::now() < deadline {
            self.poll();
            if !self.is_running() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        self.poll();
        !self.is_running()
    }
}

fn strip_windows_verbatim(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

pub fn render_terminal_pane(
    frame: &mut Frame<'_>,
    area: Rect,
    pane: &TerminalPaneState,
    theme: &Theme,
    mode_color: Color,
) {
    if area.width < 10 || area.height < 4 {
        return;
    }

    let border_color = if pane.is_focused() {
        mode_color
    } else {
        theme.dim
    };

    let status_badge = if let Some(cmd) = pane.running_command() {
        format!("running `{cmd}`")
    } else if let Some(code) = pane.last_exit_code() {
        format!("exit {code}")
    } else {
        "idle".to_string()
    };

    let cwd_str = pane.cwd().display().to_string();
    let max_cwd = (area.width as usize).saturating_sub(42).max(10);
    let short_cwd: String = if cwd_str.chars().count() > max_cwd {
        let tail: String = cwd_str
            .chars()
            .rev()
            .take(max_cwd.saturating_sub(1))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("…{tail}")
    } else {
        cwd_str
    };

    let focus_hint = if pane.is_focused() {
        " [FOCUSED • Esc composer • Ctrl+X t hide] "
    } else {
        " [Ctrl+X ↑ focus • Ctrl+X t hide] "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(theme.bg_element))
        .title(Span::styled(
            format!(" Terminal • {short_cwd} • {status_badge}{focus_hint}"),
            Style::default()
                .fg(if pane.is_focused() {
                    mode_color
                } else {
                    theme.ink
                })
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 || inner.width < 4 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let scrollback_area = chunks[0];
    let prompt_area = chunks[1];

    let visible_rows = scrollback_area.height as usize;
    let total = pane.lines().len();
    let end = total.saturating_sub(pane.scroll_offset() as usize);
    let start = end.saturating_sub(visible_rows);

    let mut rendered_lines = Vec::with_capacity(visible_rows);
    for line in pane
        .lines()
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
    {
        let style = match line.kind {
            TerminalLineKind::Command => Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
            TerminalLineKind::Output => Style::default().fg(theme.ink),
            TerminalLineKind::Error => Style::default().fg(theme.error),
            TerminalLineKind::System => Style::default().fg(theme.quiet),
        };
        rendered_lines.push(Line::from(Span::styled(format!(" {}", line.text), style)));
    }
    frame.render_widget(Paragraph::new(rendered_lines), scrollback_area);

    let prompt_prefix = " $ ";
    let input_text = pane.input();
    let prompt_line = Line::from(vec![
        Span::styled(
            prompt_prefix,
            Style::default()
                .fg(if pane.is_focused() {
                    theme.teal
                } else {
                    theme.dim
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            input_text,
            Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(Paragraph::new(prompt_line), prompt_area);

    if pane.is_focused() {
        let prefix_chars: String = input_text.chars().take(pane.cursor()).collect();
        let cursor_w = UnicodeWidthStr::width(prefix_chars.as_str()) as u16;
        let x = prompt_area
            .x
            .saturating_add(3)
            .saturating_add(cursor_w)
            .min(prompt_area.x + prompt_area.width.saturating_sub(1));
        frame.set_cursor_position((x, prompt_area.y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_pane_executes_builtins_and_external_commands() {
        let mut pane = TerminalPaneState::default();
        assert!(!pane.is_visible());
        assert!(pane.toggle());
        assert!(pane.is_visible());
        assert!(pane.is_focused());

        pane.run_command("pwd");
        assert_eq!(pane.last_exit_code(), Some(0));

        pane.run_command("echo clawcode_term_ok");
        assert!(pane.wait_for_completion(Duration::from_secs(5)));
        assert_eq!(pane.last_exit_code(), Some(0));
        assert!(
            pane.lines()
                .iter()
                .any(|line| line.text.contains("clawcode_term_ok"))
        );

        pane.run_command("clear");
        assert!(pane.lines().is_empty());
    }
}
