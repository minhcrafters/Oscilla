use crate::OscillaParams;
use crate::OscillaTask;
use crate::dsp::WavetableSlot;
use crate::dsp::filter::FilterType;
use crate::script::{ScriptCompiler, ScriptMode};
use crate::wavetable::{SCOPE_SIZE, SharedWavetable};
use arc_swap::ArcSwap;
use iced_audio::param::nice_to_iced;
use iced_audio::{Gesture, Knob};
use iced_code_editor::CodeEditor;
use iced_code_editor::IndentStyle;
use iced_code_editor::theme;
use nice_plug::prelude::*;
use nice_plug_iced::iced::Task;
use nice_plug_iced::iced::{
    Background, Border, Center, Color, Element, Font, Length, Point, PollSubNotifier, Rectangle,
    Renderer, Shadow, Size, Theme, Vector, border, font,
    mouse::Cursor,
    theme::Palette,
    widget::{
        Column, Container, Row, Space, button,
        canvas::{self, Canvas, Frame, Geometry, Program},
        column, container, pick_list, row, text,
    },
};
use nice_plug_iced::{EditorState, NiceGuiContext};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

/// Thread-safe handle for `CodeEditor`.
///
/// `CodeEditor` contains `Rc` and is `!Send`, but the editor is only ever
/// accessed from the GUI thread. This wrapper provides an `unsafe impl Send`
/// so it can be stored in `EditorState<T: Send>`.
pub struct EditorHandle(pub CodeEditor);

// SAFETY: EditorHandle is only ever accessed on the GUI thread.
unsafe impl Send for EditorHandle {}
unsafe impl Sync for EditorHandle {}

impl Deref for EditorHandle {
    type Target = CodeEditor;
    fn deref(&self) -> &CodeEditor {
        &self.0
    }
}

impl DerefMut for EditorHandle {
    fn deref_mut(&mut self) -> &mut CodeEditor {
        &mut self.0
    }
}

pub mod knob_style;

// Palette

const BG_DEEP: Color = Color::from_rgb(0.118, 0.118, 0.118);
const SURFACE: Color = Color::from_rgb(0.145, 0.145, 0.149);
const BORDER: Color = Color::from_rgb(0.243, 0.243, 0.259);
const ACCENT: Color = Color::from_rgb(0.0, 0.478, 0.8);
const ACCENT_SOFT: Color = Color::from_rgba(0.0, 0.478, 0.8, 0.12);
const ACCENT_GLOW: Color = Color::from_rgba(0.0, 0.478, 0.8, 0.22);
const ACCENT_LINE: Color = Color::from_rgba(0.0, 0.478, 0.8, 0.15);
const FG_DIM: Color = Color::from_rgb(0.522, 0.522, 0.522);
const FG_TEXT: Color = Color::from_rgb(0.831, 0.831, 0.831);
const GREEN: Color = Color::from_rgb(0.306, 0.788, 0.690);
const RED: Color = Color::from_rgb(0.945, 0.298, 0.298);
const YELLOW: Color = Color::from_rgb(0.875, 0.808, 0.0);

/// VS Code Dark+ theme for the code editor.
fn vscode_editor_style() -> theme::Style {
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

// Typography

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

// Styles

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

// Messages

#[derive(Debug, Clone)]
pub enum Message {
    Poll,
    EditorEvent(iced_code_editor::Message),
    CompileScript,
    SelectAll,

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

    FilterTypeChanged(FilterType),
    ScriptModeChanged(ScriptMode),
}

// Editor state

pub struct OscillaEditorState {
    pub params: Arc<OscillaParams>,
    pub wavetable_slot: Arc<WavetableSlot>,
    pub lua_source_slot: Arc<ArcSwap<String>>,
    pub peak_output: Arc<AtomicF32>,
    pub scope_buffer: Arc<Mutex<Box<[f32; SCOPE_SIZE]>>>,
    pub notifier: PollSubNotifier,
    pub compiler: Arc<ScriptCompiler>,
    pub sample_rate: Arc<AtomicF32>,
    pub async_executor: Arc<dyn Fn(OscillaTask) + Send + Sync>,
    pub editor_handle: EditorHandle,
}

// --- Knob helpers ---

fn arc_knob<'a>(
    label: &'a str,
    value: String,
    param: &'a FloatParam,
    gesture: fn(Gesture) -> Message,
) -> Column<'a, Message> {
    let ip = nice_to_iced(param);
    column![
        knob_label(label),
        Knob::new(ip).on_gesture(gesture).style(knob_style::ArcKnob),
        value_label(value)
    ]
    .spacing(3)
    .align_x(Center)
}

