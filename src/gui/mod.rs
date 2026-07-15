//! Iced GUI for Oscilla — VS Code Dark IDE theme.

use crate::OscillaParams;
use crate::dsp::WavetableSlot;
use crate::script::ScriptMode;
use iced_audio::Gesture;
use nice_plug::prelude::*;
use nice_plug_iced::iced::{
    self, Background, Border, Center, Color, Font, Length, PollSubNotifier, Shadow, Theme, border,
    font,
    theme::Palette,
    widget::{
        button,
        canvas::{self, Canvas, Frame, Geometry, Program},
        column, container, pick_list, row, text, text_editor,
    },
};
use nice_plug_iced::{EditorState, NiceGuiContext};
use std::sync::Arc;
use std::sync::atomic::Ordering;

// ── VS Code Dark+ palette ─────────────────────────────────────────────

const BG_DEEP: Color = Color::from_rgb(0.118, 0.118, 0.118); // #1E1E1E  editor background
const SURFACE: Color = Color::from_rgb(0.145, 0.145, 0.149); // #252526  panel / sidebar
const BORDER: Color = Color::from_rgb(0.235, 0.235, 0.235); // #3C3C3C  thin separators
const ACCENT: Color = Color::from_rgb(0.0, 0.478, 0.8); // #007ACC  VS Code blue
const ACCENT_SOFT: Color = Color::from_rgba(0.0, 0.478, 0.8, 0.12); // subtle wash (status bar, hover)
const ACCENT_GLOW: Color = Color::from_rgba(0.0, 0.478, 0.8, 0.22); // waveform under-glow
const ACCENT_LINE: Color = Color::from_rgba(0.0, 0.478, 0.8, 0.15); // waveform centre line
const FG_DIM: Color = Color::from_rgb(0.522, 0.522, 0.522); // #858585  muted labels
const FG_TEXT: Color = Color::from_rgb(0.831, 0.831, 0.831); // #D4D4D4  body text
const GREEN: Color = Color::from_rgb(0.306, 0.788, 0.690); // #4EC9B0  terminal green (success)
const RED: Color = Color::from_rgb(0.945, 0.298, 0.298); // #F14C4C  error red
const YELLOW: Color = Color::from_rgb(0.875, 0.808, 0.0); // #DFCE00  warning yellow

// ── Typography ────────────────────────────────────────────────────────

fn heading(label: &str) -> text::Text<'_> {
    text(label).size(10).color(FG_DIM).font(Font {
        weight: font::Weight::Semibold,
        ..Font::DEFAULT
    })
}

fn knob_label(label: &str) -> text::Text<'_> {
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

fn value_label(val: String) -> text::Text<'static> {
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

fn btn_text(label: &str) -> text::Text<'_> {
    text(label).size(11).color(Color::WHITE).font(Font {
        weight: font::Weight::Semibold,
        ..Font::DEFAULT
    })
}

fn status_text(label: &str, color: Color) -> text::Text<'_> {
    text(label).size(11).color(color).font(Font {
        weight: font::Weight::Normal,
        ..Font::MONOSPACE
    })
}

fn peak_text(val: String) -> text::Text<'static> {
    text(val).size(11).color(ACCENT).font(Font {
        weight: font::Weight::Bold,
        ..Font::MONOSPACE
    })
}

fn rad(r: f32) -> border::Radius {
    border::radius(r)
}

// ── Reusable style functions ──────────────────────────────────────────

