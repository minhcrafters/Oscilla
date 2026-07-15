//! Oscilla — programmable oscillator synthesizer plugin.
//!
//! A cross-platform VST3/CLAP software synthesizer based around a custom
//! lightweight scripting language that lets users program their own
//! oscillator waveforms. Think of it as a "shader language for sound."

pub mod dsp;
pub mod gui;
pub mod preset;
pub mod script;
pub mod wavetable;

use crate::dsp::filter::FilterType;
use crate::dsp::{SynthEngine, TimeBufferSlot, WavetableSlot};
use crate::gui::EditorHandle;
use crate::script::{ScriptCompiler, ScriptMode};
use crate::wavetable::{default_time_buffer, default_wavetable};
use atomic_refcell::AtomicRefCell;
use iced_code_editor::CodeEditor;
use nice_plug::prelude::*;
use nice_plug_iced::iced::PollSubNotifier;
use std::sync::Arc;
use std::sync::atomic::Ordering;

// Plugin

pub struct Oscilla {
    params: Arc<OscillaParams>,
    engine: SynthEngine,
    wavetable_slot: Arc<WavetableSlot>,
    time_buffer_slot: Arc<TimeBufferSlot>,
    compiler: Arc<ScriptCompiler>,
    peak_output: Arc<AtomicF32>,
    sample_rate: Arc<AtomicF32>,
    notifier: PollSubNotifier,
    last_compiled: String,
}

// Background tasks

#[derive(Debug)]
#[allow(dead_code)]
pub enum OscillaTask {
    CompileScript { source: String, mode: ScriptMode },
}

impl Default for Oscilla {
    fn default() -> Self {
        let sample_rate = Arc::new(AtomicF32::new(44100.0));
        let default_table = default_wavetable();
        let default_time_buf = default_time_buffer(44100.0);

        Self {
            params: Arc::new(OscillaParams::default()),
            engine: SynthEngine::new(sample_rate.load(Ordering::Relaxed)),
            wavetable_slot: Arc::new(WavetableSlot::new(Arc::new(default_table))),
            time_buffer_slot: Arc::new(TimeBufferSlot::new(Arc::new(default_time_buf))),
            compiler: Arc::new(ScriptCompiler::new()),
            peak_output: Arc::new(AtomicF32::new(0.0)),
            notifier: PollSubNotifier::new(),
            sample_rate,
            last_compiled: String::new(),
        }
    }
}

// Parameters

/// Helper: parse a string as f32.
fn parse_f32(s: &str) -> Option<f32> {
    s.parse().ok()
}

#[derive(Params)]
pub struct OscillaParams {
    #[persist = "window-state"]
    pub window_state: Arc<nice_plug_iced::WindowState>,

    #[id = "volume"]
    pub volume: FloatParam,

    #[id = "attack"]
    pub attack: FloatParam,

    #[id = "decay"]
    pub decay: FloatParam,

    #[id = "sustain"]
    pub sustain: FloatParam,

    #[id = "release"]
    pub release: FloatParam,

    #[id = "cutoff"]
    pub filter_cutoff: FloatParam,

    #[id = "resonance"]
    pub filter_resonance: FloatParam,

    #[id = "filter_type"]
    pub filter_type: IntParam,

    #[id = "unison"]
    pub unison_voices: FloatParam,

    #[id = "detune"]
    pub detune_cents: FloatParam,

    #[id = "width"]
    pub stereo_width: FloatParam,

    #[id = "glide"]
    pub glide_time: FloatParam,

    /// 0 = Wavetable (x-based), 1 = Time-based (t-based).
    #[id = "script_mode"]
    pub script_mode: IntParam,

    /// The wave script is persisted but not automatable.
    #[persist = "wave-script"]
    pub wave_script: AtomicRefCell<String>,
}

