use crate::persistence::{Db, Session};
use crate::provider::{DiscoveryService, DiscoverySource, ModelInfo, ProviderError, ProviderId};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

type RefreshResult = Result<Vec<ModelInfo>, ProviderError>;
type PendingRefresh = (ProviderId, Receiver<RefreshResult>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    New(String),
    Sessions,
    Exit,
    Mode(ConversationMode),
    Connect,
    Models,
    ModelsRefresh,
    Help,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationMode {
    Plan,
    Build,
}

pub fn parse_command(input: &str) -> Result<Command, String> {
    match input.trim() {
        "/sessions" => Ok(Command::Sessions),
        "/exit" => Ok(Command::Exit),
        "/plan" => Ok(Command::Mode(ConversationMode::Plan)),
        "/build" => Ok(Command::Mode(ConversationMode::Build)),
        "/connect" => Ok(Command::Connect),
        "/models" => Ok(Command::Models),
        "/models refresh" => Ok(Command::ModelsRefresh),
        "/help" => Ok(Command::Help),
        "/new" => Err("usage: /new <title>".into()),
        value if value.starts_with("/new ") => {
            let title = value[5..].trim();
            if title.is_empty() {
                Err("usage: /new <title>; title cannot be empty".into())
            } else {
                Ok(Command::New(title.to_owned()))
            }
        }
        "" => Err("enter a command; try /plan, /build, or /help".into()),
        value => Err(format!(
            "unknown command `{value}`; try /plan, /build, or /help"
        )),
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum CommandOutput {
    SessionCreated(Session),
    Sessions(Vec<Session>),
    Mode(ConversationMode),
    Exit,
    Connected(ProviderId),
    Models(Vec<ModelInfo>),
    RefreshStarted,
    Help(String),
}

pub struct CommandService<D> {
    db: Option<Db>,
    mode: ConversationMode,
    provider: Option<ProviderId>,
    discovery: DiscoveryService,
    source: D,
    pending_refresh: Option<PendingRefresh>,
    diagnostic: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CliDiscovery;

pub fn runtime_service() -> Result<CommandService<CliDiscovery>, rusqlite::Error> {
    Ok(CommandService::with_db(CliDiscovery, Db::open_in_memory()?))
}

impl DiscoverySource for CliDiscovery {
    fn discover(&self, _provider: &ProviderId) -> Result<Vec<ModelInfo>, ProviderError> {
        Err(ProviderError::Network(
            "provider discovery unavailable".into(),
        ))
    }
}

impl<D: DiscoverySource + Clone> CommandService<D> {
    pub fn new(source: D) -> Self {
        Self {
            db: None,
            mode: ConversationMode::Plan,
            provider: None,
            discovery: DiscoveryService::new(Duration::from_secs(300), Duration::from_secs(5)),
            source,
            pending_refresh: None,
            diagnostic: None,
        }
    }

    pub fn with_db(source: D, db: Db) -> Self {
        let mut service = Self::new(source);
        service.db = Some(db);
        service
    }

    pub fn execute(&mut self, command: Command) -> Result<CommandOutput, ProviderError> {
        self.poll_refresh();
        match command {
            Command::New(title) => {
                let title = bounded_title(&title);
                let session = self
                    .db
                    .as_ref()
                    .ok_or_else(|| ProviderError::Protocol("session database unavailable".into()))
                    .and_then(|db| {
                        db.create_session(&title)
                            .map_err(|error| ProviderError::Protocol(error.to_string()))
                    })?;
                Ok(CommandOutput::SessionCreated(session))
            }
            Command::Sessions => {
                let sessions = self
                    .db
                    .as_ref()
                    .ok_or_else(|| ProviderError::Protocol("session database unavailable".into()))?
                    .list_sessions()
                    .map_err(|error| ProviderError::Protocol(error.to_string()))?;
                Ok(CommandOutput::Sessions(sessions))
            }
            Command::Exit => Ok(CommandOutput::Exit),
            Command::Mode(mode) => {
                self.mode = mode;
                Ok(CommandOutput::Mode(mode))
            }
            Command::Connect => {
                let provider = ProviderId::new("openai");
                self.provider = Some(provider.clone());
                Ok(CommandOutput::Connected(provider))
            }
            Command::Models => Ok(CommandOutput::Models(self.models()?)),
            Command::ModelsRefresh => {
                let provider = self
                    .provider
                    .clone()
                    .ok_or_else(|| ProviderError::Protocol("connect provider first".into()))?;
                let receiver = self.discovery.models_refresh(
                    provider,
                    self.source.clone(),
                    Duration::from_secs(2),
                );
                self.pending_refresh = Some((
                    self.provider.clone().expect("provider checked above"),
                    receiver,
                ));
                Ok(CommandOutput::RefreshStarted)
            }
            Command::Help => Ok(CommandOutput::Help(
                "available commands: /plan, /build, /models, /sessions, /new <title>, /exit".into(),
            )),
        }
    }

    pub fn poll_refresh(&mut self) {
        let Some((provider, receiver)) = self.pending_refresh.take() else {
            return;
        };
        match receiver.try_recv() {
            Ok(result) => {
                if let Err(error) = &result {
                    self.diagnostic = Some(format!("model refresh failed: {error}"));
                }
                self.discovery.apply(provider, result)
            }
            Err(TryRecvError::Empty) => {
                self.pending_refresh = Some((provider, receiver));
            }
            Err(TryRecvError::Disconnected) => {
                let error = ProviderError::Network("model discovery worker stopped".into());
                self.diagnostic = Some(format!("model refresh failed: {error}"));
                self.discovery.apply(provider, Err(error));
            }
        }
    }

    pub fn take_diagnostic(&mut self) -> Option<String> {
        self.diagnostic.take()
    }

    pub fn models(&mut self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.poll_refresh();
        let provider = self
            .provider
            .as_ref()
            .ok_or_else(|| ProviderError::Protocol("connect provider first".into()))?;
        Ok(self.discovery.models(provider).unwrap_or(&[]).to_vec())
    }

    pub fn is_connected(&self) -> bool {
        self.provider.is_some()
    }

    pub fn provider(&self) -> Option<&ProviderId> {
        self.provider.as_ref()
    }

    pub fn create_session(&self, title: &str) -> Result<Session, ProviderError> {
        let title = bounded_title(title);
        self.db
            .as_ref()
            .ok_or_else(|| ProviderError::Protocol("session database unavailable".into()))
            .and_then(|db| {
                db.create_session(&title)
                    .map_err(|error| ProviderError::Protocol(error.to_string()))
            })
    }

    pub fn append_message(
        &self,
        session_id: i64,
        role: &str,
        content: &str,
    ) -> Result<(), ProviderError> {
        self.db
            .as_ref()
            .ok_or_else(|| ProviderError::Protocol("session database unavailable".into()))
            .and_then(|db| {
                db.append_message(session_id, role, content)
                    .map(|_| ())
                    .map_err(|error| ProviderError::Protocol(error.to_string()))
            })
    }
}

const MAX_SESSION_TITLE_BYTES: usize = 80;

fn bounded_title(title: &str) -> String {
    let end = title
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(title.len()))
        .take_while(|index| *index <= MAX_SESSION_TITLE_BYTES)
        .last()
        .unwrap_or(0);
    title[..end].to_owned()
}

pub fn run() -> std::io::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().map(String::as_str) == Some("--version") {
        println!("clawcode {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if !arguments.is_empty() {
        let command = parse_command(&arguments.join(" ")).map_err(std::io::Error::other)?;
        let mut service = CommandService::new(CliDiscovery);
        let output = service
            .execute(command)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        println!("{output:?}");
        return Ok(());
    }

    crate::tui::run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug)]
    struct Source;

    impl DiscoverySource for Source {
        fn discover(&self, _provider: &ProviderId) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![ModelInfo {
                id: "mock-model".into(),
                context_window: 4096,
            }])
        }
    }

    #[test]
    fn parses_provider_commands() {
        assert_eq!(parse_command("/connect"), Ok(Command::Connect));
        assert_eq!(parse_command("/models"), Ok(Command::Models));
        assert_eq!(parse_command("/models refresh"), Ok(Command::ModelsRefresh));
    }

    #[test]
    fn refresh_applies_models_without_blocking_command() {
        let mut service = CommandService::new(Source);
        assert!(matches!(
            service.execute(Command::Connect),
            Ok(CommandOutput::Connected(_))
        ));
        assert_eq!(
            service.execute(Command::Models).unwrap(),
            CommandOutput::Models(vec![])
        );
        assert_eq!(
            service.execute(Command::ModelsRefresh).unwrap(),
            CommandOutput::RefreshStarted
        );
        for _ in 0..100 {
            service.poll_refresh();
            if service.models().unwrap()
                == vec![ModelInfo {
                    id: "mock-model".into(),
                    context_window: 4096,
                }]
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("refresh result was not applied");
    }
}
