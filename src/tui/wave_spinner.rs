use ratatui::style::{Color, Style};
use ratatui::text::Span;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct WaveSpinner {
    frames: Vec<Vec<Span<'static>>>,
    animation_origin: Instant,
    base_color: Color,
    frame_duration: Duration,
}

impl Default for WaveSpinner {
    fn default() -> Self {
        Self::new(Color::Rgb(255, 165, 0))
    }
}

impl WaveSpinner {
    pub const WIDTH: u16 = 8;

    const DEFAULT_FRAME_DURATION: Duration = Duration::from_millis(50);
    const OPACITIES: [f32; 5] = [1.0, 0.8, 0.6, 0.4, 0.2];
    const COMPACT_FRAMES: [&'static str; 10] = ["⠋", "⠉", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇"];

    pub fn new(base_color: Color) -> Self {
        let frames = Self::generate_frames(base_color);
        Self {
            frames,
            animation_origin: Instant::now(),
            base_color,
            frame_duration: Self::DEFAULT_FRAME_DURATION,
        }
    }

    pub fn with_speed(base_color: Color, frame_duration_ms: u64) -> Self {
        let mut spinner = Self::new(base_color);
        spinner.set_speed(frame_duration_ms);
        spinner
    }

    pub fn set_speed(&mut self, frame_duration_ms: u64) {
        self.frame_duration = Duration::from_millis(frame_duration_ms.max(1));
        self.animation_origin = Instant::now();
    }

    /// Advances the spinner animation. Since frames are determined by wall-clock
    /// duration, tick() is a lightweight hook for callers in tick loops.
    pub fn tick(&mut self) {
        // Wall clock drives current_frame()
    }

    pub fn current_frame(&self) -> usize {
        Self::frame_for_elapsed(
            self.animation_origin.elapsed(),
            self.frame_duration,
            self.frames.len(),
        )
    }

    pub fn compact_frame(&self) -> &'static str {
        Self::COMPACT_FRAMES[self.current_frame() % Self::COMPACT_FRAMES.len()]
    }

    fn frame_for_elapsed(elapsed: Duration, frame_duration: Duration, frame_count: usize) -> usize {
        if frame_count == 0 || frame_duration.is_zero() {
            return 0;
        }

        ((elapsed.as_nanos() / frame_duration.as_nanos()) % frame_count as u128) as usize
    }

    pub fn spans(&self) -> Vec<Span<'static>> {
        self.frames[self.current_frame()].clone()
    }

    pub fn spans_for_width(&self, width: u16) -> Vec<Span<'static>> {
        if width == 0 {
            Vec::new()
        } else if width < Self::WIDTH {
            vec![Span::styled(
                Self::COMPACT_FRAMES[self.current_frame() % Self::COMPACT_FRAMES.len()],
                Style::default().fg(self.base_color),
            )]
        } else {
            self.spans()
        }
    }

    pub fn set_color(&mut self, base_color: Color) {
        if self.base_color != base_color {
            self.base_color = base_color;
            self.frames = Self::generate_frames(base_color);
        }
    }

    fn generate_frames(base_color: Color) -> Vec<Vec<Span<'static>>> {
        vec![
            // Moving right (frames 0-12) - 5 block wave
            Self::create_frame(&[(0, 0)], base_color),
            Self::create_frame(&[(0, 1), (1, 0)], base_color),
            Self::create_frame(&[(0, 2), (1, 1), (2, 0)], base_color),
            Self::create_frame(&[(0, 3), (1, 2), (2, 1), (3, 0)], base_color),
            Self::create_frame(&[(0, 4), (1, 3), (2, 2), (3, 1), (4, 0)], base_color),
            Self::create_frame(&[(1, 4), (2, 3), (3, 2), (4, 1), (5, 0)], base_color),
            Self::create_frame(&[(2, 4), (3, 3), (4, 2), (5, 1), (6, 0)], base_color),
            Self::create_frame(&[(3, 4), (4, 3), (5, 2), (6, 1), (7, 0)], base_color),
            Self::create_frame(&[(4, 3), (5, 2), (6, 1), (7, 0)], base_color),
            Self::create_frame(&[(5, 2), (6, 1), (7, 0)], base_color),
            Self::create_frame(&[(6, 1), (7, 0)], base_color),
            Self::create_frame(&[(7, 0)], base_color),
            // PAUSE: Hold empty before bouncing back
            Self::create_empty_frame(),
            Self::create_empty_frame(),
            Self::create_empty_frame(),
            // Moving left (frames 15-26) - fade direction reverses
            Self::create_frame(&[(7, 0)], base_color),
            Self::create_frame(&[(6, 1), (7, 0)], base_color),
            Self::create_frame(&[(5, 2), (6, 1), (7, 0)], base_color),
            Self::create_frame(&[(4, 3), (5, 2), (6, 1), (7, 0)], base_color),
            Self::create_frame(&[(3, 0), (4, 1), (5, 2), (6, 3), (7, 4)], base_color),
            Self::create_frame(&[(2, 0), (3, 1), (4, 2), (5, 3), (6, 4)], base_color),
            Self::create_frame(&[(1, 0), (2, 1), (3, 2), (4, 3), (5, 4)], base_color),
            Self::create_frame(&[(0, 0), (1, 1), (2, 2), (3, 3), (4, 4)], base_color),
            Self::create_frame(&[(0, 0), (1, 1), (2, 2), (3, 3)], base_color),
            Self::create_frame(&[(0, 0), (1, 1), (2, 2)], base_color),
            Self::create_frame(&[(0, 0), (1, 1)], base_color),
            // PAUSE: Hold empty before looping
            Self::create_empty_frame(),
            Self::create_empty_frame(),
            Self::create_empty_frame(),
            Self::create_empty_frame(),
        ]
    }

    fn create_frame(positions: &[(usize, usize)], base_color: Color) -> Vec<Span<'static>> {
        let mut chars: Vec<Span<'static>> = Vec::with_capacity(8);

        for i in 0..8 {
            if let Some((_, opacity_idx)) = positions.iter().find(|(pos, _)| *pos == i) {
                let opacity = Self::OPACITIES[*opacity_idx];
                let color = Self::apply_opacity(base_color, opacity);
                chars.push(Span::styled("■", Style::default().fg(color)));
            } else {
                chars.push(Span::styled("⬝", Style::default().fg(Color::DarkGray)));
            }
        }

        chars
    }

    fn create_empty_frame() -> Vec<Span<'static>> {
        (0..8)
            .map(|_| Span::styled("⬝", Style::default().fg(Color::DarkGray)))
            .collect()
    }

    fn apply_opacity(color: Color, opacity: f32) -> Color {
        match color {
            Color::Rgb(r, g, b) => Color::Rgb(
                (r as f32 * opacity) as u8,
                (g as f32 * opacity) as u8,
                (b as f32 * opacity) as u8,
            ),
            _ => color,
        }
    }
}