impl Default for OscillaParams {
    fn default() -> Self {
        const W: u32 = 820;
        const H: u32 = 540;

        Self {
            window_state: nice_plug_iced::WindowState::from_logical_size(W, H),

            volume: FloatParam::new("Volume", 0.8, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(5.0))
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            attack: FloatParam::new(
                "Attack",
                0.01,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 4.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(5.0))
            .with_unit(" s")
            .with_value_to_string(Arc::new(|v| format!("{v:.3}")))
            .with_string_to_value(Arc::new(parse_f32)),

            decay: FloatParam::new(
                "Decay",
                0.2,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 4.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(5.0))
            .with_unit(" s")
            .with_value_to_string(Arc::new(|v| format!("{v:.3}")))
            .with_string_to_value(Arc::new(parse_f32)),

            sustain: FloatParam::new("Sustain", 0.7, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(5.0))
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            release: FloatParam::new(
                "Release",
                0.4,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 8.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(5.0))
            .with_unit(" s")
            .with_value_to_string(Arc::new(|v| format!("{v:.3}")))
            .with_string_to_value(Arc::new(parse_f32)),

            filter_cutoff: FloatParam::new(
                "Cutoff",
                20000.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20000.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(10.0))
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_hz_then_khz(0))
            .with_string_to_value(formatters::s2v_f32_hz_then_khz()),

            filter_resonance: FloatParam::new(
                "Resonance",
                0.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 0.99,
                },
            )
            .with_smoother(SmoothingStyle::Linear(5.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            filter_type: IntParam::new("Filter Type", 0, IntRange::Linear { min: 0, max: 2 }),

            unison_voices: FloatParam::new(
                "Unison",
                1.0,
                FloatRange::Linear { min: 1.0, max: 7.0 },
            )
            .with_smoother(SmoothingStyle::Linear(2.0))
            .with_value_to_string(Arc::new(|v| format!("{}", v as i32)))
            .with_string_to_value(Arc::new(parse_f32)),

            detune_cents: FloatParam::new(
                "Detune",
                10.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 50.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(5.0))
            .with_unit(" cents")
            .with_value_to_string(Arc::new(|v| format!("{v:.0}")))
            .with_string_to_value(Arc::new(parse_f32)),

            stereo_width: FloatParam::new("Width", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(5.0))
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            glide_time: FloatParam::new(
                "Glide",
                0.05,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_smoother(SmoothingStyle::Linear(5.0))
            .with_unit(" s")
            .with_value_to_string(Arc::new(|v| format!("{v:.3}")))
            .with_string_to_value(Arc::new(parse_f32)),

            script_mode: IntParam::new("Script Mode", 0, IntRange::Linear { min: 0, max: 1 }),

            wave_script: AtomicRefCell::new(String::from("sin(x)")),
        }
    }
}

// Plugin trait

impl Plugin for Oscilla {
    const NAME: &'static str = "Oscilla";
    const VENDOR: &'static str = "pychael";
    const URL: &'static str = "https://pychael.me";
    const EMAIL: &'static str = "dev@pychael.me";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = OscillaTask;

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn task_executor(&mut self) -> Box<dyn Fn(Self::BackgroundTask) + Send> {
        let wt_slot = self.wavetable_slot.clone();
        let tb_slot = self.time_buffer_slot.clone();
        let compiler = self.compiler.clone();
        let notifier = self.notifier.clone();
        let sr = self.sample_rate.clone();

        Box::new(move |task| match task {
            OscillaTask::CompileScript { source, mode } => match compiler.compile(&source, mode) {
                Ok(()) => {
                    let rate = sr.load(Ordering::Relaxed);
                    match compiler.generate_both(mode, rate) {
                        Ok((wt_opt, tb_opt)) => {
                            if let Some(wt) = wt_opt {
                                wt_slot.store(wt);
                            }
                            if let Some(tb) = tb_opt {
                                tb_slot.store(tb);
                            }
                            notifier.notify();
                        }
                        Err(e) => {
                            log::error!("Oscilla: buffer generation: {e}");
                            compiler.store_error(e);
                            notifier.notify();
                        }
                    }
                }
                Err(e) => {
                    log::error!("Oscilla: script compile: {e}");
                    compiler.store_error(e);
                    notifier.notify();
                }
            },
        })
    }

