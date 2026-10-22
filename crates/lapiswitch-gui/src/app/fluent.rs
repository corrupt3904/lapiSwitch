//! Windows 11 (Fluent) look: light and dark palettes, icons from the
//! system's Segoe MDL2 Assets font and the shared widget styles. The active
//! palette follows Windows or the theme chosen in Settings.
use iced::widget::{button, container, text};
use iced::{Border, Color, Font, Theme};
use std::sync::atomic::{AtomicBool, Ordering};

/// Colors of one Fluent theme.
pub(crate) struct Palette {
    /// Window and navigation background (Mica).
    pub bg: Color,
    /// Cards and dialogs.
    pub card: Color,
    pub card_hover: Color,
    pub card_border: Color,
    /// Navigation and subtle buttons.
    pub subtle_hover: Color,
    pub subtle_selected: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub text_disabled: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub on_accent: Color,
    /// Accent wash for selected cards and informational bars.
    pub accent_soft: Color,
    /// Standard (non-accent) buttons.
    pub control: Color,
    pub control_hover: Color,
    pub control_border: Color,
    pub danger: Color,
    pub danger_bg: Color,
    pub warning: Color,
    pub warning_bg: Color,
    pub success: Color,
    pub success_bg: Color,
    /// Dialog scrim over the page.
    pub scrim: Color,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

pub(crate) const LIGHT: Palette = Palette {
    bg: rgb(0xF3, 0xF3, 0xF3),
    card: rgb(0xFF, 0xFF, 0xFF),
    card_hover: rgb(0xF9, 0xF9, 0xF9),
    card_border: rgb(0xE5, 0xE5, 0xE5),
    subtle_hover: rgb(0xEA, 0xEA, 0xEA),
    subtle_selected: rgb(0xE5, 0xE5, 0xE5),
    text: rgb(0x1B, 0x1B, 0x1B),
    text_secondary: rgb(0x5F, 0x5F, 0x5F),
    text_disabled: rgb(0xA0, 0xA0, 0xA0),
    accent: rgb(0x00, 0x5F, 0xB8),
    accent_hover: rgb(0x19, 0x6E, 0xBF),
    on_accent: rgb(0xFF, 0xFF, 0xFF),
    accent_soft: rgb(0xE5, 0xF0, 0xFA),
    control: rgb(0xFB, 0xFB, 0xFB),
    control_hover: rgb(0xF4, 0xF4, 0xF4),
    control_border: rgb(0xD9, 0xD9, 0xD9),
    danger: rgb(0xC4, 0x2B, 0x1C),
    danger_bg: rgb(0xFD, 0xE7, 0xE9),
    warning: rgb(0x9D, 0x5D, 0x00),
    warning_bg: rgb(0xFF, 0xF4, 0xCE),
    success: rgb(0x0F, 0x7B, 0x0F),
    success_bg: rgb(0xDF, 0xF6, 0xDD),
    scrim: Color::from_rgba8(0, 0, 0, 0.3),
};

pub(crate) const DARK: Palette = Palette {
    bg: rgb(0x20, 0x20, 0x20),
    card: rgb(0x2B, 0x2B, 0x2B),
    card_hover: rgb(0x32, 0x32, 0x32),
    card_border: rgb(0x1C, 0x1C, 0x1C),
    subtle_hover: rgb(0x2D, 0x2D, 0x2D),
    subtle_selected: rgb(0x33, 0x33, 0x33),
    text: rgb(0xFF, 0xFF, 0xFF),
    text_secondary: rgb(0xC8, 0xC8, 0xC8),
    text_disabled: rgb(0x78, 0x78, 0x78),
    accent: rgb(0x60, 0xCD, 0xFF),
    accent_hover: rgb(0x5A, 0xBC, 0xEB),
    on_accent: rgb(0x00, 0x00, 0x00),
    accent_soft: rgb(0x1E, 0x33, 0x45),
    control: rgb(0x37, 0x37, 0x37),
    control_hover: rgb(0x3C, 0x3C, 0x3C),
    control_border: rgb(0x45, 0x45, 0x45),
    danger: rgb(0xFF, 0x99, 0xA4),
    danger_bg: rgb(0x44, 0x27, 0x26),
    warning: rgb(0xFC, 0xE1, 0x00),
    warning_bg: rgb(0x43, 0x35, 0x19),
    success: rgb(0x6C, 0xCB, 0x5F),
    success_bg: rgb(0x39, 0x3D, 0x1B),
    scrim: Color::from_rgba8(0, 0, 0, 0.5),
};

static DARK_MODE: AtomicBool = AtomicBool::new(false);

/// Called before building each view; styles read the palette when drawn.
pub(crate) fn set_dark(dark: bool) {
    DARK_MODE.store(dark, Ordering::Relaxed);
}

/// The active palette.
pub(crate) fn p() -> &'static Palette {
    if DARK_MODE.load(Ordering::Relaxed) {
        &DARK
    } else {
        &LIGHT
    }
}

