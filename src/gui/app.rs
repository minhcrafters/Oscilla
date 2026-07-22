use super::knob_style;
use super::popup::{PopupState, list_presets, popup_view, preset_dir};
use super::theme::*;
use super::waveform::waveform_canvas;
use super::{Message, OscillaEditorState};
use crate::OscillaTask;
use crate::dsp::filter::FilterType;
use crate::preset::Preset;
use crate::script::ScriptMode;
use crate::wavetable::SCOPE_SIZE;
use iced_audio::param::nice_to_iced;
use iced_audio::{Gesture, Knob};
use iced_code_editor::CodeEditor;
use iced_code_editor::IndentStyle;
use nice_plug::prelude::*;
use nice_plug_iced::iced::Task;
use nice_plug_iced::iced::{
    Background, Border, Center, Element, Font, Length, Shadow, Theme,
    font, theme::Palette,
    widget::{
        Column, Container, Row, Space, button, column, container, mouse_area, pick_list, row,
        text,
    },
};
use nice_plug_iced::{EditorState, NiceGuiContext};
use std::sync::atomic::Ordering;

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

pub struct OscillaGui {
    editor_state: EditorState<OscillaEditorState>,
    nice_ctx: NiceGuiContext,
    status_message: String,
    compile_ok: bool,
    peak_output_db: f32,
    compile_pending: bool,
    popup: Option<PopupState>,
}

impl OscillaGui {
    pub fn new(
        mut editor_state: EditorState<OscillaEditorState>,
        nice_ctx: NiceGuiContext,
    ) -> (Self, Task<Message>) {
        editor_state.editor_handle.set_font(Font::MONOSPACE);
        editor_state.editor_handle.set_font_size(13.0, true);
        editor_state.editor_handle.set_folding_enabled(false);
        editor_state.editor_handle.set_wrap_enabled(false);
        editor_state.editor_handle.set_theme(vscode_editor_style());
        editor_state
            .editor_handle
            .set_indent_style(IndentStyle::Spaces(2));

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
                self.load_preset_file(&filename);
                Task::none()
            }
            Message::LoadFromList(filename) => {
                self.load_preset_file(&filename);
                self.popup = None;
                Task::none()
            }
            Message::ClosePopup => {
                self.popup = None;
                Task::none()
            }
        }
    }

    fn load_preset_file(&mut self, filename: &str) {
        if let Some(dir) = preset_dir() {
            let path = dir.join(filename);
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

        let mode_val: f32 = if preset.script_mode == "time" {
            1.0
        } else {
            0.0
        };
        setter.begin_set_parameter(&p.script_mode);
        setter.set_parameter_normalized(&p.script_mode, mode_val);
        setter.end_set_parameter(&p.script_mode);

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
            .style(accent_btn_style);

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
            .style(accent_btn_style);
        let load_btn = button(btn_text("Load"))
            .on_press(Message::LoadPreset)
            .padding([6, 18])
            .style(accent_btn_style);

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

        let scope_snapshot = {
            let guard = self.editor_state.scope_buffer.lock().unwrap();
            let (buf, pos) = &*guard;
            let mut ordered = Box::new([0.0f32; SCOPE_SIZE]);
            let n = SCOPE_SIZE;
            ordered[..n - pos].copy_from_slice(&buf[*pos..]);
            ordered[n - pos..].copy_from_slice(&buf[..*pos]);
            ordered
        };
        let preview = waveform_canvas(
            self.editor_state.wavetable_slot.load(),
            scope_snapshot,
            mode,
        );

        let preview_panel = container(
            mouse_area(
                container(preview)
                    .style(|_| section_panel())
                    .padding(12)
                    .width(Length::Fill),
            )
            .on_press(Message::LoseEditorFocus),
        );

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

        if let Some(ref popup) = self.popup {
            popup_view(popup)
        } else {
            main.into()
        }
    }
}
