use ratatui::layout::Rect;

use super::{App, ConversationMode};
use crate::cli;

impl App {
    pub fn terminal_size(&self) -> (u16, u16) {
        self.terminal_size.get()
    }

    pub fn set_terminal_size(&self, width: u16, height: u16) {
        self.terminal_size.set((width, height));
    }

    pub fn last_popup_area(&self) -> Option<Rect> {
        self.last_popup_area.get()
    }

    pub fn set_last_popup_area(&self, area: Option<Rect>) {
        self.last_popup_area.set(area);
    }

    pub fn last_quick_actions_area(&self) -> Option<[Rect; 4]> {
        self.last_quick_actions_area.get()
    }

    pub fn set_last_quick_actions_area(&self, areas: Option<[Rect; 4]>) {
        self.last_quick_actions_area.set(areas);
    }

    pub fn get_or_compute_popup_area(&self) -> Option<Rect> {
        if let Some(area) = self.last_popup_area.get() {
            return Some(area);
        }
        let count = self.suggestion_count();
        if count == 0 {
            return None;
        }
        let (width, height) = self.terminal_size.get();
        if width == 0 || height == 0 {
            return None;
        }
        let workspace_height = height.saturating_sub(1);
        let (input_x, input_y, input_w) = if self.transcript.is_empty() {
            let input_height = 5.min(workspace_height.saturating_sub(6));
            let input_y = workspace_height.saturating_sub(input_height + 2);
            let content_width = if width >= 106 {
                100
            } else {
                width.saturating_sub(4)
            };
            let input_x = width.saturating_sub(content_width) / 2;
            (input_x, input_y, content_width)
        } else {
            let input_y = workspace_height.saturating_sub(6);
            (0, input_y, width)
        };

        let available_space = input_y as usize;
        if available_space < 3 {
            return None;
        }
        let max_visible =
            if self.prompt.starts_with("/theme ") || self.prompt.starts_with("/model ") {
                8.min(available_space.saturating_sub(2))
            } else {
                6.min(available_space.saturating_sub(2))
            };
        let visible_count = count.min(max_visible);
        if visible_count == 0 {
            return None;
        }
        let popup_height = (visible_count as u16) + 2;
        let popup_y = input_y.saturating_sub(popup_height);
        let popup_width = if self.prompt.starts_with("/model ") {
            input_w.min(70)
        } else if self.prompt.starts_with("/theme ") {
            input_w.min(50)
        } else {
            input_w.min(64)
        };

        Some(Rect {
            x: input_x,
            y: popup_y,
            width: popup_width,
            height: popup_height,
        })
    }

    pub fn get_or_compute_quick_actions_area(&self) -> Option<[Rect; 4]> {
        if let Some(cards) = self.last_quick_actions_area.get() {
            return Some(cards);
        }
        let (width, height) = self.terminal_size.get();
        crate::tui::home::compute_quick_actions_area_for_size(width, height)
    }

    pub fn handle_mouse_click(&mut self, x: u16, y: u16) {
        if let Some(call_id) = self.tool_row_at(x, y) {
            if call_id == "__thought__" {
                self.toggle_thought_expanded();
                return;
            }
            let expandable = self
                .tool_rows
                .iter()
                .find(|row| row.call_id == call_id)
                .is_some_and(|row| row.expandable);
            if expandable {
                self.toggle_tool_expanded(&call_id);
                self.diagnostic = if self.is_tool_expanded(&call_id) {
                    "tool output expanded".to_string()
                } else {
                    "tool output collapsed".to_string()
                };
            }
            return;
        }

        let has_command_suggestions = !self.matching_suggestions().is_empty();
        let has_model_suggestions =
            self.prompt.starts_with("/model ") && !self.matching_model_suggestions().is_empty();
        let has_theme_suggestions =
            self.prompt.starts_with("/theme ") && !self.matching_theme_suggestions().is_empty();

        if (has_command_suggestions || has_model_suggestions || has_theme_suggestions)
            && let Some(popup_area) = self.get_or_compute_popup_area()
            && x >= popup_area.x
            && x < popup_area.x + popup_area.width
            && y >= popup_area.y
            && y < popup_area.y + popup_area.height
        {
            let visible_count = (popup_area.height.saturating_sub(2)) as usize;
            if visible_count == 0 {
                return;
            }
            let rel_row = if y <= popup_area.y + 1 {
                0
            } else {
                ((y.saturating_sub(popup_area.y + 1)) as usize).min(visible_count.saturating_sub(1))
            };
            let selected_idx = self.selected_suggestion_index();
            let scroll_offset = if selected_idx >= visible_count {
                (selected_idx + 1).saturating_sub(visible_count)
            } else {
                0
            };
            let item_idx = scroll_offset + rel_row;

            if has_model_suggestions {
                let model_suggestions = self.matching_model_suggestions();
                if let Some(model_id) = model_suggestions.get(item_idx) {
                    self.prompt = format!("/model {model_id}");
                    self.submit_prompt();
                    return;
                }
            } else if has_theme_suggestions {
                let theme_suggestions = self.matching_theme_suggestions();
                if let Some(theme_name) = theme_suggestions.get(item_idx) {
                    self.prompt = format!("/theme {theme_name}");
                    self.submit_prompt();
                    return;
                }
            } else {
                let suggestions = self.matching_suggestions();
                if let Some(suggestion) = suggestions.get(item_idx) {
                    if suggestion.template.ends_with(' ') {
                        self.prompt = suggestion.template.to_string();
                        self.cursor_position = self.prompt.chars().count();
                        self.selected_suggestion = 0;
                    } else {
                        self.prompt = suggestion.template.to_string();
                        self.submit_prompt();
                    }
                    return;
                }
            }
            return;
        }

        if self.transcript.is_empty()
            && let Some(cards) = self.get_or_compute_quick_actions_area()
        {
            for (idx, card) in cards.iter().enumerate() {
                if x >= card.x && x < card.x + card.width && y >= card.y && y < card.y + card.height
                {
                    self.prompt.clear();
                    self.cursor_position = 0;
                    match idx {
                        0 => {
                            self.set_mode(ConversationMode::Plan);
                            self.diagnostic = "Switched to Plan mode (read-only)".to_string();
                        }
                        1 => {
                            self.set_mode(ConversationMode::Build);
                            self.diagnostic = "Switched to Build mode (edits enabled)".to_string();
                        }
                        2 => {
                            let command = cli::parse_command("/models").expect("valid command");
                            if let Ok(output) = self.command_service.execute(command) {
                                self.apply_command_output(output);
                            }
                        }
                        3 => {
                            self.which_key.show();
                            self.diagnostic =
                                "Shortcuts cheatsheet (Ctrl+X or Esc to dismiss)".to_string();
                        }
                        _ => {}
                    }
                    return;
                }
            }
        }
    }
}
