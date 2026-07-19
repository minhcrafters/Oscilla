//! Oscilla — programmable oscillator synthesizer plugin.
//!
//! A cross-platform VST3/CLAP software synthesizer based around a custom
//! lightweight Lua scripting language that lets users program their own
//! oscillator waveforms. Think of it as a "shader language for sound."

pub mod dsp;
pub mod gui;
pub mod preset;
pub mod script;
pub mod wavetable;

#[cfg(feature = "time-buffer")]
use crate::dsp::TimeBufferSlot;
use crate::dsp::filter::FilterType;
use crate::dsp::{SynthEngine, WavetableSlot};
use crate::gui::EditorHandle;
use crate::script::{LuaContext, ScriptCompiler, ScriptMode};
use crate::wavetable::{SCOPE_SIZE, default_wavetable};
use arc_swap::ArcSwap;
use atomic_refcell::AtomicRefCell;
use iced_code_editor::CodeEditor;
use nice_plug::prelude::*;
use nice_plug_iced::iced::PollSubNotifier;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

// Plugin

pub struct Oscilla {
    params: Arc<OscillaParams>,
    engine: SynthEngine,
    wavetable_slot: Arc<WavetableSlot>,
    /// Compiled Lua source code for time-based mode, swapped atomically.
    lua_source_slot: Arc<ArcSwap<String>>,
    /// Pre-rendered time buffer (only with `time-buffer` feature).
    #[cfg(feature = "time-buffer")]
    time_buffer_slot: Arc<TimeBufferSlot>,
    /// Cached compiled Lua context for time-based mode, avoiding per-block recompilation.
    /// Only ever accessed on the audio thread.
    #[cfg(not(feature = "time-buffer"))]
    cached_lua_ctx: Option<CachedLua>,
    compiler: Arc<ScriptCompiler>,
    peak_output: Arc<AtomicF32>,
    scope_buffer: Arc<Mutex<(Box<[f32; SCOPE_SIZE]>, usize)>>,
    /// Next write position in the scope ring buffer, persisted across audio blocks.
    next_scope_pos: usize,
    sample_rate: Arc<AtomicF32>,
    notifier: PollSubNotifier,
}

/// Wrapper around `LuaContext` that asserts `Send` and `Sync`.
///
/// `LuaContext` contains `Rc` and is `!Send`, but this wrapper is only
/// accessed from the audio thread. Follows the same pattern as `EditorHandle`.
#[cfg(not(feature = "time-buffer"))]
struct CachedLua(String, LuaContext);

#[cfg(not(feature = "time-buffer"))]
unsafe impl Send for CachedLua {}
#[cfg(not(feature = "time-buffer"))]
unsafe impl Sync for CachedLua {}

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

        Self {
            params: Arc::new(OscillaParams::default()),
            engine: SynthEngine::new(sample_rate.load(Ordering::Relaxed)),
            wavetable_slot: Arc::new(WavetableSlot::new(Arc::new(default_table))),
            lua_source_slot: Arc::new(ArcSwap::from_pointee(String::from(
                crate::script::DEFAULT_TIMEBASED_SCRIPT,
            ))),
            #[cfg(feature = "time-buffer")]
            time_buffer_slot: {
                let buf =
                    crate::script::generate_time_buffer("math.sin(t * math.pi * 2 * 440)", 44100.0)
                        .unwrap_or_else(|_| vec![0.0f32; 44100]);
                Arc::new(TimeBufferSlot::new(buf))
            },
            #[cfg(not(feature = "time-buffer"))]
            cached_lua_ctx: None,
            compiler: Arc::new(ScriptCompiler::new()),
            peak_output: Arc::new(AtomicF32::new(0.0)),
            scope_buffer: Arc::new(Mutex::new((Box::new([0.0f32; SCOPE_SIZE]), 0))),
            next_scope_pos: 0,
            notifier: PollSubNotifier::new(),
            sample_rate,
        }
    }
}

// Parameters

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

    #[id = "script_mode"]
    pub script_mode: IntParam,

    #[persist = "wave-script"]
    pub wave_script: AtomicRefCell<String>,
}

