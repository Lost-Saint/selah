use iced::widget::{button, container, progress_bar, slider};
use iced::{Border, Color, Theme};

pub(crate) const APP_BACKGROUND: Color = Color::from_rgb8(12, 14, 16);
pub(crate) const MIXER_BACKGROUND: Color = Color::from_rgb8(17, 19, 21);
pub(crate) const MASTER_BACKGROUND: Color = Color::from_rgb8(22, 24, 27);
pub(crate) const STRIP_BACKGROUND: Color = Color::from_rgb8(20, 22, 24);
pub(crate) const BORDER: Color = Color::from_rgb8(48, 52, 57);
pub(crate) const TEXT: Color = Color::from_rgb8(241, 243, 245);
pub(crate) const MUTED: Color = Color::from_rgb8(170, 175, 181);
pub(crate) const SUBTLE: Color = Color::from_rgb8(125, 131, 138);
pub(crate) const ACCENT: Color = Color::from_rgb8(79, 196, 182);
pub(crate) const SUCCESS: Color = Color::from_rgb8(73, 216, 137);
pub(crate) const WARNING: Color = Color::from_rgb8(245, 184, 65);
pub(crate) const DANGER: Color = Color::from_rgb8(255, 103, 115);

pub(crate) fn page(_theme: &Theme) -> container::Style {
    container::Style::default().background(APP_BACKGROUND)
}

pub(crate) fn header(_theme: &Theme) -> container::Style {
    container::Style::default()
        .background(Color::from_rgb8(27, 29, 32))
        .border(Border::default().color(BORDER).width(1.0))
}

pub(crate) fn mixer_panel(_theme: &Theme) -> container::Style {
    container::Style::default().background(MIXER_BACKGROUND)
}

pub(crate) fn master_panel(_theme: &Theme) -> container::Style {
    container::Style::default()
        .background(MASTER_BACKGROUND)
        .border(Border::default().color(BORDER).width(1.0))
}

pub(crate) fn strip(_theme: &Theme) -> container::Style {
    container::Style::default()
        .background(STRIP_BACKGROUND)
        .border(Border::default().color(BORDER).width(1.0))
}

pub(crate) fn dark_inset(_theme: &Theme) -> container::Style {
    container::Style::default()
        .background(Color::from_rgb8(14, 16, 18))
        .border(Border::default().color(BORDER).width(1.0).rounded(6.0))
}

pub(crate) fn warning_panel(_theme: &Theme) -> container::Style {
    container::Style::default()
        .background(Color::from_rgb8(48, 37, 20))
        .border(
            Border::default()
                .color(Color::from_rgb8(108, 78, 31))
                .width(1.0)
                .rounded(8.0),
        )
}

pub(crate) fn danger_panel(_theme: &Theme) -> container::Style {
    container::Style::default()
        .background(Color::from_rgb8(48, 25, 30))
        .border(
            Border::default()
                .color(Color::from_rgb8(115, 48, 59))
                .width(1.0)
                .rounded(8.0),
        )
}

pub(crate) fn brand_mark(_theme: &Theme) -> container::Style {
    container::Style::default()
        .color(Color::from_rgb8(7, 23, 24))
        .background(ACCENT)
        .border(Border::default().rounded(5.0))
}

pub(crate) fn status_pill(_theme: &Theme) -> container::Style {
    container::Style::default()
        .color(SUCCESS)
        .background(Color::from_rgb8(22, 52, 40))
        .border(Border::default().rounded(99.0))
}

pub(crate) fn neutral_pill(_theme: &Theme) -> container::Style {
    container::Style::default()
        .color(MUTED)
        .background(Color::from_rgb8(38, 41, 45))
        .border(Border::default().rounded(99.0))
}

pub(crate) fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Active => ACCENT,
        button::Status::Hovered => Color::from_rgb8(100, 216, 202),
        button::Status::Pressed => Color::from_rgb8(61, 167, 156),
        button::Status::Disabled => Color::from_rgb8(43, 74, 72),
    };
    button::Style {
        background: Some(background.into()),
        text_color: if status == button::Status::Disabled {
            SUBTLE
        } else {
            Color::from_rgb8(7, 23, 24)
        },
        border: Border::default().rounded(6.0),
        ..button::Style::default()
    }
}

pub(crate) fn secondary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Active => Color::from_rgb8(38, 41, 45),
        button::Status::Hovered => Color::from_rgb8(51, 55, 60),
        button::Status::Pressed => Color::from_rgb8(30, 33, 36),
        button::Status::Disabled => Color::from_rgb8(28, 31, 34),
    };
    button::Style {
        background: Some(background.into()),
        text_color: if status == button::Status::Disabled {
            SUBTLE
        } else {
            TEXT
        },
        border: Border::default().color(BORDER).width(1.0).rounded(6.0),
        ..button::Style::default()
    }
}

pub(crate) fn dark_choice(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let background = if active {
            Color::from_rgb8(38, 73, 69)
        } else if status == button::Status::Hovered {
            Color::from_rgb8(47, 51, 56)
        } else {
            Color::from_rgb8(27, 29, 32)
        };
        button::Style {
            background: Some(background.into()),
            text_color: if active { ACCENT } else { MUTED },
            border: Border::default().color(BORDER).width(1.0).rounded(5.0),
            ..button::Style::default()
        }
    }
}

pub(crate) fn dark_slider(_theme: &Theme, status: slider::Status) -> slider::Style {
    let handle = match status {
        slider::Status::Active => Color::from_rgb8(217, 220, 224),
        slider::Status::Hovered | slider::Status::Dragged => ACCENT,
    };
    slider::Style {
        rail: slider::Rail {
            backgrounds: (ACCENT.into(), Color::from_rgb8(48, 52, 57).into()),
            width: 4.0,
            border: Border::default().rounded(2.0),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 9.0 },
            background: handle.into(),
            border_width: 2.0,
            border_color: Color::from_rgb8(22, 24, 27),
        },
    }
}

pub(crate) fn fader_slider(_theme: &Theme, status: slider::Status) -> slider::Style {
    let handle = match status {
        slider::Status::Active => Color::from_rgb8(75, 79, 85),
        slider::Status::Hovered | slider::Status::Dragged => Color::from_rgb8(102, 107, 114),
    };
    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Color::from_rgb8(75, 79, 85).into(),
                Color::from_rgb8(7, 8, 9).into(),
            ),
            width: 4.0,
            border: Border::default().rounded(2.0),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Rectangle {
                width: 30,
                border_radius: 4.0.into(),
            },
            background: handle.into(),
            border_width: 1.0,
            border_color: Color::from_rgb8(103, 108, 114),
        },
    }
}

pub(crate) fn meter(_theme: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: Color::from_rgb8(5, 6, 7).into(),
        bar: SUCCESS.into(),
        border: Border::default()
            .color(Color::from_rgb8(31, 34, 37))
            .width(1.0)
            .rounded(2.0),
    }
}
