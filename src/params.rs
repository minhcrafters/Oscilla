use atomic_refcell::AtomicRefCell;
use nice_plug::prelude::*;
use std::sync::Arc;

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