impl Default for OscillaParams {
    fn default() -> Self {
        const W: u32 = 820;
        const H: u32 = 540;

        Self {
            window_state: nice_plug_iced::WindowState::from_logical_size(W, H),

            volume: FloatParam::new("Volume", 0.75, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(5.0))
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            attack: FloatParam::new(
                "Attack",
                0.01,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 10.0,
                    factor: FloatRange::skew_factor(-1.0),
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
                    max: 10.0,
                    factor: FloatRange::skew_factor(-1.0),
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
                    max: 10.0,
                    factor: FloatRange::skew_factor(-1.0),
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
                    factor: FloatRange::skew_factor(-1.0),
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
                    max: 5.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(5.0))
            .with_unit(" s")
            .with_value_to_string(Arc::new(|v| format!("{v:.3}")))
            .with_string_to_value(Arc::new(parse_f32)),

            script_mode: IntParam::new("Script Mode", 0, IntRange::Linear { min: 0, max: 1 }),

            wave_script: AtomicRefCell::new(String::from(crate::script::DEFAULT_WAVETABLE_SCRIPT)),
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
        #[cfg(not(feature = "time-buffer"))]
        let lua_slot = self.lua_source_slot.clone();
        #[cfg(feature = "time-buffer")]
        let tb_slot = self.time_buffer_slot.clone();
        let compiler = self.compiler.clone();
        let notifier = self.notifier.clone();
        #[cfg(feature = "time-buffer")]
        let sr = self.sample_rate.clone();

        Box::new(move |task| match task {
            OscillaTask::CompileScript { source, mode } => match compiler.compile(&source, mode) {
                Ok(()) => compile_backend(
                    &source,
                    mode,
                    &wt_slot,
                    #[cfg(not(feature = "time-buffer"))]
                    &lua_slot,
                    #[cfg(feature = "time-buffer")]
                    &tb_slot,
                    &compiler,
                    &notifier,
                    #[cfg(feature = "time-buffer")]
                    &sr,
                ),
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
            lua_source_slot: self.lua_source_slot.clone(),
            #[cfg(feature = "time-buffer")]
            time_buffer_slot: self.time_buffer_slot.clone(),
            peak_output: self.peak_output.clone(),
            scope_buffer: self.scope_buffer.clone(),
            notifier: self.notifier.clone(),
            compiler: self.compiler.clone(),
            sample_rate: self.sample_rate.clone(),
            async_executor: executor,
            editor_handle: EditorHandle(CodeEditor::new(&initial_text, "lua")),
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
                .subscription(|_| {
                    use nice_plug_iced::iced::keyboard::{self, key::Key};

                    let poll = nice_plug_iced::iced::poll_events().map(|_| gui::Message::Poll);
                    let keys = nice_plug_iced::iced::event::listen().filter_map(|event| {
                        if let nice_plug_iced::iced::Event::Keyboard(
                            keyboard::Event::KeyPressed { key, modifiers, .. },
                        ) = event
                        {
                            match key {
                                Key::Character(c) if c == "a" && modifiers.control() => {
                                    Some(gui::Message::SelectAll)
                                }
                                _ => None,
                            }
                        } else {
                            None
                        }
                    });

                    nice_plug_iced::iced::Subscription::batch([poll, keys])
                })
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
        let table = self.wavetable_slot.load();
        let mut next_event = context.next_event();
        let mut had_events = false;
        let mut peak = 0.0f32;

        let mode = self.apply_block_params();

        #[cfg(feature = "time-buffer")]
        let time_buf_guard = self.time_buffer_slot.load();

        // For time-based mode, compile the Lua context when source changes.
        #[cfg(not(feature = "time-buffer"))]
        if mode == ScriptMode::TimeBased {
            let src = self.lua_source_slot.load_full();
            let needs_recompile = match &self.cached_lua_ctx {
                Some(cached) => cached.0 != *src,
                None => true,
            };
            if needs_recompile && let Ok(ctx) = LuaContext::compile(&src, ScriptMode::TimeBased) {
                self.cached_lua_ctx = Some(CachedLua(src.to_string(), ctx));
            }
        }

        let mut scope_pos = self.next_scope_pos;
        let mut scope_local: [f32; SCOPE_SIZE] = [0.0; SCOPE_SIZE];
        let scope_local_start = scope_pos;

        for (sample_id, channel_samples) in buffer.iter_samples().enumerate() {
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

            if had_events {
                self.engine.process_events();
                had_events = false;
            }

            self.engine.set_volume(self.params.volume.smoothed.next());

            #[cfg(feature = "time-buffer")]
            let (l, r) = self.engine.process_sample_buf(
                &table,
                match mode {
                    ScriptMode::TimeBased => Some(&time_buf_guard),
                    ScriptMode::Wavetable => None,
                },
            );
            #[cfg(not(feature = "time-buffer"))]
            let lua_ref = self.cached_lua_ctx.as_ref().map(|cached| &cached.1);
            #[cfg(not(feature = "time-buffer"))]
            let (l, r) = self.engine.process_sample(&table, lua_ref);

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

            scope_local[scope_pos] = (l + r) * 0.5;
            scope_pos = (scope_pos + 1) % SCOPE_SIZE;
        }

        self.next_scope_pos = scope_pos;
        self.flush_scope_buffer(&scope_local, scope_pos, scope_local_start);

        self.update_peak_meter(peak);

        ProcessStatus::KeepAlive
    }

    fn deactivate(&mut self) {}
}

impl Oscilla {
    /// Read mode and filter type from params and apply all block parameters.
    /// Returns the current script mode.
    fn apply_block_params(&mut self) -> ScriptMode {
        let mode = ScriptMode::from_param_value(self.params.script_mode.modulated_plain_value());
        self.engine.script_mode = mode;

        let ft = FilterType::from_param_value(self.params.filter_type.modulated_plain_value());
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
        mode
    }

    /// Copy the local scope buffer to the shared ring buffer behind the mutex.
    fn flush_scope_buffer(
        &mut self,
        scope_local: &[f32; SCOPE_SIZE],
        scope_pos: usize,
        scope_local_start: usize,
    ) {
        let mut guard = self.scope_buffer.lock().unwrap();
        let (ref mut buf, ref mut pos) = *guard;
        *pos = scope_pos;
        if scope_pos > scope_local_start {
            buf[scope_local_start..scope_pos]
                .copy_from_slice(&scope_local[scope_local_start..scope_pos]);
        } else {
            buf[scope_local_start..].copy_from_slice(&scope_local[scope_local_start..]);
            buf[..scope_pos].copy_from_slice(&scope_local[..scope_pos]);
        }
    }

    /// Update peak output with envelope following.
    fn update_peak_meter(&self, peak: f32) {
        if self.params.window_state.is_open() {
            self.notifier.notify();

            let cur = self.peak_output.load(Ordering::Relaxed);
            let new = if peak > cur {
                peak
            } else if peak < 0.0001 {
                0.0
            } else {
                cur * 0.99 + peak * 0.01
            };
            if (cur - new).abs() > 0.0001 {
                self.peak_output.store(new, Ordering::Relaxed);
            }
        }
    }
}

/// Generate a wavetable by evaluating a Lua function at 2048 phase points.
fn generate_wavetable_from_lua(ctx: &LuaContext) -> crate::wavetable::SharedWavetable {
    let inv_n = 1.0 / crate::wavetable::WAVETABLE_SIZE as f32;
    let mut data = Box::new([0.0f32; crate::wavetable::WAVETABLE_SIZE]);
    for (i, s) in data.iter_mut().enumerate() {
        let x = i as f32 * inv_n * 2.0 * std::f32::consts::PI;
        *s = ctx.eval_x(x);
    }
    crate::wavetable::remove_dc(&mut data);
    crate::wavetable::normalize(&mut data);
    crate::wavetable::band_limit(&mut data, 200);
    crate::wavetable::normalize(&mut data);
    Arc::new(data)
}

/// Perform mode-specific compilation on a background thread.
#[allow(clippy::too_many_arguments)]
fn compile_backend(
    source: &str,
    mode: ScriptMode,
    wt_slot: &WavetableSlot,
    #[cfg(not(feature = "time-buffer"))] lua_slot: &ArcSwap<String>,
    #[cfg(feature = "time-buffer")] tb_slot: &TimeBufferSlot,
    compiler: &ScriptCompiler,
    notifier: &PollSubNotifier,
    #[cfg(feature = "time-buffer")] sr: &AtomicF32,
) {
    match mode {
        ScriptMode::Wavetable => match LuaContext::compile(source, ScriptMode::Wavetable) {
            Ok(ctx) => {
                wt_slot.store(generate_wavetable_from_lua(&ctx));
                notifier.notify();
            }
            Err(e) => {
                log::error!("Oscilla: Lua eval error: {e}");
                compiler.store_error(e);
                notifier.notify();
            }
        },
        ScriptMode::TimeBased => {
            #[cfg(feature = "time-buffer")]
            {
                let rate = sr.load(Ordering::Relaxed);
                match crate::script::generate_time_buffer(source, rate) {
                    Ok(buf) => {
                        tb_slot.store(Arc::new(buf));
                        notifier.notify();
                    }
                    Err(e) => {
                        log::error!("Oscilla: buffer gen: {e}");
                        compiler.store_error(e);
                        notifier.notify();
                    }
                }
            }
            #[cfg(not(feature = "time-buffer"))]
            {
                lua_slot.store(Arc::new(source.to_owned()));
                notifier.notify();
            }
        }
    }
}

// Format exports

impl ClapPlugin for Oscilla {
    const CLAP_ID: &'static str = "me.pychael.oscilla";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Programmable oscillator synthesizer");
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
