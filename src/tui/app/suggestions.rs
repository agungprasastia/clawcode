use super::App;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSuggestion {
    pub name: &'static str,
    pub description: &'static str,
    pub template: &'static str,
}

pub const AVAILABLE_COMMANDS: &[CommandSuggestion] = &[
    CommandSuggestion {
        name: "/plan",
        description: "Switch to read-only Plan mode",
        template: "/plan",
    },
    CommandSuggestion {
        name: "/build",
        description: "Switch to Build mode with edits enabled",
        template: "/build",
    },
    CommandSuggestion {
        name: "/model",
        description: "Select active model",
        template: "/model",
    },
    CommandSuggestion {
        name: "/models",
        description: "Interactive model picker",
        template: "/models",
    },
    CommandSuggestion {
        name: "/models refresh",
        description: "Refresh provider model list",
        template: "/models refresh",
    },
    CommandSuggestion {
        name: "/agents",
        description: "Interactive agent picker & mode switcher",
        template: "/agents",
    },
    CommandSuggestion {
        name: "/themes",
        description: "Interactive theme selector & color palette",
        template: "/themes",
    },
    CommandSuggestion {
        name: "/theme",
        description: "Switch color theme (e.g. /theme catppuccin)",
        template: "/theme ",
    },
    CommandSuggestion {
        name: "/keys",
        description: "Keyboard shortcuts cheatsheet (Ctrl+X)",
        template: "/keys",
    },
    CommandSuggestion {
        name: "/status",
        description: "Show session status & diagnostics",
        template: "/status",
    },
    CommandSuggestion {
        name: "/git",
        description: "Interactive git status, diff viewer & staging",
        template: "/git",
    },
    CommandSuggestion {
        name: "/skills",
        description: "Interactive skill picker & library",
        template: "/skills",
    },
    CommandSuggestion {
        name: "/skill ",
        description: "Load and invoke a specific skill",
        template: "/skill ",
    },
    CommandSuggestion {
        name: "/connect",
        description: "Connect configured AI provider",
        template: "/connect",
    },
    CommandSuggestion {
        name: "/clear",
        description: "Clear conversation & return to home",
        template: "/clear",
    },
    CommandSuggestion {
        name: "/compact",
        description: "Compact session context",
        template: "/compact",
    },
    CommandSuggestion {
        name: "/copy",
        description: "Copy transcript or session status",
        template: "/copy",
    },
    CommandSuggestion {
        name: "/sessions",
        description: "List saved chat sessions",
        template: "/sessions",
    },
    CommandSuggestion {
        name: "/new",
        description: "Create a new conversation session",
        template: "/new ",
    },
    CommandSuggestion {
        name: "/help",
        description: "Show manual, commands & shortcuts",
        template: "/help",
    },
    CommandSuggestion {
        name: "/exit",
        description: "Quit Clawcode workbench",
        template: "/exit",
    },
];

impl App {
    pub fn matching_suggestions(&self) -> Vec<&'static CommandSuggestion> {
        if !self.prompt.starts_with('/') {
            return Vec::new();
        }
        let query = self.prompt.trim_start_matches('/');
        let mut exact_matches = Vec::new();
        let mut other_matches = Vec::new();
        for cmd in AVAILABLE_COMMANDS {
            let cmd_name = cmd.name.trim_start_matches('/');
            if cmd_name.starts_with(query) {
                exact_matches.push(cmd);
            } else if cmd.name.contains(query) {
                other_matches.push(cmd);
            }
        }
        exact_matches.extend(other_matches);
        exact_matches
    }

    pub fn selected_suggestion_index(&self) -> usize {
        let count = self.suggestion_count();
        if count == 0 {
            0
        } else {
            self.selected_suggestion.min(count - 1)
        }
    }

    pub fn available_models(&self) -> &[crate::provider::ModelInfo] {
        &self.available_models
    }

    pub fn matching_model_suggestions(&self) -> Vec<String> {
        if !self.prompt.starts_with("/model ") {
            return Vec::new();
        }
        let query = self
            .prompt
            .strip_prefix("/model ")
            .unwrap_or("")
            .trim()
            .to_lowercase();
        self.available_models
            .iter()
            .map(|m| m.id.clone())
            .filter(|id| query.is_empty() || id.to_lowercase().contains(&query))
            .collect()
    }

    pub fn matching_theme_suggestions(&self) -> Vec<&'static str> {
        if !self.prompt.starts_with("/theme ") {
            return Vec::new();
        }
        let query = self
            .prompt
            .strip_prefix("/theme ")
            .unwrap_or("")
            .trim()
            .to_lowercase();
        crate::tui::ThemeKind::ALL
            .iter()
            .filter_map(|t| {
                if query.is_empty()
                    || t.name().to_lowercase().contains(&query)
                    || t.id().to_lowercase().contains(&query)
                {
                    Some(t.name())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn suggestion_count(&self) -> usize {
        if self.prompt.starts_with("/model ") {
            self.matching_model_suggestions().len()
        } else if self.prompt.starts_with("/theme ") {
            self.matching_theme_suggestions().len()
        } else {
            self.matching_suggestions().len()
        }
    }

    pub fn next_suggestion(&mut self) {
        let count = self.suggestion_count();
        if count > 0 {
            self.selected_suggestion = (self.selected_suggestion + 1) % count;
        } else {
            self.selected_suggestion = 0;
        }
    }

    pub fn previous_suggestion(&mut self) {
        let count = self.suggestion_count();
        if count > 0 {
            self.selected_suggestion =
                if self.selected_suggestion == 0 || self.selected_suggestion >= count {
                    count - 1
                } else {
                    self.selected_suggestion - 1
                };
        } else {
            self.selected_suggestion = 0;
        }
    }

    pub fn autocomplete_selected_command(&mut self) -> bool {
        if self.prompt.starts_with("/model ") {
            let model_suggestions = self.matching_model_suggestions();
            let idx = self.selected_suggestion_index();
            if let Some(first) = model_suggestions
                .get(idx)
                .or_else(|| model_suggestions.first())
            {
                self.prompt = format!("/model {first}");
                self.cursor_position = self.prompt.chars().count();
                self.selected_suggestion = 0;
                return true;
            }
        }
        if self.prompt.starts_with("/theme ") {
            let theme_suggestions = self.matching_theme_suggestions();
            let idx = self.selected_suggestion_index();
            if let Some(first) = theme_suggestions
                .get(idx)
                .or_else(|| theme_suggestions.first())
            {
                self.prompt = format!("/theme {first}");
                self.cursor_position = self.prompt.chars().count();
                self.selected_suggestion = 0;
                return true;
            }
        }
        let suggestions = self.matching_suggestions();
        let idx = self.selected_suggestion_index();
        if let Some(suggestion) = suggestions
            .get(idx)
            .copied()
            .or_else(|| suggestions.first().copied())
        {
            self.prompt = suggestion.template.to_string();
            self.cursor_position = self.prompt.chars().count();
            self.selected_suggestion = 0;
            true
        } else {
            false
        }
    }
}
