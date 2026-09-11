use super::Risk;

pub fn classify_shell(command: &str) -> Risk {
    let command = command.to_ascii_lowercase();

    if contains_command(&command, "sudo") || contains_command(&command, "runas") {
        Risk::PrivilegeEscalation
    } else if command.contains("curl") && command.contains('|') && contains_command(&command, "sh")
    {
        Risk::NetworkMutation
    } else if command.contains("git reset")
        || command.contains("git clean")
        || command.contains("rm -rf")
        || command.contains("del /s")
    {
        Risk::Destructive
    } else if is_dependency_install(&command) {
        Risk::DependencyInstall
    } else {
        Risk::Safe
    }
}

fn contains_command(command: &str, name: &str) -> bool {
    command
        .split(|character: char| character.is_whitespace() || matches!(character, ';' | '|' | '&'))
        .any(|part| part == name)
}

fn is_dependency_install(command: &str) -> bool {
    [
        "cargo install",
        "npm install",
        "npm i",
        "pnpm install",
        "pnpm add",
        "yarn add",
        "yarn install",
        "pip install",
        "pip3 install",
        "gem install",
        "brew install",
        "apt install",
        "apt-get install",
    ]
    .iter()
    .any(|installer| command.contains(installer))
}
