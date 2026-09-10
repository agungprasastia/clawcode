mod app;
mod input;
mod render;

use std::{convert::Infallible, io, time::Duration};

use crossterm::{
    event, execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub use app::{App, Input, UiEvent, UiEventQueue};

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(16);
const UI_EVENT_QUEUE_CAPACITY: usize = 64;

pub fn run() -> io::Result<()> {
    let mut terminal = TerminalSession::start()?;
    let mut app = App::default();
    let mut events = UiEventQueue::new(UI_EVENT_QUEUE_CAPACITY);

    terminal
        .terminal
        .draw(|frame| render::render(frame, &app))?;
    while app.is_running() {
        if event::poll(INPUT_POLL_INTERVAL)?
            && let Some(event) = input::translate(event::read()?)
        {
            events.push(event);
        }
        if app.apply_pending(&mut events) && app.is_running() {
            terminal
                .terminal
                .draw(|frame| render::render(frame, &app))?;
        }
    }

    Ok(())
}

pub fn render_to_test_backend(app: &App, width: u16, height: u16) -> Result<(), Infallible> {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| render::render(frame, app))?;
    Ok(())
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalSession {
    fn start() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }

        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                let _ = disable_raw_mode();
                Err(error)
            }
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}