fn section_panel() -> container::Style {
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

fn btn_style(hovered: bool) -> button::Style {
    let border_alpha = if hovered { 0.6 } else { 0.3 };
    let shadow = if hovered {
        Shadow {
            color: ACCENT_SOFT,
            offset: iced::Vector::new(0.0, 1.0),
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

fn picklist_style() -> pick_list::Style {
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

// ── Messages ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    Poll,
    EditorAction(text_editor::Action),
    CompileScript,

    VolumeGestured(Gesture),
    AttackGestured(Gesture),
    DecayGestured(Gesture),
    SustainGestured(Gesture),
    ReleaseGestured(Gesture),
    CutoffGestured(Gesture),
    ResonanceGestured(Gesture),
    UnisonVoicesGestured(Gesture),
    DetuneGestured(Gesture),
    WidthGestured(Gesture),
    GlideGestured(Gesture),

    FilterTypeChanged(crate::dsp::filter::FilterType),
    ScriptModeChanged(crate::script::ScriptMode),
}

// ── Editor state ──────────────────────────────────────────────────────

pub struct OscillaEditorState {
    pub params: Arc<OscillaParams>,
    pub wavetable_slot: Arc<WavetableSlot>,
    pub time_buffer_slot: Arc<crate::dsp::TimeBufferSlot>,
    pub peak_output: Arc<AtomicF32>,
    pub notifier: PollSubNotifier,
    pub compiler: Arc<crate::script::ScriptCompiler>,
    /// Script editor content — persists between editor opens.
    pub script_content: text_editor::Content,
}

// ── Application ───────────────────────────────────────────────────────

pub struct OscillaGui {
    editor_state: EditorState<OscillaEditorState>,
    #[allow(dead_code)]
    nice_ctx: NiceGuiContext,
    status_message: String,
    compile_ok: bool,
    peak_output_db: f32,
}

impl OscillaGui {
    pub fn new(
        mut editor_state: EditorState<OscillaEditorState>,
        nice_ctx: NiceGuiContext,
    ) -> Self {
        // DAW restores params AFTER editor() creates OscillaEditorState,
        // so script_content may still be the default. Sync it now.
        let saved = editor_state.params.wave_script.borrow().clone();
        if editor_state.script_content.text() != saved {
            editor_state.script_content = text_editor::Content::with_text(&saved);
        }
        Self {
            editor_state,
            nice_ctx,
            status_message: String::from("Ready"),
            compile_ok: true,
            peak_output_db: util::MINUS_INFINITY_DB,
        }
    }

    pub fn theme(&self) -> Option<Theme> {
        Some(Theme::custom(
            "VS Code Dark+",
            Palette {
                background: BG_DEEP,
                text: FG_TEXT,
                primary: ACCENT,
                success: GREEN,
                danger: RED,
                warning: YELLOW,
            },
        ))
    }

    pub fn update(&mut self, message: Message) {
        let setter = self.nice_ctx.param_setter();
        let p = &self.editor_state.params;
        match message {
            Message::Poll => {
                self.peak_output_db =
                    util::gain_to_db(self.editor_state.peak_output.load(Ordering::Relaxed));
            }
            Message::EditorAction(a) => self.editor_state.script_content.perform(a),
            Message::CompileScript => {
                let src = self.editor_state.script_content.text();
                let mode = match self.editor_state.params.script_mode.modulated_plain_value() {
                    0 => crate::script::ScriptMode::Wavetable,
                    _ => crate::script::ScriptMode::TimeBased,
                };
                let comp = self.editor_state.compiler.clone();
                self.status_message = String::from("Compiling...");
                self.compile_ok = true;
                match comp.compile(&src, mode) {
                    Ok(()) => {
                        let sr = 44100.0; // default; actual sample rate from host isn't available here
                        match comp.generate_both(mode, sr) {
                            Ok((wt_opt, tb_opt)) => {
                                if let Some(wt) = wt_opt {
                                    self.editor_state.wavetable_slot.store(wt);
                                }
                                if let Some(tb) = tb_opt {
                                    self.editor_state.time_buffer_slot.store(tb);
                                }
                                // Persist script to params so DAW saves it.
                                *self.editor_state.params.wave_script.borrow_mut() =
                                    self.editor_state.script_content.text();
                                self.status_message = String::from("Compiled");
                            }
                            Err(e) => {
                                self.status_message = format!("Gen error: {e}");
                                self.compile_ok = false;
                            }
                        }
                    }
                    Err(e) => {
                        self.status_message = format!("Error: {e}");
                        self.compile_ok = false;
                    }
                }
            }
            Message::VolumeGestured(g) => iced_audio::param::set_nice_param(&p.volume, g, &setter),
            Message::AttackGestured(g) => iced_audio::param::set_nice_param(&p.attack, g, &setter),
            Message::DecayGestured(g) => iced_audio::param::set_nice_param(&p.decay, g, &setter),
            Message::SustainGestured(g) => {
                iced_audio::param::set_nice_param(&p.sustain, g, &setter)
            }
            Message::ReleaseGestured(g) => {
                iced_audio::param::set_nice_param(&p.release, g, &setter)
            }
            Message::CutoffGestured(g) => {
                iced_audio::param::set_nice_param(&p.filter_cutoff, g, &setter)
            }
            Message::ResonanceGestured(g) => {
                iced_audio::param::set_nice_param(&p.filter_resonance, g, &setter)
            }
            Message::UnisonVoicesGestured(g) => {
                iced_audio::param::set_nice_param(&p.unison_voices, g, &setter)
            }
            Message::DetuneGestured(g) => {
                iced_audio::param::set_nice_param(&p.detune_cents, g, &setter)
            }
            Message::WidthGestured(g) => {
                iced_audio::param::set_nice_param(&p.stereo_width, g, &setter)
            }
            Message::GlideGestured(g) => {
                iced_audio::param::set_nice_param(&p.glide_time, g, &setter)
            }
            Message::FilterTypeChanged(ft) => {
                setter.begin_set_parameter(&p.filter_type);
                setter.set_parameter_normalized(&p.filter_type, ft as i32 as f32 / 2.0);
                setter.end_set_parameter(&p.filter_type);
            }
            Message::ScriptModeChanged(mode) => {
                let val = mode as i32 as f32 / 1.0;
                setter.begin_set_parameter(&p.script_mode);
                setter.set_parameter_normalized(&p.script_mode, val);
                setter.end_set_parameter(&p.script_mode);
            }
        }
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        let p = &self.editor_state.params;

        /// A knob unit: label → knob → numeric value.
        fn knob<'a>(
            label: &'a str,
            value: String,
            param: &'a FloatParam,
            gesture: fn(Gesture) -> Message,
        ) -> iced::widget::Column<'a, Message> {
            let ip = iced_audio::param::nice_to_iced(param);
            column![
                knob_label(label),
                iced_audio::Knob::new(ip).on_gesture(gesture),
                value_label(value)
            ]
            .spacing(3)
            .align_x(Center)
        }

        fn section<'a>(
            label: &'a str,
            kids: iced::widget::Row<'a, Message>,
        ) -> iced::widget::Container<'a, Message> {
            container(column![heading(label), kids].spacing(10))
                .style(|_| section_panel())
                .padding(14)
                .width(Length::Fill)
        }

        // ── Script editor (fills leftover space) ───────────────────
        let editor = text_editor(&self.editor_state.script_content)
            .placeholder("Enter expression... e.g. sin(x) + sin(x*3)*0.25")
            .on_action(Message::EditorAction)
            .font(Font::MONOSPACE)
            .size(iced::Pixels(13.0))
            .height(Length::Fill)
            .style(|_theme, _status| text_editor::Style {
                background: Background::Color(BG_DEEP),
                border: Border {
                    color: BORDER,
                    width: 1.0,
                    radius: rad(3.0),
                },
                placeholder: FG_DIM,
                selection: Color::from_rgba(0.0, 0.478, 0.8, 0.3),
                value: FG_TEXT,
            })
            .highlight("Rust", iced_highlighter::Theme::Base16Ocean);

        let ok = if self.compile_ok { GREEN } else { RED };

        let compile_btn = button(btn_text("Compile & Apply"))
            .on_press(Message::CompileScript)
            .padding([6, 18])
            .style(|_theme, status| match status {
                button::Status::Active => btn_style(false),
                button::Status::Hovered | button::Status::Pressed => btn_style(true),
                _ => button::Style {
                    background: Some(Background::Color(SURFACE)),
                    border: Border {
                        color: BORDER,
                        width: 1.0,
                        radius: rad(3.0),
                    },
                    text_color: FG_DIM,
                    shadow: Shadow::default(),
                    snap: false,
                },
            });

        let mode = match p.script_mode.modulated_plain_value() {
            0 => ScriptMode::Wavetable,
            _ => ScriptMode::TimeBased,
        };

        let mode_picklist = pick_list(
            &[ScriptMode::Wavetable, ScriptMode::TimeBased][..],
            Some(mode),
            Message::ScriptModeChanged,
        )
        .text_size(11)
        .padding([6, 12])
        .style(|_theme, _status| picklist_style());

        let mode_column = row![mode_picklist, heading("Mode"),]
            .spacing(6)
            .align_y(Center);

        let editor_panel = container(
            column![
                heading("WAVEFORM SCRIPT"),
                mode_column,
                editor,
                row![compile_btn, status_text(&self.status_message, ok),]
                    .spacing(12)
                    .align_y(Center),
            ]
            .spacing(8),
        )
        .style(|_| section_panel())
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill);

        // ── Waveform preview (fixed height) ────────────────────────
        let preview = Canvas::new(WaveformPreview {
            wavetable: self.editor_state.wavetable_slot.load(),
            time_buffer: self.editor_state.time_buffer_slot.load(),
            mode,
            accent: ACCENT,
        })
        .width(Length::Fill)
        .height(Length::Fixed(100.0));

        let preview_panel = container(preview)
            .style(|_| section_panel())
            .padding(4)
            .width(Length::Fill);

        // ── Envelope ───────────────────────────────────────────────
        let envelope = section(
            "ENVELOPE",
            row![
                knob(
                    "Attack",
                    format!("{:.3}", p.attack.modulated_plain_value()),
                    &p.attack,
                    Message::AttackGestured
                ),
                knob(
                    "Decay",
                    format!("{:.3}", p.decay.modulated_plain_value()),
                    &p.decay,
                    Message::DecayGestured
                ),
                knob(
                    "Sustain",
                    format!("{:.0}%", p.sustain.modulated_plain_value() * 100.0),
                    &p.sustain,
                    Message::SustainGestured
                ),
                knob(
                    "Release",
                    format!("{:.3}", p.release.modulated_plain_value()),
                    &p.release,
                    Message::ReleaseGestured
                ),
            ]
            .spacing(14),
        );

        // ── Filter ─────────────────────────────────────────────────
        let ft = match p.filter_type.modulated_plain_value() {
            0 => crate::dsp::filter::FilterType::LowPass,
            1 => crate::dsp::filter::FilterType::HighPass,
            _ => crate::dsp::filter::FilterType::BandPass,
        };
        let freq = p.filter_cutoff.modulated_plain_value();
        let freq_str = if freq >= 1000.0 {
            format!("{:.1}k", freq / 1000.0)
        } else {
            format!("{:.0}", freq)
        };

        let filter = section(
            "FILTER",
            row![
                knob(
                    "Cutoff",
                    freq_str,
                    &p.filter_cutoff,
                    Message::CutoffGestured
                ),
                knob(
                    "Res",
                    format!("{:.0}%", p.filter_resonance.modulated_plain_value() * 100.0),
                    &p.filter_resonance,
                    Message::ResonanceGestured
                ),
                column![
                    heading("Type"),
                    pick_list(
                        &[
                            crate::dsp::filter::FilterType::LowPass,
                            crate::dsp::filter::FilterType::HighPass,
                            crate::dsp::filter::FilterType::BandPass
                        ][..],
                        Some(ft),
                        Message::FilterTypeChanged,
                    )
                    .text_size(11)
                    .padding([6, 12])
                    .style(|_theme, _status| picklist_style()),
                ]
                .spacing(6)
                .align_x(Center),
            ]
            .spacing(14),
        );

        // ── Unison ─────────────────────────────────────────────────
        let unison = section(
            "UNISON",
            row![
                knob(
                    "Voices",
                    format!("{}", p.unison_voices.modulated_plain_value() as i32),
                    &p.unison_voices,
                    Message::UnisonVoicesGestured
                ),
                knob(
                    "Detune",
                    format!("{:.0}", p.detune_cents.modulated_plain_value()),
                    &p.detune_cents,
                    Message::DetuneGestured
                ),
                knob(
                    "Width",
                    format!("{:.0}%", p.stereo_width.modulated_plain_value() * 100.0),
                    &p.stereo_width,
                    Message::WidthGestured
                ),
            ]
            .spacing(14),
        );

        // ── Master ─────────────────────────────────────────────────
        let master = section(
            "MASTER",
            row![
                knob(
                    "Volume",
                    format!("{:.0}%", p.volume.modulated_plain_value() * 100.0),
                    &p.volume,
                    Message::VolumeGestured
                ),
                knob(
                    "Glide",
                    format!("{:.3}", p.glide_time.modulated_plain_value()),
                    &p.glide_time,
                    Message::GlideGestured
                ),
            ]
            .spacing(14),
        );

        // ── Footer ─────────────────────────────────────────────────
        let peak = if self.peak_output_db <= util::MINUS_INFINITY_DB {
            String::from("-- dB")
        } else {
            format!("{:.1} dB", self.peak_output_db)
        };
        let footer = container(
            row![
                text("Oscilla").size(11).color(FG_DIM).font(Font {
                    weight: font::Weight::Bold,
                    ..Font::MONOSPACE
                }),
                peak_text(peak),
            ]
            .spacing(16),
        )
        .style(|_| section_panel())
        .padding([8, 14])
        .width(Length::Fill);

        // ── Grid layout ────────────────────────────────────────────
        //
        //  ┌──────────────────────────┬─────────────────┐
        //  │  Script Editor (FILL)    │   ENVELOPE      │
        //  │                          │   FILTER        │
        //  │                          │   UNISON        │
        //  ├──────────────────────────┤   MASTER        │
        //  │  Waveform (fixed 100px)  │   FOOTER        │
        //  └──────────────────────────┴─────────────────┘
        //

        let left = column![
            editor_panel,  // fills remaining vertical space
            preview_panel, // fixed 100 px
        ]
        .spacing(10)
        .width(Length::FillPortion(3));

        let right = column![envelope, filter, unison, master, footer]
            .spacing(8)
            .width(Length::FillPortion(2));

        container(row![left, right].spacing(12).padding(12))
            .style(|_| container::Style {
                background: Some(Background::Color(BG_DEEP)),
                border: Border::default(),
                shadow: Shadow::default(),
                text_color: None,
                snap: false,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

// ── Waveform preview canvas ───────────────────────────────────────────

struct WaveformPreview {
    wavetable: crate::wavetable::SharedWavetable,
    time_buffer: crate::wavetable::SharedTimeBuffer,
    mode: ScriptMode,
    accent: Color,
}

impl Program<Message> for WaveformPreview {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h, mid) = (bounds.width, bounds.height, bounds.height / 2.0);

        // Subtle grid.
        let grid = Color::from_rgba(1.0, 1.0, 1.0, 0.04);
        let (sx, sy) = (w / 8.0, h / 6.0);
        let mut x = 0.0;
        while x <= w {
            frame.fill_rectangle(iced::Point::new(x, 0.0), iced::Size::new(0.5, h), grid);
            x += sx;
        }
        let mut y = 0.0;
        while y <= h {
            frame.fill_rectangle(iced::Point::new(0.0, y), iced::Size::new(w, 0.5), grid);
            y += sy;
        }

        // Accent center line.
        frame.fill_rectangle(
            iced::Point::new(0.0, mid - 0.5),
            iced::Size::new(w, 1.0),
            ACCENT_LINE,
        );

        // Waveform path.
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
                        builder.move_to(iced::Point::new(px, py));
                        first = false;
                    } else {
                        builder.line_to(iced::Point::new(px, py));
                    }
                    i += step;
                }
            }
            ScriptMode::TimeBased => {
                let data = self.time_buffer.as_ref();
                if !data.is_empty() {
                    let step = (data.len() as f32 / w).max(1.0);
                    while i < data.len() as f32 {
                        let s = data[i as usize];
                        let px = (i / data.len() as f32) * w;
                        let py = mid - s * mid * 0.80;
                        if first {
                            builder.move_to(iced::Point::new(px, py));
                            first = false;
                        } else {
                            builder.line_to(iced::Point::new(px, py));
                        }
                        i += step;
                    }
                }
            }
        }
        let path = builder.build();

        // Glow under-layer.
        frame.stroke(
            &path,
            canvas::Stroke::default()
                .with_color(ACCENT_GLOW)
                .with_width(3.0),
        );
        // Sharp waveform on top.
        frame.stroke(
            &path,
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(1.5),
        );

        vec![frame.into_geometry()]
    }
}
