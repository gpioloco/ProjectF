use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type Rgb = [u8; 3];
pub fn hex([r, g, b]: Rgb) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum Photo {
    Canopy,
    Sienna,
    BlueHour,
}

impl Photo {
    pub const ALL: [Self; 3] = [Self::Canopy, Self::Sienna, Self::BlueHour];
    pub fn bytes(self) -> &'static [u8] {
        match self {
            Self::Canopy => include_bytes!("../assets/wallpapers/canopy.jpg"),
            Self::Sienna => include_bytes!("../assets/wallpapers/sienna.jpg"),
            Self::BlueHour => include_bytes!("../assets/wallpapers/blue-hour.jpg"),
        }
    }
    pub fn credit(self) -> &'static str {
        match self {
            Self::Canopy => "Luca Bravo",
            Self::Sienna => "Bernd Dittrich",
            Self::BlueHour => "Felix Rottmann",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Canopy => "Canopy",
            Self::Sienna => "Sienna",
            Self::BlueHour => "Blue Hour",
        }
    }
}

#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum Wallpaper {
    Bundled(Photo),
    Local(PathBuf),
}

#[derive(Clone, Copy, Debug, Default, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct Effects {
    pub blur: u8,
    pub rain: u8,
    pub grain: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub background: Rgb,
    pub surface: Rgb,
    pub text: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub secondary: Rgb,
}
impl Palette {
    pub fn colors(&self) -> [Rgb; 6] {
        [
            self.background,
            self.surface,
            self.muted,
            self.accent,
            self.secondary,
            self.text,
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub wallpaper: Wallpaper,
    pub palette: Palette,
    pub effects: Effects,
    pub desktop: bool,
    pub vscode: bool,
    pub nvim: bool,
    pub glaze: bool,
    pub code_theme: String,
    pub vim_scheme: String,
    pub focused: Option<Rgb>,
    pub unfocused: Option<Rgb>,
}

impl Theme {
    pub fn preset(photo: Photo) -> Self {
        let (id, colors) = match photo {
            Photo::Canopy => (
                "canopy",
                [0x121B16, 0x27372E, 0xE6EBDA, 0x95A699, 0xA8C588, 0xD4C39C],
            ),
            Photo::Sienna => (
                "sienna",
                [0x221916, 0x402A24, 0xF4E7D6, 0xB59D8F, 0xDE9569, 0xE5C59B],
            ),
            Photo::BlueHour => (
                "blue-hour",
                [0x111B32, 0x243455, 0xE3EAF9, 0x96A7C7, 0x90B6F6, 0xBBB5E4],
            ),
        };
        let [background, surface, text, muted, accent, secondary] =
            colors.map(|c: u32| [(c >> 16) as u8, (c >> 8) as u8, c as u8]);
        Self {
            id: id.into(),
            name: photo.title().into(),
            wallpaper: Wallpaper::Bundled(photo),
            palette: Palette {
                background,
                surface,
                text,
                muted,
                accent,
                secondary,
            },
            effects: Effects::default(),
            desktop: true,
            vscode: true,
            nvim: true,
            glaze: true,
            code_theme: String::new(),
            vim_scheme: String::new(),
            focused: None,
            unfocused: None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty()
                && self.name.chars().count() <= 64
                && !self.name.chars().any(char::is_control),
            "Use a theme name between 1 and 64 characters on one line."
        );
        ensure!(
            self.vim_scheme.len() <= 120
                && self
                    .vim_scheme
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-.".contains(&c))
                && !self.vim_scheme.starts_with('.'),
            "Use a Neovim colorscheme name containing letters, numbers, dots, underscores or hyphens."
        );
        ensure!(
            self.code_theme.len() <= 200 && !self.code_theme.chars().any(char::is_control),
            "Enter a single-line VS Code theme name."
        );
        ensure!(
            [self.effects.blur, self.effects.rain, self.effects.grain]
                .into_iter()
                .all(|v| v <= 100),
            "Effect strength must be between 0 and 100."
        );
        ensure!(
            self.desktop || self.vscode || self.nvim || self.glaze,
            "Select at least one component."
        );
        Ok(())
    }
}
