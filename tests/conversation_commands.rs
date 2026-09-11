use clawcode::cli::{Command, CommandOutput, CommandService, ConversationMode, parse_command};
use clawcode::persistence::Db;
use clawcode::provider::{DiscoverySource, ModelInfo, ProviderError, ProviderId};

#[derive(Clone)]
struct Source;

impl DiscoverySource for Source {
    fn discover(&self, _provider: &ProviderId) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: "model-a".into(),
            context_window: 4096,
        }])
    }
}

#[test]
fn parses_session_and_mode_commands() {
    assert_eq!(
        parse_command("/new planning"),
        Some(Command::New("planning".into()))
    );
    assert_eq!(parse_command("/sessions"), Some(Command::Sessions));
    assert_eq!(parse_command("/exit"), Some(Command::Exit));
    assert_eq!(
        parse_command("/plan"),
        Some(Command::Mode(ConversationMode::Plan))
    );
    assert_eq!(
        parse_command("/build"),
        Some(Command::Mode(ConversationMode::Build))
    );
}

#[test]
fn creates_lists_and_selects_bounded_session() {
    let db = Db::open_in_memory().unwrap();
    let mut service = CommandService::with_db(Source, db);
    let output = service
        .execute(parse_command("/new a very long title that should be bounded").unwrap())
        .unwrap();
    assert!(matches!(output, CommandOutput::SessionCreated(session) if session.title.len() <= 80));
    assert!(
        matches!(service.execute(Command::Sessions).unwrap(), CommandOutput::Sessions(sessions) if sessions.len() == 1)
    );
}

#[test]
fn mode_switch_and_refresh_are_non_blocking() {
    let db = Db::open_in_memory().unwrap();
    let mut service = CommandService::with_db(Source, db);
    assert_eq!(
        service
            .execute(Command::Mode(ConversationMode::Build))
            .unwrap(),
        CommandOutput::Mode(ConversationMode::Build)
    );
    assert_eq!(
        service.execute(Command::Connect).unwrap(),
        CommandOutput::Connected(ProviderId::new("openai"))
    );
    assert_eq!(
        service.execute(Command::ModelsRefresh).unwrap(),
        CommandOutput::RefreshStarted
    );
}

#[test]
fn unknown_command_has_actionable_diagnostic() {
    assert_eq!(parse_command("/wat"), None);
}
