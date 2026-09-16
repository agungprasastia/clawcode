use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeKind {
    #[default]
    ClawcodeDark,
    CrabcodeOrange,
    CatppuccinMocha,
    Dracula,
    Nord,
    GruvboxDark,
    TokyoNight,
    Monokai,
}

impl ThemeKind {
    pub const ALL: [ThemeKind; 8] = [
        ThemeKind::ClawcodeDark,
        ThemeKind::CrabcodeOrange,
        ThemeKind::CatppuccinMocha,
        ThemeKind::Dracula,
        ThemeKind::Nord,
        ThemeKind::GruvboxDark,
        ThemeKind::TokyoNight,
        ThemeKind::Monokai,
    ];

    pub fn id(&self) -> &'static str {
        match self {
            Self::ClawcodeDark => "clawcode-dark",
            Self::CrabcodeOrange => "crabcode-orange",
            Self::CatppuccinMocha => "catppuccin",
            Self::Dracula => "dracula",
            Self::Nord => "nord",
            Self::GruvboxDark => "gruvbox",
            Self::TokyoNight => "tokyo-night",
            Self::Monokai => "monokai",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::ClawcodeDark => "Clawcode Dark",
            Self::CrabcodeOrange => "Crabcode Orange",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::Dracula => "Dracula",
            Self::Nord => "Nord",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::TokyoNight => "Tokyo Night",
            Self::Monokai => "Monokai",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::ClawcodeDark => "Amber & teal accents on deep charcoal",
            Self::CrabcodeOrange => "Warm fiery orange & coral sunset",
            Self::CatppuccinMocha => "Pastel mauve, blue & mocha surface",
            Self::Dracula => "Vibrant purple, pink & cyan gothic",
            Self::Nord => "Arctic frost blue, teal & polar night",
            Self::GruvboxDark => "Retro warm yellow, aqua & earthy brown",
            Self::TokyoNight => "Midnight blue, neon cyan & magenta",
            Self::Monokai => "Vivid yellow, green & retro charcoal",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let q = name.trim().to_lowercase();
        Self::ALL.into_iter().find(|t| {
            t.id().to_lowercase() == q
                || t.name().to_lowercase() == q
                || t.id().contains(&q)
                || t.name().to_lowercase().contains(&q)
        })
    }

