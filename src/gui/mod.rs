use crate::dsp::filter::FilterType;
#[cfg(feature = "time-buffer")]
use crate::dsp::TimeBufferSlot;
use crate::dsp::WavetableSlot;
use crate::preset::Preset;
use crate::script::{ScriptCompiler, ScriptMode};
use crate::wavetable::{SharedWavetable, SCOPE_SIZE};
use crate::OscillaParams;
use crate::OscillaTask;
use arc_swap::ArcSwap;
use iced_audio::param::nice_to_iced;
use iced_audio::{Gesture, Knob};
use iced_code_editor::theme;
use iced_code_editor::CodeEditor;
use iced_code_editor::IndentStyle;
use nice_plug::prelude::*;
use nice_plug_iced::iced::Task;
use nice_plug_iced::iced::{
    border, font,
    mouse::Cursor,
    theme::Palette,
    widget::{
        button,
        canvas::{self, Canvas, Frame, Geometry, Program},
        column, container, mouse_area, pick_list, row, scrollable, text, text_input, Column,
        Container, Row, Space,
    },
    Background, Border, Center, Color, Element, Font, Length, Point, PollSubNotifier, Rectangle,
    Renderer, Shadow, Size, Theme, Vector,
};
use nice_plug_iced::{EditorState, NiceGuiContext};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;

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
    LoseEditorFocus,

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
    SavePreset,
    LoadPreset,
    /// Save popup interactions.
    SaveNameChanged(String),
    ConfirmSave,
    /// Load popup interactions.
    SelectPreset(String),
    ConfirmLoad,
    LoadFromList(String),
    ClosePopup,
}

// Editor state

pub struct OscillaEditorState {
    pub params: Arc<OscillaParams>,
    pub wavetable_slot: Arc<WavetableSlot>,
    pub lua_source_slot: Arc<ArcSwap<String>>,
    #[cfg(feature = "time-buffer")]
    pub time_buffer_slot: Arc<TimeBufferSlot>,
    pub peak_output: Arc<AtomicF32>,
    pub scope_buffer: Arc<Mutex<(Box<[f32; SCOPE_SIZE]>, usize)>>,
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
    /// Popup state: None = closed, Some(true) = save, Some(false) = load.
    popup: Option<PopupState>,
}