/// The iced theme for default text and window colors.
pub(crate) fn iced_theme(dark: bool) -> Theme {
    let p = if dark { &DARK } else { &LIGHT };
    Theme::custom(
        if dark {
            "lapiSwitch Fluent dark"
        } else {
            "lapiSwitch Fluent light"
        },
        iced::theme::Palette {
            background: p.bg,
            text: p.text,
            primary: p.accent,
            success: p.success,
            warning: p.warning,
            danger: p.danger,
        },
    )
}

/// Windows' icon font (Windows 10 and 11).
pub(crate) const ICONS: Font = Font::with_name("Segoe MDL2 Assets");

/// Segoe MDL2 Assets code points.
pub(crate) mod glyph {
    pub const HOME: char = '\u{E80F}';
    pub const EDIT: char = '\u{E70F}';
    pub const GLOBE: char = '\u{E774}';
    pub const HISTORY: char = '\u{E81C}';
    pub const SETTINGS: char = '\u{E713}';
    pub const CHEVRON: char = '\u{E76C}';
    pub const INFO: char = '\u{E946}';
    pub const WARNING: char = '\u{E7BA}';
    pub const ERROR: char = '\u{EA39}';
    pub const COMPLETED: char = '\u{E930}';
}

pub(crate) fn icon<'a>(glyph: char) -> text::Text<'a> {
    text(glyph.to_string()).font(ICONS).size(16)
}

/// Fluent corner radii: controls 4, cards and dialogs 8.
pub(crate) const CONTROL_RADIUS: f32 = 4.0;
pub(crate) const CARD_RADIUS: f32 = 8.0;

fn disabled(mut style: button::Style, status: button::Status) -> button::Style {
    if status == button::Status::Disabled {
        style.text_color = p().text_disabled;
        if let Some(iced::Background::Color(color)) = style.background {
            style.background = Some(color.scale_alpha(0.5).into());
        }
    }
    style
}

/// Accent button: the one main action of a view.
pub(crate) fn accent_style(_: &Theme, status: button::Status) -> button::Style {
    let p = p();
    disabled(
        button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => p.accent_hover,
                    button::Status::Disabled => p.control,
                    _ => p.accent,
                }
                .into(),
            ),
            text_color: p.on_accent,
            border: Border {
                color: if status == button::Status::Disabled {
                    p.control_border
                } else {
                    p.accent
                },
                width: 1.0,
                radius: CONTROL_RADIUS.into(),
            },
            ..Default::default()
        },
        status,
    )
}

/// Standard button: secondary actions.
pub(crate) fn standard_style(_: &Theme, status: button::Status) -> button::Style {
    let p = p();
    disabled(
        button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => p.control_hover,
                    _ => p.control,
                }
                .into(),
            ),
            text_color: p.text,
            border: Border {
                color: p.control_border,
                width: 1.0,
                radius: CONTROL_RADIUS.into(),
            },
            ..Default::default()
        },
        status,
    )
}

/// Subtle button: navigation items and icon buttons.
pub(crate) fn subtle_style(_: &Theme, status: button::Status) -> button::Style {
    let p = p();
    disabled(
        button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => Some(p.subtle_hover.into()),
                _ => None,
            },
            text_color: p.text,
            border: Border {
                radius: CONTROL_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        status,
    )
}

