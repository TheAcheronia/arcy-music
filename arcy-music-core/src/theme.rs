use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThemeColors {
    pub name: String,
    pub display_name: String,
    pub bg_base: String,
    pub bg_surface: String,
    pub bg_overlay: String,
    pub card_bg: String,
    pub accent: String,
    pub accent_secondary: String,
    pub accent_pink: String,
    pub accent_green: String,
    pub accent_yellow: String,
    pub accent_red: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub text_muted: String,
    pub border: String,
}

impl ThemeColors {
    pub fn mocha() -> Self {
        Self {
            name: "mocha".into(),
            display_name: "Catppuccin Mocha".into(),
            bg_base: "#1e1e2e".into(),
            bg_surface: "#181825".into(),
            bg_overlay: "#11111b".into(),
            card_bg: "#313244".into(),
            accent: "#89b4fa".into(),
            accent_secondary: "#cba6f7".into(),
            accent_pink: "#f5c2e7".into(),
            accent_green: "#a6e3a1".into(),
            accent_yellow: "#f9e2af".into(),
            accent_red: "#f38ba8".into(),
            text_primary: "#cdd6f4".into(),
            text_secondary: "#bac2de".into(),
            text_muted: "#a6adc8".into(),
            border: "#45475a".into(),
        }
    }

    pub fn nord() -> Self {
        Self {
            name: "nord".into(),
            display_name: "Nord Arctic".into(),
            bg_base: "#2e3440".into(),
            bg_surface: "#242933".into(),
            bg_overlay: "#1e222a".into(),
            card_bg: "#3b4252".into(),
            accent: "#88c0d0".into(),
            accent_secondary: "#81a1c1".into(),
            accent_pink: "#b48ead".into(),
            accent_green: "#a3be8c".into(),
            accent_yellow: "#ebcb8b".into(),
            accent_red: "#bf616a".into(),
            text_primary: "#eceff4".into(),
            text_secondary: "#e5e9f0".into(),
            text_muted: "#d8dee9".into(),
            border: "#4c566a".into(),
        }
    }

    pub fn tokyo() -> Self {
        Self {
            name: "tokyo".into(),
            display_name: "Tokyo Night".into(),
            bg_base: "#1a1b26".into(),
            bg_surface: "#16161e".into(),
            bg_overlay: "#0f0f14".into(),
            card_bg: "#24283b".into(),
            accent: "#7aa2f7".into(),
            accent_secondary: "#bb9af7".into(),
            accent_pink: "#f7768e".into(),
            accent_green: "#9ece6a".into(),
            accent_yellow: "#e0af68".into(),
            accent_red: "#f7768e".into(),
            text_primary: "#c0caf5".into(),
            text_secondary: "#a9b1d6".into(),
            text_muted: "#565f89".into(),
            border: "#292e42".into(),
        }
    }

    pub fn cyberpunk() -> Self {
        Self {
            name: "cyberpunk".into(),
            display_name: "Cyberpunk Neon".into(),
            bg_base: "#0d0f18".into(),
            bg_surface: "#05070c".into(),
            bg_overlay: "#020306".into(),
            card_bg: "#1a1d2e".into(),
            accent: "#00f0ff".into(),
            accent_secondary: "#ff007f".into(),
            accent_pink: "#ff007f".into(),
            accent_green: "#00ff66".into(),
            accent_yellow: "#ffe600".into(),
            accent_red: "#ff0055".into(),
            text_primary: "#ffffff".into(),
            text_secondary: "#00f0ff".into(),
            text_muted: "#707590".into(),
            border: "#2a2d45".into(),
        }
    }

    pub fn dracula() -> Self {
        Self {
            name: "dracula".into(),
            display_name: "Dracula Dark".into(),
            bg_base: "#282a36".into(),
            bg_surface: "#1e1f29".into(),
            bg_overlay: "#14151b".into(),
            card_bg: "#44475a".into(),
            accent: "#bd93f9".into(),
            accent_secondary: "#ff79c6".into(),
            accent_pink: "#ff79c6".into(),
            accent_green: "#50fa7b".into(),
            accent_yellow: "#f1fa8c".into(),
            accent_red: "#ff5555".into(),
            text_primary: "#f8f8f2".into(),
            text_secondary: "#f8f8f2".into(),
            text_muted: "#6272a4".into(),
            border: "#6272a4".into(),
        }
    }

    pub fn gruvbox() -> Self {
        Self {
            name: "gruvbox".into(),
            display_name: "Gruvbox Slate".into(),
            bg_base: "#282828".into(),
            bg_surface: "#1d2021".into(),
            bg_overlay: "#141617".into(),
            card_bg: "#3c3836".into(),
            accent: "#8ec07c".into(),
            accent_secondary: "#fabd2f".into(),
            accent_pink: "#d3869b".into(),
            accent_green: "#b8bb26".into(),
            accent_yellow: "#fabd2f".into(),
            accent_red: "#fb4934".into(),
            text_primary: "#ebdbb2".into(),
            text_secondary: "#d5c4a1".into(),
            text_muted: "#928374".into(),
            border: "#504945".into(),
        }
    }

    pub fn get_by_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "nord" => Self::nord(),
            "tokyo" | "tokyonight" | "tokyo_night" => Self::tokyo(),
            "cyberpunk" | "cyber" | "neon" => Self::cyberpunk(),
            "dracula" => Self::dracula(),
            "gruvbox" => Self::gruvbox(),
            _ => Self::mocha(),
        }
    }

    pub fn all() -> Vec<Self> {
        vec![
            Self::mocha(),
            Self::nord(),
            Self::tokyo(),
            Self::cyberpunk(),
            Self::dracula(),
            Self::gruvbox(),
        ]
    }
}