    pub fn to_theme(&self) -> Theme {
        match self {
            Self::ClawcodeDark => Theme {
                bg_element: Color::Rgb(24, 28, 30),
                ink: Color::Rgb(240, 238, 233),
                quiet: Color::Rgb(140, 148, 144),
                dim: Color::Rgb(85, 95, 92),
                amber: Color::Rgb(232, 181, 84),
                teal: Color::Rgb(102, 190, 174),
                panel: Color::Rgb(48, 56, 60),
                success: Color::Rgb(74, 222, 128),
                error: Color::Rgb(248, 113, 113),
                warning: Color::Rgb(251, 191, 36),
            },
            Self::CrabcodeOrange => Theme {
                bg_element: Color::Rgb(28, 22, 20),
                ink: Color::Rgb(245, 235, 230),
                quiet: Color::Rgb(165, 145, 135),
                dim: Color::Rgb(100, 85, 80),
                amber: Color::Rgb(255, 120, 50),
                teal: Color::Rgb(255, 180, 80),
                panel: Color::Rgb(55, 42, 38),
                success: Color::Rgb(130, 215, 110),
                error: Color::Rgb(255, 90, 90),
                warning: Color::Rgb(255, 180, 50),
            },
            Self::CatppuccinMocha => Theme {
                bg_element: Color::Rgb(30, 30, 46),
                ink: Color::Rgb(205, 214, 244),
                quiet: Color::Rgb(166, 173, 200),
                dim: Color::Rgb(108, 112, 134),
                amber: Color::Rgb(203, 166, 247),
                teal: Color::Rgb(137, 180, 250),
                panel: Color::Rgb(49, 50, 68),
                success: Color::Rgb(166, 227, 161),
                error: Color::Rgb(243, 139, 168),
                warning: Color::Rgb(249, 226, 175),
            },
            Self::Dracula => Theme {
                bg_element: Color::Rgb(40, 42, 54),
                ink: Color::Rgb(248, 248, 242),
                quiet: Color::Rgb(189, 147, 249),
                dim: Color::Rgb(98, 114, 164),
                amber: Color::Rgb(255, 121, 198),
                teal: Color::Rgb(139, 233, 253),
                panel: Color::Rgb(68, 71, 90),
                success: Color::Rgb(80, 250, 123),
                error: Color::Rgb(255, 85, 85),
                warning: Color::Rgb(241, 250, 140),
            },
            Self::Nord => Theme {
                bg_element: Color::Rgb(46, 52, 64),
                ink: Color::Rgb(236, 239, 244),
                quiet: Color::Rgb(216, 222, 233),
                dim: Color::Rgb(76, 86, 106),
                amber: Color::Rgb(136, 192, 208),
                teal: Color::Rgb(129, 161, 193),
                panel: Color::Rgb(59, 66, 82),
                success: Color::Rgb(163, 190, 140),
                error: Color::Rgb(191, 97, 106),
                warning: Color::Rgb(235, 203, 139),
            },
            Self::GruvboxDark => Theme {
                bg_element: Color::Rgb(40, 40, 40),
                ink: Color::Rgb(235, 219, 178),
                quiet: Color::Rgb(168, 153, 132),
                dim: Color::Rgb(124, 111, 100),
                amber: Color::Rgb(250, 189, 47),
                teal: Color::Rgb(142, 192, 124),
                panel: Color::Rgb(60, 56, 54),
                success: Color::Rgb(184, 187, 38),
                error: Color::Rgb(251, 73, 52),
                warning: Color::Rgb(254, 128, 25),
            },
            Self::TokyoNight => Theme {
                bg_element: Color::Rgb(26, 27, 38),
                ink: Color::Rgb(192, 202, 245),
                quiet: Color::Rgb(122, 162, 247),
                dim: Color::Rgb(86, 95, 137),
                amber: Color::Rgb(224, 175, 104),
                teal: Color::Rgb(125, 207, 255),
                panel: Color::Rgb(36, 40, 59),
                success: Color::Rgb(158, 206, 106),
                error: Color::Rgb(247, 118, 142),
                warning: Color::Rgb(224, 175, 104),
            },
            Self::Monokai => Theme {
                bg_element: Color::Rgb(39, 40, 34),
                ink: Color::Rgb(248, 248, 242),
                quiet: Color::Rgb(142, 144, 130),
                dim: Color::Rgb(117, 113, 94),
                amber: Color::Rgb(230, 219, 116),
                teal: Color::Rgb(102, 217, 239),
                panel: Color::Rgb(62, 61, 50),
                success: Color::Rgb(166, 226, 46),
                error: Color::Rgb(249, 38, 114),
                warning: Color::Rgb(253, 151, 31),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub bg_element: Color,
    pub ink: Color,
    pub quiet: Color,
    pub dim: Color,
    pub amber: Color,
    pub teal: Color,
    pub panel: Color,
    pub success: Color,
    pub error: Color,
    pub warning: Color,
}

impl Theme {
    pub const fn new() -> Self {
        Self {
            bg_element: Color::Rgb(24, 28, 30),
            ink: Color::Rgb(240, 238, 233),
            quiet: Color::Rgb(140, 148, 144),
            dim: Color::Rgb(85, 95, 92),
            amber: Color::Rgb(232, 181, 84),
            teal: Color::Rgb(102, 190, 174),
            panel: Color::Rgb(48, 56, 60),
            success: Color::Rgb(74, 222, 128),
            error: Color::Rgb(248, 113, 113),
            warning: Color::Rgb(251, 191, 36),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::new()
    }
}

pub fn darken_color(color: Color, factor: f32) -> Color {
    match color {
        Color::Rgb(r, g, b) => {
            let r = (r as f32 * factor).clamp(0.0, 255.0) as u8;
            let g = (g as f32 * factor).clamp(0.0, 255.0) as u8;
            let b = (b as f32 * factor).clamp(0.0, 255.0) as u8;
            Color::Rgb(r, g, b)
        }
        _ => color,
    }
}