/// Destructive action (filled).
pub(crate) fn danger_style(_: &Theme, status: button::Status) -> button::Style {
    let p = p();
    let fill = if DARK_MODE.load(Ordering::Relaxed) {
        rgb(0xE8, 0x4E, 0x40)
    } else {
        p.danger
    };
    disabled(
        button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => fill.scale_alpha(0.9),
                    button::Status::Disabled => p.control,
                    _ => fill,
                }
                .into(),
            ),
            text_color: rgb(0xFF, 0xFF, 0xFF),
            border: Border {
                color: fill,
                width: 1.0,
                radius: CONTROL_RADIUS.into(),
            },
            ..Default::default()
        },
        status,
    )
}

/// Destructive action (outlined), e.g. original restore on the home page.
pub(crate) fn danger_outline_style(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = standard_style(theme, status);
    if status != button::Status::Disabled {
        style.text_color = p().danger;
        style.border.color = p().danger;
    }
    style
}

/// A dialog that warns: card surface with a danger outline.
pub(crate) fn warning_dialog_style(_: &Theme) -> container::Style {
    let p = p();
    container::Style {
        background: Some(p.card.into()),
        text_color: Some(p.text),
        border: Border {
            color: p.danger,
            width: 1.0,
            radius: CARD_RADIUS.into(),
        },
        ..Default::default()
    }
}

/// Thin Fluent scrollbar on a transparent rail.
pub(crate) fn scroll_style(
    theme: &Theme,
    status: iced::widget::scrollable::Status,
) -> iced::widget::scrollable::Style {
    use iced::widget::scrollable;
    let p = p();
    let mut style = scrollable::default(theme, status);
    let hovered = matches!(
        status,
        scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. }
    );
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: if hovered {
                p.text_secondary
            } else {
                p.text_disabled
            }
            .into(),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
        },
    };
    style.vertical_rail = rail;
    style.horizontal_rail = rail;
    style
}

/// The thin vertical scrollbar geometry.
pub(crate) fn thin_scrollbar() -> iced::widget::scrollable::Direction {
    iced::widget::scrollable::Direction::Vertical(
        iced::widget::scrollable::Scrollbar::new()
            .width(6)
            .scroller_width(4),
    )
}

/// Card surface.
pub(crate) fn card_style(_: &Theme) -> container::Style {
    let p = p();
    container::Style {
        background: Some(p.card.into()),
        text_color: Some(p.text),
        border: Border {
            color: p.card_border,
            width: 1.0,
            radius: CARD_RADIUS.into(),
        },
        ..Default::default()
    }
}

/// Rounded outline pill: connection status and DEMO.
pub(crate) fn pill_style(_: &Theme) -> container::Style {
    let p = p();
    container::Style {
        background: Some(p.card.into()),
        border: Border {
            color: p.card_border,
            width: 1.0,
            radius: 999.0.into(),
        },
        ..Default::default()
    }
}

/// InfoBar severities.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Severity {
    Info,
    Warning,
    Error,
    Success,
}

impl Severity {
    pub(crate) fn glyph(self) -> char {
        match self {
            Self::Info => glyph::INFO,
            Self::Warning => glyph::WARNING,
            Self::Error => glyph::ERROR,
            Self::Success => glyph::COMPLETED,
        }
    }
    pub(crate) fn icon_color(self) -> Color {
        let p = p();
        match self {
            Self::Info => p.accent,
            Self::Warning => p.warning,
            Self::Error => p.danger,
            Self::Success => p.success,
        }
    }
    pub(crate) fn style(self) -> impl Fn(&Theme) -> container::Style {
        move |_| {
            let p = p();
            container::Style {
                background: Some(
                    match self {
                        Self::Info => p.card,
                        Self::Warning => p.warning_bg,
                        Self::Error => p.danger_bg,
                        Self::Success => p.success_bg,
                    }
                    .into(),
                ),
                text_color: Some(p.text),
                border: Border {
                    color: p.card_border,
                    width: 1.0,
                    radius: CARD_RADIUS.into(),
                },
                ..Default::default()
            }
        }
    }
}
