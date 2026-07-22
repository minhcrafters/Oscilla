use iced_audio::{knob, text_marks};
use nice_plug_iced::iced::widget::canvas::LineCap;

use super::theme::{ACCENT, BG_DEEP, BORDER, SURFACE};

mod colors {
    use super::*;
    use nice_plug_iced::iced::Color;

    pub const KNOB_BG: Color = SURFACE;
    pub const KNOB_BORDER: Color = BORDER;
    pub const HANDLE: Color = ACCENT;
    pub const FILLED: Color = Color::from_rgba(0.0, 0.478, 0.8, 0.7);
    pub const HANDLE_HOVER: Color = Color::from_rgb(0.2, 0.6, 0.95);
    pub const FILLED_HOVER: Color = Color::from_rgba(0.2, 0.6, 0.95, 0.8);
    pub const ARC_EMPTY: Color = BG_DEEP;
    pub const ARC_FILLED: Color = ACCENT;
    pub const ARC_RIGHT: Color = Color::from_rgb(0.0, 0.6, 0.9);
}

// Circle knob with circular notch

pub struct CircleKnob;

impl knob::StyleSheet for CircleKnob {
    type Style = nice_plug_iced::iced::Theme;

    fn idle(&self, _style: &Self::Style) -> knob::Appearance {
        knob::Appearance::Circle(knob::CircleAppearance {
            color: colors::KNOB_BG,
            border_width: 3.0,
            border_color: colors::KNOB_BORDER,
            notch: knob::NotchShape::Circle(knob::CircleNotch {
                color: colors::HANDLE,
                border_width: 1.0,
                border_color: colors::FILLED,
                diameter: knob::StyleLength::Scaled(0.21),
                offset: knob::StyleLength::Scaled(0.21),
            }),
        })
    }

    fn hovered(&self, _style: &Self::Style) -> knob::Appearance {
        knob::Appearance::Circle(knob::CircleAppearance {
            notch: knob::NotchShape::Circle(knob::CircleNotch {
                color: colors::HANDLE_HOVER,
                border_color: colors::FILLED_HOVER,
                diameter: knob::StyleLength::Scaled(0.21),
                offset: knob::StyleLength::Scaled(0.21),
                border_width: 1.0,
            }),
            color: colors::KNOB_BG,
            border_width: 3.0,
            border_color: colors::KNOB_BORDER,
        })
    }

    fn gesturing(&self, style: &Self::Style) -> knob::Appearance {
        self.hovered(style)
    }

    fn value_arc_appearance(&self, _style: &Self::Style) -> Option<knob::ValueArcAppearance> {
        Some(knob::ValueArcAppearance {
            width: 3.0,
            offset: 1.5,
            empty_color: Some(colors::ARC_EMPTY),
            left_filled_color: colors::ARC_FILLED,
            right_filled_color: None,
            cap: LineCap::Butt,
        })
    }

    fn mod_range_arc_appearance(
        &self,
        _style: &Self::Style,
    ) -> Option<knob::ModRangeArcAppearance> {
        Some(knob::ModRangeArcAppearance {
            width: 3.0,
            offset: 6.0,
            empty_color: None,
            filled_color: colors::ARC_FILLED,
            filled_inverse_color: colors::ARC_RIGHT,
            cap: LineCap::Butt,
        })
    }

    fn text_marks_appearance(&self, _style: &Self::Style) -> Option<knob::TextMarksAppearance> {
        Some(knob::TextMarksAppearance {
            style: text_marks::Appearance {
                color: [0.16, 0.16, 0.16, 0.9].into(),
                text_size: 11,
                font: Default::default(),
                bounds_width: 20,
                bounds_height: 20,
            },
            offset: 15.0,
            h_char_offset: 3.0,
            v_offset: -0.75,
        })
    }
}

// --- Arc knob (slim modern look) ---

pub struct ArcKnob;

impl knob::StyleSheet for ArcKnob {
    type Style = nice_plug_iced::iced::Theme;

    fn idle(&self, _style: &Self::Style) -> knob::Appearance {
        knob::Appearance::Arc(knob::ArcAppearance {
            width: knob::StyleLength::Fixed(3.15),
            empty_color: colors::ARC_EMPTY,
            filled_color: colors::ARC_FILLED,
            notch: knob::NotchShape::Line(knob::LineNotch {
                color: colors::ARC_FILLED,
                width: knob::StyleLength::Fixed(3.15),
                length: knob::StyleLength::Scaled(0.25),
                cap: LineCap::Round,
                offset: knob::StyleLength::Fixed(2.5),
            }),
            cap: LineCap::Round,
        })
    }

    fn hovered(&self, style: &Self::Style) -> knob::Appearance {
        self.idle(style)
    }

    fn gesturing(&self, style: &Self::Style) -> knob::Appearance {
        self.idle(style)
    }

    fn angle_range(&self, _style: &Self::Style) -> iced_audio::KnobAngleRange {
        iced_audio::KnobAngleRange::from_deg(40.0, 320.0)
    }

    fn mod_range_arc_appearance(
        &self,
        _style: &Self::Style,
    ) -> Option<knob::ModRangeArcAppearance> {
        Some(knob::ModRangeArcAppearance {
            width: 3.0,
            offset: 1.5,
            empty_color: None,
            filled_color: colors::ARC_FILLED,
            filled_inverse_color: colors::ARC_RIGHT,
            cap: LineCap::Round,
        })
    }
}

// --- Bipolar arc knob (detune, stereo width) ---

pub struct ArcBipolarKnob;

impl knob::StyleSheet for ArcBipolarKnob {
    type Style = nice_plug_iced::iced::Theme;

    fn idle(&self, _style: &Self::Style) -> knob::Appearance {
        let notch_center = knob::LineNotch {
            color: colors::ARC_EMPTY,
            width: knob::StyleLength::Fixed(3.15),
            length: knob::StyleLength::Scaled(0.39),
            cap: LineCap::Butt,
            offset: knob::StyleLength::Fixed(0.0),
        };
        knob::Appearance::ArcBipolar(knob::ArcBipolarAppearance {
            width: knob::StyleLength::Fixed(3.15),
            empty_color: colors::ARC_EMPTY,
            left_filled_color: colors::ARC_FILLED,
            right_filled_color: colors::ARC_RIGHT,
            notch_center: knob::NotchShape::Line(notch_center.clone()),
            notch_left_right: Some((
                knob::NotchShape::Line(knob::LineNotch {
                    color: colors::ARC_FILLED,
                    ..notch_center.clone()
                }),
                knob::NotchShape::Line(knob::LineNotch {
                    color: colors::ARC_RIGHT,
                    ..notch_center
                }),
            )),
            cap: LineCap::Butt,
        })
    }

    fn hovered(&self, style: &Self::Style) -> knob::Appearance {
        self.idle(style)
    }

    fn gesturing(&self, style: &Self::Style) -> knob::Appearance {
        self.idle(style)
    }

    fn angle_range(&self, _style: &Self::Style) -> iced_audio::KnobAngleRange {
        iced_audio::KnobAngleRange::from_deg(40.0, 320.0)
    }
}