enum PopupState {
    Save {
        name: String,
    },
    Load {
        files: Vec<String>,
        selected: Option<String>,
    },
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
        editor_state.editor_handle.set_wrap_enabled(false);
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
                popup: None,
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
            Message::LoseEditorFocus => {
                self.editor_state.editor_handle.lose_focus();
                Task::none()
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
                let mode = ScriptMode::from_param_value(
                    self.editor_state.params.script_mode.modulated_plain_value(),
                );
                *self.editor_state.params.wave_script.borrow_mut() = src.clone();
                self.editor_state.compiler.take_last_error();
                self.status_message = String::from("Compiling...");
                self.compile_ok = true;
                self.compile_pending = true;
                (self.editor_state.async_executor)(OscillaTask::CompileScript {
                    source: src,
                    mode,
                });

                self.editor_state.editor_handle.lose_focus();
                Task::none()
            }
            Message::VolumeGestured(g) => {
                iced_audio::param::set_nice_param(&p.volume, g, &setter);
                Task::done(Message::LoseEditorFocus)
            }
            Message::AttackGestured(g) => {
                iced_audio::param::set_nice_param(&p.attack, g, &setter);
                Task::done(Message::LoseEditorFocus)
            }
            Message::DecayGestured(g) => {
                iced_audio::param::set_nice_param(&p.decay, g, &setter);
                Task::done(Message::LoseEditorFocus)
            }
            Message::SustainGestured(g) => {
                iced_audio::param::set_nice_param(&p.sustain, g, &setter);
                Task::done(Message::LoseEditorFocus)
            }
            Message::ReleaseGestured(g) => {
                iced_audio::param::set_nice_param(&p.release, g, &setter);
                Task::done(Message::LoseEditorFocus)
            }
            Message::CutoffGestured(g) => {
                iced_audio::param::set_nice_param(&p.filter_cutoff, g, &setter);
                Task::done(Message::LoseEditorFocus)
            }
            Message::ResonanceGestured(g) => {
                iced_audio::param::set_nice_param(&p.filter_resonance, g, &setter);
                Task::done(Message::LoseEditorFocus)
            }
            Message::UnisonVoicesGestured(g) => {
                iced_audio::param::set_nice_param(&p.unison_voices, g, &setter);
                Task::done(Message::LoseEditorFocus)
            }
            Message::DetuneGestured(g) => {
                iced_audio::param::set_nice_param(&p.detune_cents, g, &setter);
                Task::done(Message::LoseEditorFocus)
            }
            Message::WidthGestured(g) => {
                iced_audio::param::set_nice_param(&p.stereo_width, g, &setter);
                Task::done(Message::LoseEditorFocus)
            }
            Message::GlideGestured(g) => {
                iced_audio::param::set_nice_param(&p.glide_time, g, &setter);
                Task::done(Message::LoseEditorFocus)
            }
            Message::FilterTypeChanged(ft) => {
                setter.begin_set_parameter(&p.filter_type);
                setter.set_parameter_normalized(&p.filter_type, ft.to_param_normalized());
                setter.end_set_parameter(&p.filter_type);
                Task::done(Message::LoseEditorFocus)
            }
            Message::ScriptModeChanged(mode) => {
                let val = mode.to_param_normalized();
                setter.begin_set_parameter(&p.script_mode);
                setter.set_parameter_normalized(&p.script_mode, val);
                setter.end_set_parameter(&p.script_mode);

                // Swap default template when switching modes.
                let current = self.editor_state.editor_handle.content();
                let wt_default = crate::script::DEFAULT_WAVETABLE_SCRIPT;
                let tb_default = crate::script::DEFAULT_TIMEBASED_SCRIPT;
                let replacement = match mode {
                    ScriptMode::TimeBased if current.trim() == wt_default.trim() => {
                        Some(tb_default)
                    }
                    ScriptMode::Wavetable if current.trim() == tb_default.trim() => {
                        Some(wt_default)
                    }
                    _ => None,
                };
                if let Some(template) = replacement {
                    let _ = self.editor_state.editor_handle.reset(template);
                }

                Task::done(Message::LoseEditorFocus)
            }
            Message::SavePreset => {
                self.popup = Some(PopupState::Save {
                    name: String::from("preset"),
                });
                Task::none()
            }
            Message::LoadPreset => {
                let files = list_presets();
                self.popup = Some(PopupState::Load {
                    files,
                    selected: None,
                });
                Task::none()
            }
            Message::SaveNameChanged(name) => {
                if let Some(PopupState::Save { name: ref mut n }) = self.popup {
                    *n = name;
                }
                Task::none()
            }
            Message::ConfirmSave => {
                if let Some(PopupState::Save { ref name }) = self.popup {
                    let preset = self.build_preset(name);
                    let text = preset.to_string();
                    if let Some(dir) = preset_dir() {
                        let _ = std::fs::create_dir_all(&dir);
                        let path = dir.join(format!("{name}.osc"));
                        match std::fs::write(&path, &text) {
                            Ok(()) => {
                                self.status_message = format!("Saved: {name}.osc");
                            }
                            Err(e) => {
                                self.status_message = format!("Save error: {e}");
                            }
                        }
                    }
                }
                self.popup = None;
                Task::none()
            }
            Message::SelectPreset(filename) => {
                if let Some(PopupState::Load { selected, .. }) = &mut self.popup {
                    *selected = Some(filename);
                }
                Task::none()
            }
            Message::ConfirmLoad => {
                let filename = match &self.popup {
                    Some(PopupState::Load {
                        selected: Some(f), ..
                    }) => f.clone(),
                    _ => return Task::none(),
                };
                self.popup = None;
                if let Some(dir) = preset_dir() {
                    let path = dir.join(&filename);
                    match std::fs::read_to_string(&path) {
                        Ok(text) => match Preset::parse(&text) {
                            Ok(preset) => {
                                self.apply_preset(&preset);
                                self.status_message = format!("Loaded: {filename}");
                            }
                            Err(e) => {
                                self.status_message = format!("Parse error: {e}");
                            }
                        },
                        Err(e) => {
                            self.status_message = format!("Read error: {e}");
                        }
                    }
                }
                Task::none()
            }
            Message::LoadFromList(filename) => {
                if let Some(dir) = preset_dir() {
                    let path = dir.join(&filename);
                    match std::fs::read_to_string(&path) {
                        Ok(text) => match Preset::parse(&text) {
                            Ok(preset) => {
                                self.apply_preset(&preset);
                                self.status_message = format!("Loaded: {filename}");
                            }
                            Err(e) => {
                                self.status_message = format!("Parse error: {e}");
                            }
                        },
                        Err(e) => {
                            self.status_message = format!("Read error: {e}");
                        }
                    }
                }
                self.popup = None;
                Task::none()
            }
            Message::ClosePopup => {
                self.popup = None;
                Task::none()
            }
        }
    }

    fn build_preset(&self, name: &str) -> Preset {
        let p = &self.editor_state.params;
        Preset {
            name: name.into(),
            wave_script: self.editor_state.editor_handle.content(),
            script_mode: match p.script_mode.modulated_plain_value() {
                0 => "wavetable".into(),
                _ => "time".into(),
            },
            filter_type: FilterType::from_param_value(p.filter_type.modulated_plain_value()),
            filter_cutoff: p.filter_cutoff.modulated_plain_value(),
            filter_resonance: p.filter_resonance.modulated_plain_value(),
            attack: p.attack.modulated_plain_value(),
            decay: p.decay.modulated_plain_value(),
            sustain: p.sustain.modulated_plain_value(),
            release: p.release.modulated_plain_value(),
            unison_voices: p.unison_voices.modulated_plain_value() as usize,
            detune_cents: p.detune_cents.modulated_plain_value(),
            stereo_width: p.stereo_width.modulated_plain_value(),
            volume: p.volume.modulated_plain_value(),
            glide: p.glide_time.modulated_plain_value(),
        }
    }

    fn apply_preset(&mut self, preset: &Preset) {
        let setter = self.nice_ctx.param_setter();
        let p = &self.editor_state.params;

        setter.begin_set_parameter(&p.volume);
        setter.set_parameter_normalized(&p.volume, preset.volume);
        setter.end_set_parameter(&p.volume);

        setter.begin_set_parameter(&p.attack);
        setter.set_parameter(&p.attack, preset.attack);
        setter.end_set_parameter(&p.attack);

        setter.begin_set_parameter(&p.decay);
        setter.set_parameter(&p.decay, preset.decay);
        setter.end_set_parameter(&p.decay);

        setter.begin_set_parameter(&p.sustain);
        setter.set_parameter_normalized(&p.sustain, preset.sustain);
        setter.end_set_parameter(&p.sustain);

        setter.begin_set_parameter(&p.release);
        setter.set_parameter(&p.release, preset.release);
        setter.end_set_parameter(&p.release);

        setter.begin_set_parameter(&p.filter_cutoff);
        setter.set_parameter(&p.filter_cutoff, preset.filter_cutoff);
        setter.end_set_parameter(&p.filter_cutoff);

        setter.begin_set_parameter(&p.filter_resonance);
        setter.set_parameter_normalized(&p.filter_resonance, preset.filter_resonance);
        setter.end_set_parameter(&p.filter_resonance);

        setter.begin_set_parameter(&p.filter_type);
        setter.set_parameter_normalized(&p.filter_type, preset.filter_type.to_param_normalized());
        setter.end_set_parameter(&p.filter_type);

        setter.begin_set_parameter(&p.unison_voices);
        setter
            .set_parameter_normalized(&p.unison_voices, (preset.unison_voices as f32 - 1.0) / 6.0);
        setter.end_set_parameter(&p.unison_voices);

        setter.begin_set_parameter(&p.detune_cents);
        setter.set_parameter_normalized(&p.detune_cents, preset.detune_cents / 50.0);
        setter.end_set_parameter(&p.detune_cents);

        setter.begin_set_parameter(&p.stereo_width);
        setter.set_parameter_normalized(&p.stereo_width, preset.stereo_width);
        setter.end_set_parameter(&p.stereo_width);

        setter.begin_set_parameter(&p.glide_time);
        let glide_norm = (preset.glide / 5.0).clamp(0.0, 1.0);
        setter.set_parameter_normalized(&p.glide_time, glide_norm);
        setter.end_set_parameter(&p.glide_time);

        // Set script mode.
        let mode_val: f32 = if preset.script_mode == "time" {
            1.0
        } else {
            0.0
        };
        setter.begin_set_parameter(&p.script_mode);
        setter.set_parameter_normalized(&p.script_mode, mode_val / 1.0);
        setter.end_set_parameter(&p.script_mode);

        // Update the code editor with the preset's wave script.
        let _ = self.editor_state.editor_handle.reset(&preset.wave_script);
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

        let mode = ScriptMode::from_param_value(p.script_mode.modulated_plain_value());

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

        let save_btn = button(btn_text("Save"))
            .on_press(Message::SavePreset)
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
        let load_btn = button(btn_text("Load"))
            .on_press(Message::LoadPreset)
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

        let editor_panel = container(
            column![
                heading("SCRIPT"),
                mode_column,
                editor_container,
                row![
                    compile_btn,
                    status_text(&self.status_message, ok),
                    Space::new().width(Length::Fill),
                    save_btn,
                    load_btn,
                ]
                .spacing(8)
                .align_y(Center),
            ]
            .spacing(8),
        )
        .style(|_| section_panel())
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill);

        // Waveform preview — read scope buffer in temporal order.
        let scope_snapshot = {
            let guard = self.editor_state.scope_buffer.lock().unwrap();
            let (buf, pos) = &*guard;
            let mut ordered = Box::new([0.0f32; SCOPE_SIZE]);
            // Reorder: samples after pos come first, then samples before pos.
            let n = SCOPE_SIZE;
            ordered[..n - pos].copy_from_slice(&buf[*pos..]);
            ordered[n - pos..].copy_from_slice(&buf[..*pos]);
            ordered
        };
        let preview = Canvas::new(WaveformPreview {
            wavetable: self.editor_state.wavetable_slot.load(),
            scope_snapshot,
            mode,
            accent: ACCENT,
        })
        .width(Length::Fill)
        .height(Length::Fill);

        let preview_panel = container(
            mouse_area(
                container(preview)
                    .style(|_| section_panel())
                    .padding(12)
                    .width(Length::Fill),
            )
            .on_press(Message::LoseEditorFocus),
        );

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
        let ft = FilterType::from_param_value(p.filter_type.modulated_plain_value());
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
        let footer = container(
            row![
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
            ]
            .align_y(Center),
        )
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

        let right = mouse_area(
            column![
                envelope.height(Length::Fill),
                filter.height(Length::Fill),
                unison.height(Length::Fill),
                master.height(Length::Fill),
                footer,
            ]
            .spacing(10)
            .width(Length::FillPortion(2))
            .height(Length::Fill),
        )
        .on_press(Message::LoseEditorFocus);

        let main = container(row![left, right].spacing(12).padding(12))
            .style(|_| container::Style {
                background: Some(Background::Color(BG_DEEP)),
                border: Border::default(),
                shadow: Shadow::default(),
                text_color: None,
                snap: false,
            })
            .width(Length::Fill)
            .height(Length::Fill);

        // Show popup overlay when one is active.
        if let Some(ref popup) = self.popup {
            popup_view(popup)
        } else {
            main.into()
        }
    }
}