    fn editor(&mut self, async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let initial_text = self.params.wave_script.borrow().clone();
        let executor: Arc<dyn Fn(OscillaTask) + Send + Sync> =
            Arc::new(move |task| async_executor.execute_background(task));
        let editor_state = gui::OscillaEditorState {
            params: self.params.clone(),
            wavetable_slot: self.wavetable_slot.clone(),
            time_buffer_slot: self.time_buffer_slot.clone(),
            peak_output: self.peak_output.clone(),
            notifier: self.notifier.clone(),
            compiler: self.compiler.clone(),
            sample_rate: self.sample_rate.clone(),
            async_executor: executor,
            script_content: EditorHandle(CodeEditor::new(&initial_text, "rs")),
        };

        nice_plug_iced::create_iced_editor(
            self.params.window_state.clone(),
            editor_state,
            self.notifier.clone(),
            Default::default(),
            |es, ctx| {
                nice_plug_iced::application(
                    es,
                    ctx,
                    gui::OscillaGui::new,
                    gui::OscillaGui::update,
                    gui::OscillaGui::view,
                )
                .theme(gui::OscillaGui::theme)
                .subscription(|_| nice_plug_iced::iced::poll_events().map(|_| gui::Message::Poll))
                .run()
            },
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate
            .store(buffer_config.sample_rate, Ordering::Relaxed);
        self.engine.set_sample_rate(buffer_config.sample_rate);
        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let mut table = self.wavetable_slot.load();
        let mut time_buf = self.time_buffer_slot.load();
        let mut next_event = context.next_event();
        let mut had_events = false;
        let mut peak = 0.0f32;

        let mode = match self.params.script_mode.modulated_plain_value() {
            0 => ScriptMode::Wavetable,
            _ => ScriptMode::TimeBased,
        };
        self.engine.script_mode = mode;

        let script = self.params.wave_script.borrow().clone();
        if script != self.last_compiled {
            if let Err(e) = self.compiler.compile(&script, mode) {
                log::error!("Oscilla: init compile error: {e}");
                self.compiler.store_error(e);
            } else {
                match self
                    .compiler
                    .generate_both(mode, self.sample_rate.load(Ordering::Relaxed))
                {
                    Ok((wt_opt, tb_opt)) => {
                        if let Some(wt) = wt_opt {
                            self.wavetable_slot.store(wt);
                        }
                        if let Some(tb) = tb_opt {
                            self.time_buffer_slot.store(tb);
                        }
                        self.last_compiled = script;
                    }
                    Err(e) => {
                        log::error!("Oscilla: init buffer generation: {e}");
                        self.compiler.store_error(e);
                    }
                }
            }
            table = self.wavetable_slot.load();
            time_buf = self.time_buffer_slot.load();
        }

        let ft = match self.params.filter_type.modulated_plain_value() {
            0 => FilterType::LowPass,
            1 => FilterType::HighPass,
            _ => FilterType::BandPass,
        };
        self.engine.update_block_params(
            self.params.attack.smoothed.next(),
            self.params.decay.smoothed.next(),
            self.params.sustain.smoothed.next(),
            self.params.release.smoothed.next(),
            self.params.filter_cutoff.smoothed.next(),
            self.params.filter_resonance.smoothed.next(),
            ft,
            self.params.unison_voices.smoothed.next() as usize,
            self.params.detune_cents.smoothed.next(),
            self.params.stereo_width.smoothed.next(),
            self.params.glide_time.smoothed.next(),
        );

        // Single-pass: collect MIDI + render audio together
        for (sample_id, channel_samples) in buffer.iter_samples().enumerate() {
            // Process any MIDI events at this sample position.
            while let Some(event) = next_event {
                if event.timing() > sample_id as u32 {
                    break;
                }
                match event {
                    NoteEvent::NoteOn { note, velocity, .. } => {
                        self.engine.note_on(note, velocity);
                        had_events = true;
                    }
                    NoteEvent::NoteOff { note, .. } => {
                        self.engine.note_off(note);
                        had_events = true;
                    }
                    _ => {}
                }
                next_event = context.next_event();
            }

            // Dispatch voice allocation when events arrive.
            if had_events {
                self.engine.process_events();
                had_events = false;
            }

            // Advance volume smoother per-sample for sample-accurate automation.
            self.engine.set_volume(self.params.volume.smoothed.next());

            // Render one sample.
            let (l, r) = self.engine.process_sample(&table, &time_buf);

            let mut ch = channel_samples.into_iter();
            if let Some(s) = ch.next() {
                *s = l;
            }
            if let Some(s) = ch.next() {
                *s = r;
            }

            let abs = l.abs().max(r.abs());
            if abs > peak {
                peak = abs;
            }
        }

        // Update peak meter
        if self.params.window_state.is_open() {
            let cur = self.peak_output.load(Ordering::Relaxed);
            let new = if peak > cur {
                peak
            } else if peak < 0.0001 {
                // Silence: drop immediately.
                0.0
            } else {
                // ~30 dB/s decay — matches typical digital peak ballistics.
                cur * 0.99 + peak * 0.01
            };
            if (cur - new).abs() > 0.0001 {
                self.peak_output.store(new, Ordering::Relaxed);
                self.notifier.notify();
            }
        }

        ProcessStatus::KeepAlive
    }

    fn deactivate(&mut self) {}
}

// Format exports

impl ClapPlugin for Oscilla {
    const CLAP_ID: &'static str = "me.pychael.oscilla.synth";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Programmable oscillator synthesizer — a shader language for sound");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Stereo,
        ClapFeature::Synthesizer,
    ];
}

impl Vst3Plugin for Oscilla {
    const VST3_CLASS_ID: [u8; 16] = *b"OscillaSynth001!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nice_export_clap!(Oscilla);
nice_export_vst3!(Oscilla);
