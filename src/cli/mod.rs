use clawcode::provider::{DiscoveryService, DiscoverySource, ModelInfo, ProviderError, ProviderId};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

type RefreshResult = Result<Vec<ModelInfo>, ProviderError>;
type PendingRefresh = (ProviderId, Receiver<RefreshResult>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Connect,
    Models,
    ModelsRefresh,
}

pub fn parse_command(input: &str) -> Option<Command> {
    match input.trim() {
        "/connect" => Some(Command::Connect),
        "/models" => Some(Command::Models),
        "/models refresh" => Some(Command::ModelsRefresh),
        _ => None,
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum CommandOutput {
    Connected(ProviderId),
    Models(Vec<ModelInfo>),
    RefreshStarted,
}

pub struct CommandService<D> {
    provider: Option<ProviderId>,
    discovery: DiscoveryService,
    source: D,
    pending_refresh: Option<PendingRefresh>,
}

#[derive(Clone, Debug)]
struct CliDiscovery;

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
            provider: None,
            discovery: DiscoveryService::new(Duration::from_secs(300), Duration::from_secs(5)),
            source,
            pending_refresh: None,
        }
    }

    pub fn execute(&mut self, command: Command) -> Result<CommandOutput, ProviderError> {
        self.poll_refresh();
        match command {
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
        }
    }

    pub fn poll_refresh(&mut self) {
        let Some((provider, receiver)) = self.pending_refresh.take() else {
            return;
        };
        match receiver.try_recv() {
            Ok(result) => self.discovery.apply(provider, result),
            Err(TryRecvError::Empty) => {
                self.pending_refresh = Some((provider, receiver));
            }
            Err(TryRecvError::Disconnected) => self.discovery.apply(
                provider,
                Err(ProviderError::Network(
                    "model discovery worker stopped".into(),
                )),
            ),
        }
    }

    pub fn models(&mut self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.poll_refresh();
        let provider = self
            .provider
            .as_ref()
            .ok_or_else(|| ProviderError::Protocol("connect provider first".into()))?;
        Ok(self.discovery.models(provider).unwrap_or(&[]).to_vec())
    }
}

pub fn run() -> std::io::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().map(String::as_str) == Some("--version") {
        println!("clawcode {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if let Some(command) = parse_command(&arguments.join(" ")) {
        let mut service = CommandService::new(CliDiscovery);
        let output = service
            .execute(command)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        println!("{output:?}");
        return Ok(());
    }

    clawcode::tui::run()
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
        assert_eq!(parse_command("/connect"), Some(Command::Connect));
        assert_eq!(parse_command("/models"), Some(Command::Models));
        assert_eq!(
            parse_command("/models refresh"),
            Some(Command::ModelsRefresh)
        );
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