#[allow(dead_code)]
fn bipolar_knob<'a>(
    label: &'a str,
    value: String,
    param: &'a FloatParam,
    gesture: fn(Gesture) -> Message,
) -> Column<'a, Message> {
    let ip = nice_to_iced(param);
    column![
        knob_label(label),
        Knob::new(ip)
            .on_gesture(gesture)
            .style(knob_style::ArcBipolarKnob),
        value_label(value)
    ]
    .spacing(3)
    .align_x(Center)
}

// Application

pub struct OscillaGui {
    editor_state: EditorState<OscillaEditorState>,
    #[allow(dead_code)]
    nice_ctx: NiceGuiContext,
    status_message: String,
    compile_ok: bool,
    peak_output_db: f32,
    compile_pending: bool,
}

impl OscillaGui {
    pub fn new(
        mut editor_state: EditorState<OscillaEditorState>,
        nice_ctx: NiceGuiContext,
    ) -> (Self, Task<Message>) {
        // Configure the code editor to match VS Code Dark+ theme.
        editor_state.editor_handle.set_font(Font::MONOSPACE);
        editor_state.editor_handle.set_font_size(13.0, true);
        editor_state.editor_handle.set_folding_enabled(false);
        editor_state.editor_handle.set_theme(vscode_editor_style());
        editor_state
            .editor_handle
            .set_indent_style(IndentStyle::Spaces(2));

        // The nice-plug wrapper calls Plugin::editor() before the host restores
        // plugin state, so the CodeEditor may have been created with the default
        // "math.sin(x)" text.  At this point (window-open time) persisted params have
        // been restored, so sync the editor content with the real wave script.
        let persisted_script = editor_state.params.wave_script.borrow().clone();
        let task = if editor_state.editor_handle.content() != persisted_script {
            editor_state
                .editor_handle
                .reset(&persisted_script)
                .map(Message::EditorEvent)
        } else {
            Task::none()
        };

        (
            Self {
                editor_state,
                nice_ctx,
                status_message: String::from("Ready"),
                compile_ok: true,
                peak_output_db: util::MINUS_INFINITY_DB,
                compile_pending: false,
            },
            task,
        )
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

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let setter = self.nice_ctx.param_setter();
        let p = &self.editor_state.params;
        match message {
            Message::Poll => {
                self.peak_output_db =
                    util::gain_to_db(self.editor_state.peak_output.load(Ordering::Relaxed));
                // Always check for errors
                if let Some(err) = self.editor_state.compiler.take_last_error() {
                    self.compile_pending = false;
                    self.compile_ok = false;
                    self.status_message = err;
                } else if self.compile_pending
                    && self.editor_state.compiler.current_mode().is_some()
                {
                    self.compile_pending = false;
                    self.status_message = String::from("Compiled");
                }

                Task::none()
            }
            Message::EditorEvent(event) => {
                CodeEditor::update(&mut self.editor_state.editor_handle, &event)
                    .map(Message::EditorEvent)
            }
            Message::SelectAll => {
                let editor = &mut self.editor_state.editor_handle;
                let line_count = editor.content().lines().count();

                let mut task = CodeEditor::update(editor, &iced_code_editor::Message::CtrlHome)
                    .map(Message::EditorEvent);

                for _ in 0..line_count.saturating_sub(1) {
                    task = task.chain(
                        CodeEditor::update(editor, &iced_code_editor::Message::End(true))
                            .map(Message::EditorEvent),
                    );
                    task = task.chain(
                        CodeEditor::update(
                            editor,
                            &iced_code_editor::Message::ArrowKey(
                                iced_code_editor::ArrowDirection::Down,
                                true,
                            ),
                        )
                        .map(Message::EditorEvent),
                    );
                }
                // Select to end of last line.
                task.chain(
                    CodeEditor::update(editor, &iced_code_editor::Message::End(true))
                        .map(Message::EditorEvent),
                )
            }
            Message::CompileScript => {
                let src = self.editor_state.editor_handle.content();
                let mode = match self.editor_state.params.script_mode.modulated_plain_value() {
                    0 => ScriptMode::Wavetable,
                    _ => ScriptMode::TimeBased,
                };
                *self.editor_state.params.wave_script.borrow_mut() = src.clone();
                // Clear any stale error before kicking off new compilation.
                self.editor_state.compiler.take_last_error();
                self.status_message = String::from("Compiling...");
                self.compile_ok = true;
                self.compile_pending = true;
                (self.editor_state.async_executor)(OscillaTask::CompileScript {
                    source: src,
                    mode,
                });

                Task::none()
            }
            Message::VolumeGestured(g) => {
                iced_audio::param::set_nice_param(&p.volume, g, &setter);

                Task::none()
            }
            Message::AttackGestured(g) => {
                iced_audio::param::set_nice_param(&p.attack, g, &setter);

                Task::none()
            }
            Message::DecayGestured(g) => {
                iced_audio::param::set_nice_param(&p.decay, g, &setter);

                Task::none()
            }
            Message::SustainGestured(g) => {
                iced_audio::param::set_nice_param(&p.sustain, g, &setter);

                Task::none()
            }
            Message::ReleaseGestured(g) => {
                iced_audio::param::set_nice_param(&p.release, g, &setter);

                Task::none()
            }
            Message::CutoffGestured(g) => {
                iced_audio::param::set_nice_param(&p.filter_cutoff, g, &setter);

                Task::none()
            }
            Message::ResonanceGestured(g) => {
                iced_audio::param::set_nice_param(&p.filter_resonance, g, &setter);

                Task::none()
            }
            Message::UnisonVoicesGestured(g) => {
                iced_audio::param::set_nice_param(&p.unison_voices, g, &setter);

                Task::none()
            }
            Message::DetuneGestured(g) => {
                iced_audio::param::set_nice_param(&p.detune_cents, g, &setter);

                Task::none()
            }
            Message::WidthGestured(g) => {
                iced_audio::param::set_nice_param(&p.stereo_width, g, &setter);

                Task::none()
            }
            Message::GlideGestured(g) => {
                iced_audio::param::set_nice_param(&p.glide_time, g, &setter);

                Task::none()
            }
            Message::FilterTypeChanged(ft) => {
                setter.begin_set_parameter(&p.filter_type);
                setter.set_parameter_normalized(&p.filter_type, ft as i32 as f32 / 2.0);
                setter.end_set_parameter(&p.filter_type);

                Task::none()
            }
            Message::ScriptModeChanged(mode) => {
                let val = mode as i32 as f32 / 1.0;
                setter.begin_set_parameter(&p.script_mode);
                setter.set_parameter_normalized(&p.script_mode, val);
                setter.end_set_parameter(&p.script_mode);

                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let p = &self.editor_state.params;

        fn section<'a>(label: &'a str, kids: Row<'a, Message>) -> Container<'a, Message> {
            container(column![heading(label), kids].spacing(10))
                .style(|_| section_panel())
                .padding(12)
                .width(Length::Fill)
        }

        let editor: Element<'_, Message> = self
            .editor_state
            .editor_handle
            .view()
            .map(Message::EditorEvent);

        let editor_container = container(editor)
            .style(|_| container::Style {
                background: Some(Background::Color(BG_DEEP)),
                border: Border {
                    color: BORDER,
                    width: 1.0,
                    radius: rad(3.0),
                },
                ..Default::default()
            })
            .height(Length::Fill);

        let ok = if self.compile_ok { GREEN } else { RED };

        let compile_btn = button(btn_text("Apply"))
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
                heading("SCRIPT"),
                mode_column,
                editor_container,
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

        // Waveform preview
        let scope_snapshot = {
            let buf = self.editor_state.scope_buffer.lock().unwrap();
            (*buf).clone()
        };
        let preview = Canvas::new(WaveformPreview {
            wavetable: self.editor_state.wavetable_slot.load(),
            scope_snapshot,
            mode,
            accent: ACCENT,
        })
        .width(Length::Fill)
        .height(Length::Fill);

        let preview_panel = container(preview)
            .style(|_| section_panel())
            .padding(12)
            .width(Length::Fill);

        // Envelope
        let envelope = section(
            "ENVELOPE",
            row![
                arc_knob(
                    "Attack",
                    format!("{:.3}", p.attack.modulated_plain_value()),
                    &p.attack,
                    Message::AttackGestured
                ),
                arc_knob(
                    "Decay",
                    format!("{:.3}", p.decay.modulated_plain_value()),
                    &p.decay,
                    Message::DecayGestured
                ),
                arc_knob(
                    "Sustain",
                    format!("{:.0}%", p.sustain.modulated_plain_value() * 100.0),
                    &p.sustain,
                    Message::SustainGestured
                ),
                arc_knob(
                    "Release",
                    format!("{:.3}", p.release.modulated_plain_value()),
                    &p.release,
                    Message::ReleaseGestured
                ),
            ]
            .spacing(14),
        );

        // Filter
        let ft = match p.filter_type.modulated_plain_value() {
            0 => FilterType::LowPass,
            1 => FilterType::HighPass,
            _ => FilterType::BandPass,
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
                arc_knob(
                    "Cutoff",
                    freq_str,
                    &p.filter_cutoff,
                    Message::CutoffGestured
                ),
                arc_knob(
                    "Res",
                    format!("{:.0}%", p.filter_resonance.modulated_plain_value() * 100.0),
                    &p.filter_resonance,
                    Message::ResonanceGestured
                ),
                column![
                    knob_label("Type"),
                    pick_list(
                        &[
                            FilterType::LowPass,
                            FilterType::HighPass,
                            FilterType::BandPass
                        ][..],
                        Some(ft),
                        Message::FilterTypeChanged,
                    )
                    .text_size(11)
                    .padding([6, 12])
                    .style(|_theme, _status| picklist_style()),
                ]
                .spacing(3)
                .align_x(Center),
            ]
            .spacing(14),
        );

        // Unison
        let unison = section(
            "UNISON",
            row![
                arc_knob(
                    "Voices",
                    format!("{}", p.unison_voices.modulated_plain_value() as i32),
                    &p.unison_voices,
                    Message::UnisonVoicesGestured
                ),
                arc_knob(
                    "Detune",
                    format!("{:.0}", p.detune_cents.modulated_plain_value()),
                    &p.detune_cents,
                    Message::DetuneGestured
                ),
                arc_knob(
                    "Width",
                    format!("{:.0}%", p.stereo_width.modulated_plain_value() * 100.0),
                    &p.stereo_width,
                    Message::WidthGestured
                ),
            ]
            .spacing(14),
        );

        // Master
        let master = section(
            "MASTER",
            row![
                arc_knob(
                    "Volume",
                    format!("{:.0}%", p.volume.modulated_plain_value() * 100.0),
                    &p.volume,
                    Message::VolumeGestured
                ),
                arc_knob(
                    "Glide",
                    format!("{:.3}", p.glide_time.modulated_plain_value()),
                    &p.glide_time,
                    Message::GlideGestured
                ),
            ]
            .spacing(14),
        );

        // Footer
        let peak = if self.peak_output_db <= util::MINUS_INFINITY_DB {
            String::from("-- dB")
        } else {
            format!("{:.1} dB", self.peak_output_db)
        };
        let footer = container(row![
            text(format!(
                "Oscilla v{}-{}",
                env!("CARGO_PKG_VERSION"),
                if cfg!(debug_assertions) { "dev" } else { "rel" }
            ))
            .size(11)
            .color(FG_DIM)
            .font(Font {
                weight: font::Weight::Bold,
                ..Font::MONOSPACE
            }),
            Space::new().width(Length::Fill),
            peak_text(peak),
        ])
        .style(|_| section_panel())
        .padding(12)
        .width(Length::Fill);

        // Grid layout
        //
        let left = column![
            editor_panel.height(Length::FillPortion(2)),
            preview_panel.height(Length::FillPortion(1)),
        ]
        .spacing(10)
        .width(Length::FillPortion(3));

        let right = column![
            envelope.height(Length::Fill),
            filter.height(Length::Fill),
            unison.height(Length::Fill),
            master.height(Length::Fill),
            footer,
        ]
        .spacing(10)
        .width(Length::FillPortion(2))
        .height(Length::Fill);

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

// Waveform preview

struct WaveformPreview {
    wavetable: SharedWavetable,
    /// Snapshot of the oscilloscope ring buffer for this frame.
    scope_snapshot: Box<[f32; SCOPE_SIZE]>,
    mode: ScriptMode,
    accent: Color,
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

        // Subtle grid.
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

        // Accent center line.
        frame.fill_rectangle(Point::new(0.0, mid - 0.5), Size::new(w, 1.0), ACCENT_LINE);

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
                // Draw the oscilloscope trace: map the 512-sample window
                // across the preview width, centred vertically.
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
