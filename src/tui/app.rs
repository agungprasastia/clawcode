#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Input {
    Quit,
    Character(char),
    Backspace,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UiEvent {
    Input(Input),
    Resize { width: u16, height: u16 },
    StreamDelta(String),
}

#[derive(Debug)]
pub struct App {
    running: bool,
    viewport: (u16, u16),
    prompt: String,
    transcript: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            running: true,
            viewport: (0, 0),
            prompt: String::new(),
            transcript: String::new(),
        }
    }
}

impl App {
    pub fn apply(&mut self, event: UiEvent) {
        match event {
            UiEvent::Input(Input::Quit) => self.running = false,
            UiEvent::Input(Input::Character(character)) => self.prompt.push(character),
            UiEvent::Input(Input::Backspace) => {
                self.prompt.pop();
            }
            UiEvent::Resize { width, height } => self.viewport = (width, height),
            UiEvent::StreamDelta(delta) => self.transcript.push_str(&delta),
        }
    }

    pub fn apply_batch(&mut self, events: impl IntoIterator<Item = UiEvent>) {
        for event in events {
            self.apply(event);
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn viewport(&self) -> (u16, u16) {
        self.viewport
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn transcript(&self) -> &str {
        &self.transcript
    }
}
