use super::App;

impl App {
    pub fn open_session_tabs(&self) -> &[i64] {
        &self.open_session_tabs
    }

    pub fn session_tab_title(&self, session_id: i64) -> Option<&str> {
        self.session_titles.get(&session_id).map(String::as_str)
    }

    pub fn open_session_in_tab(&mut self, session_id: i64) {
        self.open_session_in_tab_with_title(session_id, None);
    }

    pub fn open_session_in_tab_with_title(&mut self, session_id: i64, title: Option<&str>) {
        if !self.open_session_tabs.contains(&session_id) {
            self.open_session_tabs.push(session_id);
        }
        if let Some(title) = title {
            self.session_titles.insert(session_id, title.to_string());
        } else if !self.session_titles.contains_key(&session_id) {
            self.cache_session_title(session_id);
        }
        if self.active_session_id != Some(session_id) {
            self.switch_session(session_id);
        }
    }

    pub fn cycle_session_tab(&mut self, forward: bool) {
        self.ensure_active_tab();
        if self.open_session_tabs.len() <= 1 {
            self.diagnostic = "no other open session tabs".to_string();
            return;
        }
        let current = self
            .active_session_id
            .and_then(|id| self.open_session_tabs.iter().position(|tab| *tab == id));
        let next = match (current, forward) {
            (Some(index), true) => (index + 1) % self.open_session_tabs.len(),
            (Some(index), false) => {
                (index + self.open_session_tabs.len() - 1) % self.open_session_tabs.len()
            }
            (None, true) => 0,
            (None, false) => self.open_session_tabs.len() - 1,
        };
        if let Some(session_id) = self.open_session_tabs.get(next).copied() {
            self.switch_session(session_id);
            let label = self
                .session_titles
                .get(&session_id)
                .cloned()
                .unwrap_or_else(|| format!("#{session_id}"));
            self.diagnostic = format!("session tab: {label}");
        }
    }

    pub fn select_session_tab(&mut self, index: usize) {
        self.ensure_active_tab();
        if let Some(session_id) = self.open_session_tabs.get(index).copied() {
            self.switch_session(session_id);
            let label = self
                .session_titles
                .get(&session_id)
                .cloned()
                .unwrap_or_else(|| format!("#{session_id}"));
            self.diagnostic = format!("session tab: {label}");
        } else {
            self.diagnostic = format!("session tab {} is empty", index + 1);
        }
    }

    pub fn close_active_session_tab(&mut self) {
        let Some(session_id) = self.active_session_id else {
            self.diagnostic = "no active session tab to close".to_string();
            return;
        };
        let Some(index) = self
            .open_session_tabs
            .iter()
            .position(|tab| *tab == session_id)
        else {
            self.diagnostic = "active session is not open in a tab".to_string();
            return;
        };
        self.open_session_tabs.remove(index);
        self.recently_closed_tabs.retain(|tab| *tab != session_id);
        self.recently_closed_tabs.push(session_id);
        if self.recently_closed_tabs.len() > 10 {
            self.recently_closed_tabs.remove(0);
        }

        if let Some(next) = self
            .open_session_tabs
            .get(index.min(self.open_session_tabs.len().saturating_sub(1)))
            .copied()
        {
            self.switch_session(next);
            self.diagnostic = "session tab closed".to_string();
        } else {
            self.stash_active();
            self.active_session_id = None;
            self.reset_turn_view(true);
            self.prompt.clear();
            self.cursor_position = 0;
            self.status = super::ConversationStatus::Idle;
            self.diagnostic = "session tab closed".to_string();
        }
    }

    pub fn reopen_closed_session_tab(&mut self) {
        while let Some(session_id) = self.recently_closed_tabs.pop() {
            if self.open_session_tabs.contains(&session_id) {
                continue;
            }
            self.open_session_tabs.push(session_id);
            self.cache_session_title(session_id);
            self.switch_session(session_id);
            self.diagnostic = "reopened session tab".to_string();
            return;
        }
        self.diagnostic = "no recently closed session tab".to_string();
    }

    pub fn remove_session_tab(&mut self, session_id: i64) {
        self.open_session_tabs.retain(|tab| *tab != session_id);
        self.recently_closed_tabs.retain(|tab| *tab != session_id);
        self.session_titles.remove(&session_id);
    }

    fn ensure_active_tab(&mut self) {
        let Some(session_id) = self.active_session_id else {
            return;
        };
        if !self.open_session_tabs.contains(&session_id) {
            self.open_session_tabs.push(session_id);
            self.cache_session_title(session_id);
        }
    }

    fn cache_session_title(&mut self, session_id: i64) {
        if self.session_titles.contains_key(&session_id) {
            return;
        }
        if let Ok(sessions) = self.command_service.list_sessions()
            && let Some(session) = sessions
                .into_iter()
                .find(|session| session.id == session_id)
        {
            self.session_titles.insert(session_id, session.title);
        }
    }
}
