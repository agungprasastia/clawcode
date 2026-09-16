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
    ConnectProvider(String),
    Model(String),
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
        "/model" => Ok(Command::Models),
        value if value.starts_with("/new ") => {
            let title = value[5..].trim();
            if title.is_empty() {
                Err("usage: /new <title>; title cannot be empty".into())
            } else {
                Ok(Command::New(title.to_owned()))
            }
        }
        value if value.starts_with("/model ") => {
            let model = value[7..].trim();
            if model.is_empty() {
                Ok(Command::Models)
            } else {
                Ok(Command::Model(model.to_owned()))
            }
        }
        value if value.starts_with("/connect ") => {
            let provider = value[9..].trim();
            if provider.is_empty() {
                Ok(Command::Connect)
            } else {
                Ok(Command::ConnectProvider(provider.to_owned()))
            }
        }
        "" => Err("enter a command; try /plan, /build, /connect, or /help".into()),
        value => Err(format!(
            "unknown command `{value}`; try /plan, /build, /connect, or /help"
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
    ModelSelected(String),
    Models(Vec<ModelInfo>),
    RefreshStarted,
    Help(String),
}

pub struct CommandService<D> {
    db: Option<Db>,
    mode: ConversationMode,
    provider: Option<ProviderId>,
    default_provider: Option<ProviderId>,
    discovery: DiscoveryService,
    source: D,
    pending_refresh: Option<PendingRefresh>,
    diagnostic: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct CliDiscovery {
    custom_models: std::collections::HashMap<ProviderId, Vec<ModelInfo>>,
}

impl CliDiscovery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_models(custom_models: std::collections::HashMap<ProviderId, Vec<ModelInfo>>) -> Self {
        Self { custom_models }
    }

    pub fn add_provider_models(&mut self, provider: ProviderId, models: Vec<ModelInfo>) {
        self.custom_models.insert(provider, models);
    }
}

pub fn runtime_service_with_config(
    config: &crate::config::Config,
) -> Result<CommandService<CliDiscovery>, rusqlite::Error> {
    let mut custom_models = std::collections::HashMap::new();
    for (name, provider_cfg) in &config.providers {
        let pid = ProviderId::new(name);
        custom_models.insert(pid, provider_cfg.to_model_infos());
    }
    let discovery_source = CliDiscovery::with_models(custom_models.clone());
    let mut service = CommandService::with_db(discovery_source, Db::open_in_memory()?);
    for (pid, models) in custom_models {
        service.discovery.apply(pid, Ok(models));
    }
    let (default_p, _) = config.initial_provider_and_model();
    if !default_p.is_empty() {
        let pid = ProviderId::new(default_p);
        service.set_default_provider(pid.clone());
        service.set_provider(pid);
    }
    Ok(service)
}

pub fn runtime_service() -> Result<CommandService<CliDiscovery>, rusqlite::Error> {
    let config = crate::config::ConfigLoader.load().unwrap_or_default();
    runtime_service_with_config(&config)
}

impl DiscoverySource for CliDiscovery {
    fn discover(&self, provider: &ProviderId) -> Result<Vec<ModelInfo>, ProviderError> {
        if let Some(models) = self.custom_models.get(provider) {
            Ok(models.clone())
        } else {
            Err(ProviderError::Network(
                "provider discovery unavailable".into(),
            ))
        }
    }
}

impl<D: DiscoverySource + Clone> CommandService<D> {
    pub fn new(source: D) -> Self {
        Self {
            db: None,
            mode: ConversationMode::Plan,
            provider: None,
            default_provider: None,
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

    pub fn set_provider(&mut self, provider: ProviderId) {
        self.provider = Some(provider);
    }

    pub fn set_default_provider(&mut self, provider: ProviderId) {
        self.default_provider = Some(provider);
    }

    pub fn register_models(&mut self, provider: ProviderId, models: Vec<ModelInfo>) {
        self.discovery.apply(provider, Ok(models));
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
            Command::ConnectProvider(name) => {
                let provider = ProviderId::new(name);
                self.provider = Some(provider.clone());
                Ok(CommandOutput::Connected(provider))
            }
            Command::Model(model) => Ok(CommandOutput::ModelSelected(model)),
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
                "available commands: /plan, /build, /connect, /model <id>, /models, /sessions, /new <title>, /help, /exit".into(),
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
        let mut service = runtime_service().map_err(|e| std::io::Error::other(e.to_string()))?;
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
