use super::theme::{ACCENT_GLOW, ACCENT_LINE};
use crate::script::ScriptMode;
use crate::wavetable::{SCOPE_SIZE, SharedWavetable};
use nice_plug_iced::iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    mouse::Cursor,
    widget::canvas::{self, Canvas, Frame, Geometry, Program},
};

use super::Message;

pub struct WaveformPreview {
    pub wavetable: SharedWavetable,
    pub scope_snapshot: Box<[f32; SCOPE_SIZE]>,
    pub mode: ScriptMode,
    pub accent: Color,
}

impl Program<Message> for WaveformPreview {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h, mid) = (bounds.width, bounds.height, bounds.height / 2.0);

        let grid = Color::from_rgba(1.0, 1.0, 1.0, 0.04);
        let (sx, sy) = (w / 8.0, h / 6.0);
        let mut x = 0.0;
        while x <= w {
            frame.fill_rectangle(Point::new(x, 0.0), Size::new(0.5, h), grid);
            x += sx;
        }
        let mut y = 0.0;
        while y <= h {
            frame.fill_rectangle(Point::new(0.0, y), Size::new(w, 0.5), grid);
            y += sy;
        }

        frame.fill_rectangle(Point::new(0.0, mid - 0.5), Size::new(w, 1.0), ACCENT_LINE);

        let mut builder = canvas::path::Builder::new();
        let (mut first, mut i) = (true, 0.0f32);

        match self.mode {
            ScriptMode::Wavetable => {
                let data = self.wavetable.as_ref();
                let step = (data.len() as f32 / w).max(1.0);
                while i < data.len() as f32 {
                    let s = data[i as usize];
                    let px = (i / data.len() as f32) * w;
                    let py = mid - s * mid * 0.80;
                    if first {
                        builder.move_to(Point::new(px, py));
                        first = false;
                    } else {
                        builder.line_to(Point::new(px, py));
                    }
                    i += step;
                }
            }
            ScriptMode::TimeBased => {
                let data = self.scope_snapshot.as_ref();
                let n = data.len() as f32;
                let step = (n / w).max(1.0);
                while i < n {
                    let s = data[i as usize];
                    let px = (i / n) * w;
                    let py = mid - s * mid * 0.80;
                    if first {
                        builder.move_to(Point::new(px, py));
                        first = false;
                    } else {
                        builder.line_to(Point::new(px, py));
                    }
                    i += step;
                }
            }
        }
        let path = builder.build();

        frame.stroke(
            &path,
            canvas::Stroke::default()
                .with_color(ACCENT_GLOW)
                .with_width(3.0),
        );
        frame.stroke(
            &path,
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(1.5),
        );

        vec![frame.into_geometry()]
    }
}

pub fn waveform_canvas(
    wavetable: SharedWavetable,
    scope_snapshot: Box<[f32; SCOPE_SIZE]>,
    mode: ScriptMode,
) -> Canvas<WaveformPreview, Message> {
    use nice_plug_iced::iced::Length;
    Canvas::new(WaveformPreview {
        wavetable,
        scope_snapshot,
        mode,
        accent: super::theme::ACCENT,
    })
    .width(Length::Fill)
    .height(Length::Fill)
}