/// Render a popup panel (save or load).
fn popup_view(popup: &PopupState) -> Element<'static, Message> {
    let card = match popup {
        PopupState::Save { name } => {
            let input = text_input("Preset name", name)
                .on_input(Message::SaveNameChanged)
                .on_submit(Message::ConfirmSave)
                .padding(8)
                .size(14);

            let buttons = row![
                Space::new().width(Length::Fill),
                button(btn_text("Cancel"))
                    .on_press(Message::ClosePopup)
                    .padding([6, 18])
                    .style(|_theme, status| match status {
                        button::Status::Active => btn_style(false),
                        button::Status::Hovered | button::Status::Pressed => {
                            btn_style(true)
                        }
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
                    }),
                button(btn_text("Save"))
                    .on_press(Message::ConfirmSave)
                    .padding([6, 18])
                    .style(|_theme, status| match status {
                        button::Status::Active => btn_style(false),
                        button::Status::Hovered | button::Status::Pressed => {
                            btn_style(true)
                        }
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
                    }),
            ]
            .spacing(8);

            column![heading("Save Preset"), input, buttons]
                .spacing(12)
                .width(300)
        }
        PopupState::Load { files, selected } => {
            let mut list = column![].spacing(2);
            if files.is_empty() {
                list = list.push(text("No presets found").size(13).color(FG_DIM));
            }
            for f in files.iter() {
                let name = f.clone();
                let is_selected = selected.as_deref() == Some(&name);
                list = list.push(
                    button(text(name.clone()).size(13).color(if is_selected {
                        ACCENT
                    } else {
                        FG_TEXT
                    }))
                    .on_press(Message::SelectPreset(name))
                    .padding([6, 10])
                    .width(Length::Fill)
                    .style(move |_theme, _status| button::Style {
                        background: if is_selected {
                            Some(Background::Color(ACCENT_SOFT))
                        } else {
                            None
                        },
                        border: Border::default(),
                        text_color: if is_selected { ACCENT } else { FG_TEXT },
                        shadow: Shadow::default(),
                        snap: false,
                    }),
                );
            }

            let buttons = row![
                Space::new().width(Length::Fill),
                button(btn_text("Cancel"))
                    .on_press(Message::ClosePopup)
                    .padding([6, 18])
                    .style(|_theme, status| match status {
                        button::Status::Active => btn_style(false),
                        button::Status::Hovered | button::Status::Pressed => {
                            btn_style(true)
                        }
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
                    }),
                button(btn_text("Load"))
                    .on_press(Message::ConfirmLoad)
                    .padding([6, 18])
                    .style(|_theme, status| match status {
                        button::Status::Active => btn_style(false),
                        button::Status::Hovered | button::Status::Pressed => {
                            btn_style(true)
                        }
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
                    }),
            ]
            .spacing(8);

            let scroll = scrollable(
                container(list)
                    .style(|_| container::Style {
                        background: Some(Background::Color(BG_DEEP)),
                        border: Border {
                            color: BORDER,
                            width: 1.0,
                            radius: rad(3.0),
                        },
                        ..Default::default()
                    })
                    .padding(8)
                    .width(Length::Fill),
            )
            .height(250);

            column![heading("Load Preset"), scroll, buttons]
                .spacing(12)
                .width(300)
        }
    };

    let card = container(card)
        .style(|_| container::Style {
            background: Some(Background::Color(SURFACE)),
            border: Border {
                color: BORDER,
                width: 1.0,
                radius: rad(6.0),
            },
            ..Default::default()
        })
        .padding(20);

    container(column![
        Space::new().height(Length::Fill),
        row![
            Space::new().width(Length::Fill),
            card,
            Space::new().width(Length::Fill),
        ]
        .height(Length::Shrink),
        Space::new().height(Length::Fill),
    ])
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.7))),
        ..Default::default()
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Path to the preset directory: `Documents/Oscilla/`.
fn preset_dir() -> Option<std::path::PathBuf> {
    Some(dirs::document_dir()?.join("Oscilla"))
}

/// List all `.osc` files in the preset directory.
fn list_presets() -> Vec<String> {
    let dir = match preset_dir() {
        Some(d) => d,
        None => return vec![],
    };
    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".osc") {
                files.push(name);
            }
        }
    }
    files.sort();
    files
}

// Waveform preview

struct WaveformPreview {
    wavetable: SharedWavetable,
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
