pub mod app;
pub mod chat;
pub mod dialogs;
pub mod diff;
pub mod home;
mod input;
mod render;
mod theme;
pub mod typewriter;
pub mod wave_spinner;

use std::{convert::Infallible, io, time::Duration};

use crossterm::{
    event, execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub use app::{
    App, ConversationMode, ConversationStatus, Input, UiEvent, UiEventQueue,
    format_tool_success_detail, is_sensitive_command, tool_target_and_verbs,
};
pub use chat::{format_transcript_lines, render_chat};
pub use dialogs::{
    AgentItem, AgentsDialogState, ModelsDialogState, PermissionDecision, PermissionDialogState,
    PermissionPrompt, QuestionDialogState, SessionsDialogState, StatusDialogState,
    ThemesDialogState, WhichKeyState, render_permission_dialog, render_question_dialog,
};
pub use diff::{DiffLine, DiffOp, DiffResult, compute_diff};
pub use home::HomeState;
pub use render::render;
pub use theme::{Theme, ThemeKind};
pub use typewriter::TypewriterState;
pub use wave_spinner::WaveSpinner;

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(16);
const UI_EVENT_QUEUE_CAPACITY: usize = 64;

pub fn default_data_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("CLAWCODE_DATA_DIR")
        && !dir.is_empty() {
            return std::path::PathBuf::from(dir);
        }
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA")
        && !local_app_data.is_empty() {
            return std::path::PathBuf::from(local_app_data).join("clawcode");
        }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
        && !xdg.is_empty() {
            return std::path::PathBuf::from(xdg).join("clawcode");
        }
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME"))
        && !home.is_empty() {
            return std::path::PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("clawcode");
        }
    std::env::temp_dir().join("clawcode")
}

pub fn attach_production_runtime(app: &mut App) {
    let config = crate::config::ConfigLoader.load().unwrap_or_default();
    let data_dir = default_data_dir();
    let _ = std::fs::create_dir_all(&data_dir);
    let db_path = data_dir.join("clawcode.db");

    let (db_runtime, db_writer, db_cli) = match (
        crate::persistence::Db::open(&db_path),
        crate::persistence::Db::open(&db_path),
        crate::persistence::Db::open(&db_path),
    ) {
        (Ok(r), Ok(w), Ok(c)) => (r, w, c),
        _ => return,
    };

    if let Ok(cwd) = std::env::current_dir() {
        let _ = db_runtime.update_workspace_root_path(1, &cwd.to_string_lossy());
    }

    if let Ok(service) = crate::cli::runtime_service_with_config_and_db(&config, db_cli) {
        app.set_command_service(service);
    }

    let writer = crate::persistence::WriterHandle::spawn(db_writer);
    let router = crate::adapters::ConfiguredRouter::new(config);
    app.attach_runtime(db_runtime, writer, Box::new(router));
}

pub fn run() -> io::Result<()> {
    let mut terminal = TerminalSession::start()?;
    let mut app = App::default();
    attach_production_runtime(&mut app);
    let mut events = UiEventQueue::new(UI_EVENT_QUEUE_CAPACITY);

    terminal
        .terminal
        .draw(|frame| render::render(frame, &app))?;
    while app.is_running() {
        let input = event::poll(INPUT_POLL_INTERVAL)?
            .then(event::read)
            .transpose()?;
        runtime_step(&mut app, &mut events, input, |app| {
            terminal
                .terminal
                .draw(|frame| render::render(frame, app))
                .map(|_| ())
        })?;
    }

    Ok(())
}

pub fn process_pending<E>(
    app: &mut App,
    events: &mut UiEventQueue,
    draw: impl FnOnce(&App) -> Result<(), E>,
) -> Result<(), E> {
    let runtime_updated = app.poll_runtime();
    let anim_ticked = app.tick();
    if (runtime_updated || anim_ticked || app.apply_pending(events)) && app.is_running() {
        draw(app)?;
    }
    Ok(())
}

pub fn runtime_step<E>(
    app: &mut App,
    events: &mut UiEventQueue,
    input_event: Option<event::Event>,
    draw: impl FnOnce(&App) -> Result<(), E>,
) -> Result<(), E> {
    if let Some(event) = input_event.and_then(input::translate) {
        events.push(event);
    }
    process_pending(app, events, draw)
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
        if let Err(error) = execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture) {
            let _ = disable_raw_mode();
            return Err(error);
        }

        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let _ = execute!(
                    io::stdout(),
                    LeaveAlternateScreen,
                    event::DisableMouseCapture
                );
                let _ = disable_raw_mode();
                Err(error)
            }
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            event::DisableMouseCapture
        );
        let _ = disable_raw_mode();
        let _ = self.terminal.show_cursor();
    }
}
