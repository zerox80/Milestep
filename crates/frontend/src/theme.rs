use crate::*;

/// User-selectable visual theme. Only colors, corner radius and the body font
/// change between themes — every CSS rule keeps using the same custom-property
/// names (see `:root[data-theme="…"]` blocks in styles.css). The choice is kept
/// in `localStorage` only; there is no server-side persistence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Theme {
    Standard,
    Pishi,
}

impl Theme {
    /// Value written to the `data-theme` attribute and to `localStorage`.
    pub(crate) fn slug(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Pishi => "pishi",
        }
    }

    pub(crate) fn from_slug(slug: &str) -> Self {
        match slug {
            "pishi" => Self::Pishi,
            _ => Self::Standard,
        }
    }

    pub(crate) fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Standard, Lang::De) => "Standard",
            (Self::Standard, Lang::En) => "Standard",
            (Self::Pishi, Lang::De) => "PISHI",
            (Self::Pishi, Lang::En) => "PISHI",
        }
    }

    pub(crate) fn description(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Standard, Lang::De) => {
                "Das Original-Design von Milestep (warmes Beige, Orange)."
            }
            (Self::Standard, Lang::En) => "The original Milestep design (warm beige, orange).",
            (Self::Pishi, Lang::De) => {
                "Helles PISHI-Design (kühles Grau, Blau) – abgestimmt mit dem Team."
            }
            (Self::Pishi, Lang::En) => {
                "Light PISHI design (cool grey, blue) – aligned with the team."
            }
        }
    }

    /// Accent / background swatch colors used for the preview chips on the
    /// settings page (kept in sync with the values in styles.css).
    pub(crate) fn swatches(self) -> [&'static str; 3] {
        match self {
            Self::Standard => ["#f7f4ee", "#e8613a", "#22201c"],
            Self::Pishi => ["#eef1f5", "#0a8fd6", "#0a1120"],
        }
    }

    pub(crate) const ALL: [Self; 2] = [Self::Standard, Self::Pishi];
}

const THEME_STORAGE_KEY: &str = "milestep-theme";

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

/// Reads the persisted theme from `localStorage` (defaults to `Standard`).
pub(crate) fn load_theme() -> Theme {
    local_storage()
        .and_then(|s| s.get_item(THEME_STORAGE_KEY).ok().flatten())
        .map_or(Theme::Standard, |slug| Theme::from_slug(&slug))
}

/// Applies the theme to the document (`<html data-theme="…">`). Called once on
/// boot (reflecting the stored choice) and on every later change via a Leptos
/// effect. Persistence is handled separately by `persist_theme`.
pub(crate) fn apply_theme(theme: Theme) {
    if let Some(element) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = element.set_attribute("data-theme", theme.slug());
    }
}

/// Persists the selected theme to `localStorage`. Only called when the user
/// changes the theme — the boot value was just read back from storage.
pub(crate) fn persist_theme(theme: Theme) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(THEME_STORAGE_KEY, theme.slug());
    }
}
