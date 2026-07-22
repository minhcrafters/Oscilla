use iced_code_editor::theme;
use nice_plug_iced::iced::{
    Background, Border, Color, Font, Length, Shadow, Vector, border, font,
    widget::{button, container, pick_list, text},
};

pub const BG_DEEP: Color = Color::from_rgb(0.118, 0.118, 0.118);
pub const SURFACE: Color = Color::from_rgb(0.145, 0.145, 0.149);
pub const BORDER: Color = Color::from_rgb(0.243, 0.243, 0.259);
pub const ACCENT: Color = Color::from_rgb(0.0, 0.478, 0.8);
pub const ACCENT_SOFT: Color = Color::from_rgba(0.0, 0.478, 0.8, 0.12);
pub const ACCENT_GLOW: Color = Color::from_rgba(0.0, 0.478, 0.8, 0.22);
pub const ACCENT_LINE: Color = Color::from_rgba(0.0, 0.478, 0.8, 0.15);
pub const FG_DIM: Color = Color::from_rgb(0.522, 0.522, 0.522);
pub const FG_TEXT: Color = Color::from_rgb(0.831, 0.831, 0.831);
pub const GREEN: Color = Color::from_rgb(0.306, 0.788, 0.690);
pub const RED: Color = Color::from_rgb(0.945, 0.298, 0.298);
pub const YELLOW: Color = Color::from_rgb(0.875, 0.808, 0.0);

pub fn vscode_editor_style() -> theme::Style {
    theme::Style {
        background: BG_DEEP,
        text_color: FG_TEXT,
        gutter_background: BG_DEEP,
        gutter_border: BORDER,
        line_number_color: FG_DIM,
        scrollbar_background: BG_DEEP,
        scroller_color: BORDER,
        current_line_highlight: Color::from_rgba(1.0, 1.0, 1.0, 0.04),
    }
}

pub fn heading(label: &str) -> text::Text<'_> {
    text(label).size(10).color(FG_DIM).font(Font {
        weight: font::Weight::Semibold,
        ..Font::DEFAULT
    })
}

pub fn knob_label(label: &str) -> text::Text<'_> {
    use nice_plug_iced::iced::Center;
    text(label)
        .size(10)
        .color(FG_DIM)
        .font(Font {
            weight: font::Weight::Normal,
            ..Font::DEFAULT
        })
        .align_x(Center)
        .width(Length::Fill)
}

pub fn value_label(val: String) -> text::Text<'static> {
    use nice_plug_iced::iced::Center;
    text(val)
        .size(10)
        .color(ACCENT)
        .font(Font {
            weight: font::Weight::Semibold,
            ..Font::MONOSPACE
        })
        .align_x(Center)
        .width(Length::Fill)
}

pub fn btn_text(label: &str) -> text::Text<'_> {
    text(label).size(11).color(Color::WHITE).font(Font {
        weight: font::Weight::Semibold,
        ..Font::DEFAULT
    })
}

pub fn status_text(label: &str, color: Color) -> text::Text<'_> {
    text(label).size(11).color(color).font(Font {
        weight: font::Weight::Normal,
        ..Font::MONOSPACE
    })
}

pub fn peak_text(val: String) -> text::Text<'static> {
    text(val).size(11).color(ACCENT).font(Font {
        weight: font::Weight::Bold,
        ..Font::MONOSPACE
    })
}

pub fn rad(r: f32) -> border::Radius {
    border::radius(r)
}

pub fn section_panel() -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: rad(4.0),
        },
        shadow: Shadow::default(),
        text_color: None,
        snap: false,
    }
}

pub fn btn_style(hovered: bool) -> button::Style {
    let border_alpha = if hovered { 0.6 } else { 0.3 };
    let shadow = if hovered {
        Shadow {
            color: ACCENT_SOFT,
            offset: Vector::new(0.0, 1.0),
            blur_radius: 6.0,
        }
    } else {
        Shadow::default()
    };
    button::Style {
        background: Some(Background::Color(ACCENT)),
        border: Border {
            color: Color::from_rgba(0.0, 0.478, 0.8, border_alpha),
            width: 1.0,
            radius: rad(3.0),
        },
        text_color: Color::WHITE,
        shadow,
        snap: false,
    }
}

pub fn picklist_style() -> pick_list::Style {
    pick_list::Style {
        text_color: FG_TEXT,
        placeholder_color: FG_DIM,
        handle_color: FG_DIM,
        background: Background::Color(SURFACE),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: rad(4.0),
        },
    }
}
