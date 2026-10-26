//! Design tokens: the semantic palette, glyph set and spacing scale every screen
//! draws with. Nothing outside this module names a concrete color.
//!
//! Hues come only from the 16 ANSI colors, so the user's terminal palette picks
//! the actual shades and both light and dark themes stay legible. Filled
//! surfaces (active tab, chips) use `REVERSED` on a foreground hue instead of a
//! fixed background, which makes their text take the terminal's own background
//! color and keeps contrast correct on either theme.
//!
//! With `NO_COLOR` set, every token drops its hue and keeps only modifiers.
//! crossterm renders a suppressed color change as a bare `ESC[m`, which would
//! also reset bold/reverse set earlier in the same cell, so state must never
//! depend on a color being emitted.

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::BorderType;

use crate::tui::model::bucket_of;
use arca_core::domain::types::RegistryEntry;

/// Same rule crossterm uses: a non-empty `NO_COLOR` disables color.
pub fn color_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()))
}

/// Semantic roles. Screens pick a role; the role picks the hue.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Focus, active navigation, interactive affordances.
    Accent,
    /// The Arca wordmark and HTTP transport.
    Brand,
    /// Enabled, installed, local origin, additive actions.
    Success,
    /// Customized, managed, manual origin, recoverable problems.
    Warning,
    /// Destructive actions and failures.
    Danger,
    /// Counts, stdio transport, informational badges.
    Info,
    /// Secondary text, hints, borders.
    Muted,
}

impl Tone {
    fn hue(self) -> Option<Color> {
        match self {
            Tone::Accent => Some(Color::Cyan),
            Tone::Brand => Some(Color::Magenta),
            Tone::Success => Some(Color::Green),
            Tone::Warning => Some(Color::Yellow),
            Tone::Danger => Some(Color::Red),
            Tone::Info => Some(Color::Blue),
            Tone::Muted => None,
        }
    }
}

/// Foreground in `tone`. `Muted` is the dimmed default foreground, which tracks
/// the terminal theme better than any fixed gray.
pub fn fg(tone: Tone) -> Style {
    match (tone.hue(), color_enabled()) {
        (None, _) => Style::new().add_modifier(Modifier::DIM),
        (Some(c), true) => Style::new().fg(c),
        (Some(_), false) => Style::new(),
    }
}

/// A filled block of `tone` (text takes the terminal background).
pub fn chip(tone: Tone) -> Style {
    let base = match (tone.hue(), color_enabled()) {
        (Some(c), true) => Style::new().fg(c),
        _ => Style::new(),
    };
    base.add_modifier(Modifier::REVERSED | Modifier::BOLD)
}

pub fn text() -> Style {
    Style::new()
}

pub fn strong() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

pub fn muted() -> Style {
    fg(Tone::Muted)
}

/// A key in a shortcut hint.
pub fn key() -> Style {
    fg(Tone::Accent).add_modifier(Modifier::BOLD)
}

/// A selected row's primary label.
pub fn selected(focused: bool) -> Style {
    if focused {
        fg(Tone::Accent).add_modifier(Modifier::BOLD)
    } else {
        strong()
    }
}

pub fn border(focused: bool) -> Style {
    if focused {
        fg(Tone::Accent)
    } else {
        muted()
    }
}

pub fn link() -> Style {
    fg(Tone::Accent).add_modifier(Modifier::UNDERLINED)
}

/// Every panel and dialog uses the same corner style.
pub const BORDER: BorderType = BorderType::Rounded;

/// Horizontal gap between inline groups (cells).
pub const GAP: &str = "  ";

/// Glyphs, restricted to characters that are single-width in common terminal
/// fonts. Emoji are avoided: their width varies by terminal and font.
pub mod glyph {
    /// Selection gutter in the focused pane / dimmed in an unfocused pane.
    pub const GUTTER: &str = "▌ ";
    pub const GUTTER_IDLE: &str = "▏ ";
    pub const NO_GUTTER: &str = "  ";
    pub const ON: &str = "●";
    pub const OFF: &str = "○";
    pub const PICKED: &str = "◉";
    pub const OK: &str = "✓";
    pub const FAIL: &str = "✗";
    pub const EDITED: &str = "✎";
    pub const WARN: &str = "!";
    pub const CARET: &str = "▏";
    pub const DOT: &str = "·";
    pub const EMPTY: &str = "—";
    pub const PROMPT: &str = "›";
    pub const ARROW: &str = "→";
    pub const SCROLL_TRACK: &str = "│";
    pub const SCROLL_THUMB: &str = "┃";
}

/// The transport label, colored but unfilled so it reads as metadata.
pub fn transport_tone(transport: &str) -> Tone {
    if transport == "stdio" {
        Tone::Info
    } else {
        Tone::Brand
    }
}

pub fn transport_span(transport: &str) -> Span<'static> {
    Span::styled(transport.to_string(), fg(transport_tone(transport)))
}

/// Provenance label and tone for a catalog entry's origin bucket.
pub fn origin(entry: &RegistryEntry) -> (&'static str, Tone) {
    match bucket_of(entry) {
        "remote" => ("订阅", Tone::Accent),
        "local" => ("本地", Tone::Success),
        "manual" => ("手动", Tone::Warning),
        _ => ("探索", Tone::Muted),
    }
}
