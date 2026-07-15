//! Rhai-based scripting engine for Oscilla.
//!
//! Users write a small "shader for sound" that defines a waveform function.
//!
//! Two operating modes are supported:
//!
//! **Wavetable mode** (`x` variable):
//! The script maps a phase value `x` (0..2π) to an amplitude value.
//! A 2048-sample single-cycle wavetable is generated and played back
//! with phase accumulation at the note frequency.
//!
//! **Time-based mode** (`t` variable):
//! The script maps elapsed time `t` (seconds since note-on) to an
//! amplitude value.  A long time-domain buffer is pre-rendered and
//! played back with pitch-shifting via rate adjustment.
//!
//! The engine exposes DSP-friendly math functions:
//! `sin`, `cos`, `saw`, `square`, `triangle`, `noise`, `fold`, `clip`, `tanh`,
//! `abs`, `pow`, `log`, `exp`, `floor`, `ceil`, `round`, `fract`.
//!
//! Example patches (wavetable mode):
//! - `sin(x) + sin(x*3)*0.25`
//! - `tanh(saw(x) + noise(x)*0.05)`
//! - `fold(sin(x*2), 0.5)`
//!
//! Example patches (time-based mode):
//! - `sin(t * pi * 2 * 440)`           — 440 Hz sine
//! - `sin(t * pi * 2 * 55) * sin(t * pi * 2 * 66)` — ring-mod bell
//! - `sin(t * pi * 2 * 220) * exp(-t * 3)` — percussive pluck
//!

use rhai::INT;
use rhai::{AST, Array, Engine, Scope};
use std::sync::Arc;
use std::sync::Mutex;

/// Which variable the script is a function of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptMode {
    /// Script uses `x` (phase 0..2pi); output is a single-cycle wavetable.
    Wavetable,
    /// Script uses `t` (seconds since note-on); output is a time-domain buffer.
    TimeBased,
}

impl std::fmt::Display for ScriptMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wavetable => write!(f, "Wavetable (x)"),
            Self::TimeBased => write!(f, "Time (t)"),
        }
    }
}

// DSP primitives

/// Simple pseudo-random function based on phase for deterministic "noise".
fn phase_noise(x: f32) -> f32 {
    let n = (x.sin() * 43_758.547).fract();
    n * 2.0 - 1.0
}

/// Sawtooth wave: maps phase 0..2pi to -1..1.
fn saw(x: f32) -> f32 {
    let t = x / (2.0 * std::f32::consts::PI);
    2.0 * (t - (t + 0.5).floor())
}

/// Square wave with explicit pulse-width.
fn square(x: f32, pw: f32) -> f32 {
    let t = x / (2.0 * std::f32::consts::PI);
    let frac = t.fract();
    if frac < pw.clamp(0.01, 0.99) {
        1.0
    } else {
        -1.0
    }
}

/// Square wave (default 50% duty cycle).
fn square_default(x: f32) -> f32 {
    square(x, 0.5)
}

/// Triangle wave.
fn triangle(x: f32) -> f32 {
    let t = x / (2.0 * std::f32::consts::PI);
    4.0 * (t - (t + 0.75).floor() + 0.25).abs() - 1.0
}

/// Wavefolding distortion.
fn fold(x: f32, threshold: f32) -> f32 {
    let t = threshold.max(0.001);
    let mut y = x;
    // Limit iterations to prevent infinite loops on pathological inputs.
    for _ in 0..64 {
        if y > t {
            y = 2.0 * t - y;
        } else if y < -t {
            y = -2.0 * t - y;
        } else {
            break;
        }
    }
    y
}

/// Hard clipping.
fn clip(x: f32, level: f32) -> f32 {
    x.clamp(-level, level)
}

/// Linear interpolation between values.
fn lerp(values: Array, t: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }

    let pos = t.clamp(0.0, 1.0) * (values.len() - 1) as f32;

    let index = pos.floor() as usize;
    let frac = pos - index as f32;

    if index >= values.len() - 1 {
        return values[index].clone().try_cast::<f32>().unwrap_or(0.0);
    }

    let a = values[index].clone().try_cast::<f32>().unwrap_or(0.0);

    let b = values[index + 1].clone().try_cast::<f32>().unwrap_or(0.0);

    a + (b - a) * frac
}

fn as_f32(v: rhai::Dynamic) -> Result<f32, Box<rhai::EvalAltResult>> {
    if let Some(f) = v.clone().try_cast::<f32>() {
        Ok(f)
    } else if let Some(f) = v.clone().try_cast::<f64>() {
        Ok(f as f32)
    } else if let Some(i) = v.clone().try_cast::<rhai::INT>() {
        Ok(i as f32)
    } else {
        Err(format!("Expected a number, got {}", v.type_name()).into())
    }
}

// Script engine

/// Script engine that compiles and evaluates Oscilla waveform scripts.
pub struct ScriptEngine {
    engine: Engine,
    ast: AST,
    /// Which mode this engine was compiled for.
    pub mode: ScriptMode,
}

impl ScriptEngine {
    /// Compile a user script into an evaluable function.
    ///
    /// The script should be an expression (or series of expressions) that
    /// evaluates to a float value representing the waveform amplitude.
    ///
    /// In wavetable mode the variable `x` (phase 0..2π) is available;
    /// in time-based mode the variable `t` (seconds) is available.
    ///
    /// Integer and float literals can be freely mixed: `sin(x*3)*0.25` works
    /// exactly as users expect.
    pub fn compile(source: &str, mode: ScriptMode) -> Result<Self, String> {
        let mut engine = Engine::new();

        // Register DSP / waveshaping functions
        // We use rhai::Dynamic to seamlessly accept f32, f64, or INT
        // and avoid missing function signatures.

        engine.register_fn(
            "saw",
            |x: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(rhai::Dynamic::from(saw(as_f32(x)?)))
            },
        );

        engine.register_fn(
            "square",
            |x: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(rhai::Dynamic::from(square_default(as_f32(x)?)))
            },
        );
        engine.register_fn(
            "square",
            |x: rhai::Dynamic,
             pw: rhai::Dynamic|
             -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(rhai::Dynamic::from(square(as_f32(x)?, as_f32(pw)?)))
            },
        );

        engine.register_fn(
            "triangle",
            |x: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(rhai::Dynamic::from(triangle(as_f32(x)?)))
            },
        );

        engine.register_fn(
            "noise",
            |x: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(rhai::Dynamic::from(phase_noise(as_f32(x)?)))
            },
        );

        engine.register_fn(
            "fold",
            |x: rhai::Dynamic,
             t: rhai::Dynamic|
             -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(rhai::Dynamic::from(fold(as_f32(x)?, as_f32(t)?)))
            },
        );

        engine.register_fn(
            "clip",
            |x: rhai::Dynamic,
             l: rhai::Dynamic|
             -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(rhai::Dynamic::from(clip(as_f32(x)?, as_f32(l)?)))
            },
        );

        engine.register_fn(
            "lerp",
            |values: rhai::Array,
             t: rhai::Dynamic|
             -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(rhai::Dynamic::from(lerp(values, as_f32(t)?)))
            },
        );

        // Core math
        macro_rules! reg_math {
            ($name:expr, $func:expr) => {
                engine.register_fn(
                    $name,
                    |x: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                        let f = as_f32(x)?;
                        Ok(rhai::Dynamic::from($func(f)))
                    },
                );
            };
        }
        reg_math!("sin", |x: f32| x.sin());
        reg_math!("cos", |x: f32| x.cos());
        reg_math!("tan", |x: f32| x.tan());
        reg_math!("tanh", |x: f32| x.tanh());
        reg_math!("abs", |x: f32| x.abs());
        reg_math!("log", |x: f32| x.ln());
        reg_math!("log10", |x: f32| x.log10());
        reg_math!("exp", |x: f32| x.exp());
        reg_math!("sqrt", |x: f32| x.sqrt());
        reg_math!("floor", |x: f32| x.floor());
        reg_math!("ceil", |x: f32| x.ceil());
        reg_math!("round", |x: f32| x.round());
        reg_math!("fract", |x: f32| x.fract());

        engine.register_fn(
            "pow",
            |x: rhai::Dynamic,
             y: rhai::Dynamic|
             -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(rhai::Dynamic::from(as_f32(x)?.powf(as_f32(y)?)))
            },
        );

        // Arithmetic fallback operators
        engine.register_fn(
            "+",
            |a: rhai::Dynamic,
             b: rhai::Dynamic|
             -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(rhai::Dynamic::from(as_f32(a)? + as_f32(b)?))
            },
        );
        engine.register_fn(
            "-",
            |a: rhai::Dynamic,
             b: rhai::Dynamic|
             -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(rhai::Dynamic::from(as_f32(a)? - as_f32(b)?))
            },
        );
        engine.register_fn(
            "*",
            |a: rhai::Dynamic,
             b: rhai::Dynamic|
             -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(rhai::Dynamic::from(as_f32(a)? * as_f32(b)?))
            },
        );
        engine.register_fn(
            "/",
            |a: rhai::Dynamic,
             b: rhai::Dynamic|
             -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                Ok(rhai::Dynamic::from(as_f32(a)? / as_f32(b)?))
            },
        );

        let ast = engine
            .compile(source)
            .map_err(|e| format!("Compilation error: {e}"))?;

        Ok(Self { engine, ast, mode })
    }

    /// Evaluate the compiled script at a given phase `x` (0..2π) and time `t`
    /// (elapsed seconds since note-on).
    ///
    /// Both `x` and `t` are always bound so scripts can freely reference
    /// either variable regardless of mode.
    pub fn evaluate(&self, x: f32, t: f32) -> Result<f32, String> {
        let mut scope = Scope::new();
        scope.push("x", x);
        scope.push("t", t);

        // Convenience constants.
        scope.push_constant("pi", std::f32::consts::PI);
        scope.push_constant("tau", 2.0_f32 * std::f32::consts::PI);
        scope.push_constant("e", std::f32::consts::E);

        let dyn_val = self
            .engine
            .eval_ast_with_scope::<rhai::Dynamic>(&mut scope, &self.ast)
            .map_err(|e| format!("Evaluation error: {e}"))?;

        let result = if let Some(v) = dyn_val.clone().try_cast::<f32>() {
            v
        } else if let Some(v) = dyn_val.clone().try_cast::<f64>() {
            v as f32
        } else if let Some(v) = dyn_val.clone().try_cast::<INT>() {
            v as f32
        } else {
            return Err(format!(
                "Script returned non-numeric type: {}",
                dyn_val.type_name()
            ));
        };

        // Clamp to reasonable range to avoid extreme values.
        Ok(result.clamp(-100.0, 100.0))
    }
}

// Compiler wrapper

/// Thread-safe wrapper for deferred compilation.
///
/// The actual compilation and wavetable/time-buffer generation is done
/// on a background thread, never on the audio callback.
pub struct ScriptCompiler {
    engine: Mutex<Option<ScriptEngine>>,
}

impl Default for ScriptCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptCompiler {
    pub fn new() -> Self {
        Self {
            engine: Mutex::new(None),
        }
    }

    /// Return the mode of the currently compiled script (if any).
    pub fn current_mode(&self) -> Option<ScriptMode> {
        let guard = self.engine.lock().unwrap();
        guard.as_ref().map(|e| e.mode)
    }

    /// Compile a script and store the engine. Returns Ok on success.
    pub fn compile(&self, source: &str, mode: ScriptMode) -> Result<(), String> {
        let engine = ScriptEngine::compile(source, mode)?;
        let mut guard = self.engine.lock().unwrap();
        *guard = Some(engine);
        Ok(())
    }

    // Wavetable generation

    /// Sample the compiled waveform function into a wavetable.
    ///
    /// Returns the shared wavetable ready for the audio thread.
    pub fn generate_wavetable(&self) -> Result<crate::wavetable::SharedWavetable, String> {
        self.generate_wavetable_at(0.0)
    }

    /// Sample the compiled waveform function into a wavetable at a specific
    /// time offset `t` (in seconds).
    pub fn generate_wavetable_at(
        &self,
        t: f32,
    ) -> Result<crate::wavetable::SharedWavetable, String> {
        let guard = self.engine.lock().unwrap();
        let engine = guard.as_ref().ok_or("No script compiled")?;

        // Evaluate all 2048 samples, collecting any errors.
        let mut first_err: Option<String> = None;
        let samples: Vec<f32> = (0..crate::wavetable::WAVETABLE_SIZE)
            .map(|i| {
                let x = (i as f32 / crate::wavetable::WAVETABLE_SIZE as f32)
                    * 2.0
                    * std::f32::consts::PI;
                match engine.evaluate(x, t) {
                    Ok(v) => v,
                    Err(e) => {
                        if first_err.is_none() {
                            first_err = Some(e);
                        }
                        0.0
                    }
                }
            })
            .collect();

        if let Some(err) = first_err {
            return Err(format!("Evaluation error: {err}"));
        }

        // Build wavetable from pre-evaluated samples.
        let mut data = Box::new([0.0f32; crate::wavetable::WAVETABLE_SIZE]);
        data.copy_from_slice(&samples);

        crate::wavetable::remove_dc(&mut data);
        crate::wavetable::normalize(&mut data);
        crate::wavetable::band_limit(&mut data, 200);
        crate::wavetable::normalize(&mut data);

        Ok(Arc::new(data))
    }

    // Time-buffer generation

    /// Generate a time-domain buffer by evaluating the script at regular
    /// time intervals.
    ///
    /// The buffer represents `buf_duration_secs` of audio at `sample_rate`.
    /// During playback each voice reads from this buffer with its read
    /// pointer advancing at a rate proportional to the note frequency
    /// (1× at A4 = 440 Hz), which gives pitch-shifted time-based synthesis.
    pub fn generate_time_buffer(
        &self,
        sample_rate: f32,
        buf_duration_secs: f32,
    ) -> Result<crate::wavetable::SharedTimeBuffer, String> {
        let guard = self.engine.lock().unwrap();
        let engine = guard.as_ref().ok_or("No script compiled")?;

        let len = (sample_rate * buf_duration_secs) as usize;
        let mut data = Vec::with_capacity(len);

        let mut first_err: Option<String> = None;
        for i in 0..len {
            let t = i as f32 / sample_rate;
            match engine.evaluate(0.0, t) {
                Ok(v) => data.push(v.clamp(-4.0, 4.0)),
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                    data.push(0.0);
                }
            }
        }

        if let Some(err) = first_err {
            return Err(format!("Evaluation error: {err}"));
        }

        // Normalize the time buffer (peak = 1.0).
        let peak = data.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()));
        if peak > 1e-10 {
            let scale = 1.0 / peak;
            for s in data.iter_mut() {
                *s *= scale;
            }
        }

        Ok(Arc::new(data))
    }

    /// Convenience: generate both wavetable and time buffer at once.
    /// Returns `(wavetable, time_buffer)`.
    pub fn generate_both(
        &self,
        mode: ScriptMode,
        sample_rate: f32,
    ) -> Result<
        (
            Option<crate::wavetable::SharedWavetable>,
            Option<crate::wavetable::SharedTimeBuffer>,
        ),
        String,
    > {
        match mode {
            ScriptMode::Wavetable => {
                let wt = self.generate_wavetable()?;
                Ok((Some(wt), None))
            }
            ScriptMode::TimeBased => {
                let tb =
                    self.generate_time_buffer(sample_rate, crate::wavetable::TIME_BUFFER_DURATION)?;
                Ok((None, Some(tb)))
            }
        }
    }
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &str) -> f32 {
        let eng = ScriptEngine::compile(src, ScriptMode::Wavetable).expect("compile");
        // Evaluate at x = pi (middle of the waveform), t = 0
        eng.evaluate(std::f32::consts::PI, 0.0).expect("eval")
    }

    #[test]
    fn test_pure_float() {
        let v = eval("sin(x)");
        assert!((v - 0.0_f32).abs() < 1e-5, "sin(π) ≈ 0, got {v}");
    }

    #[test]
    fn test_int_multiplier_in_argument() {
        let v = eval("sin(x*3)");
        assert!(v.is_finite(), "sin(x*3) must be finite, got {v}");
    }

    #[test]
    fn test_float_scaling_of_result() {
        let v = eval("sin(x) * 0.05");
        assert!(v.abs() <= 0.06, "sin(x)*0.05 out of range: {v}");
    }

    #[test]
    fn test_int_scaling_of_result() {
        let v = eval("sin(x) * 2");
        assert!(v.is_finite(), "sin(x)*2 must be finite, got {v}");
    }

    #[test]
    fn test_mixed_arithmetic() {
        let v = eval("6 * 0.05");
        assert!((v - 0.3_f32).abs() < 1e-4, "6 * 0.05 = 0.3, got {v}");
    }

    #[test]
    fn test_complex_patch() {
        let v = eval("sin(x) + sin(x*3)*0.25");
        assert!(v.is_finite(), "complex patch must be finite, got {v}");
    }

    #[test]
    fn test_tanh_saw_noise() {
        let v = eval("tanh(saw(x)+noise(x)*0.05)");
        assert!(v.is_finite(), "tanh patch must be finite, got {v}");
    }

    #[test]
    fn test_time_variable_available() {
        let v = eval("t");
        assert!((v - 0.0_f32).abs() < 1e-6, "t should be 0, got {v}");
    }

    #[test]
    fn test_time_phase_modulation() {
        let v = eval("sin(x + t)");
        assert!(
            (v - 0.0_f32).abs() < 1e-5,
            "sin(x+t) at t=0 ≈ sin(x), got {v}"
        );
    }

    #[test]
    fn test_amplitude_modulation_over_time() {
        let eng = ScriptEngine::compile("sin(x) * sin(t * pi * 4)", ScriptMode::Wavetable)
            .expect("compile");
        let v = eng.evaluate(std::f32::consts::PI * 0.5, 0.0).expect("eval");
        assert!(
            (v - 0.0_f32).abs() < 1e-5,
            "sin(x)*sin(t*4pi) at t=0 should be 0, got {v}"
        );
        let v2 = eng
            .evaluate(std::f32::consts::PI * 0.5, 0.125)
            .expect("eval");
        assert!(
            (v2 - 1.0_f32).abs() < 1e-4,
            "sin(x)*sin(t*4pi) at t=0.125 should be ≈ 1, got {v2}"
        );
    }

    #[test]
    fn test_exponential_decay_builtin() {
        let eng =
            ScriptEngine::compile("sin(x) * exp(-t * 3)", ScriptMode::Wavetable).expect("compile");
        let v0 = eng
            .evaluate(std::f32::consts::PI * 0.5, 0.0)
            .expect("eval at t=0");
        let v1 = eng
            .evaluate(std::f32::consts::PI * 0.5, 1.0)
            .expect("eval at t=1");
        assert!((v0 - 1.0_f32).abs() < 1e-4, "at t=0 should be 1, got {v0}");
        assert!(
            (v1 - 0.0497_f32).abs() < 1e-2,
            "at t=1 should be ≈ e^-3 ≈ 0.05, got {v1}"
        );
    }

    #[test]
    fn test_time_based_mode_basic() {
        let eng = ScriptEngine::compile("sin(t)", ScriptMode::TimeBased).expect("compile");
        let v0 = eng.evaluate(0.0, 0.0).expect("eval t=0");
        assert!((v0 - 0.0).abs() < 1e-6, "sin(0) should be 0, got {v0}");
        // At t = pi/2 ≈ 1.57, sin(t) ≈ 1.0
        let v1 = eng
            .evaluate(0.0, std::f32::consts::PI * 0.5)
            .expect("eval t=pi/2");
        assert!((v1 - 1.0).abs() < 1e-4, "sin(pi/2) should be 1, got {v1}");
    }

    #[test]
    fn test_time_based_evaluate_ignores_x() {
        // In time-based mode, x is still bound but the script only uses t.
        let eng = ScriptEngine::compile("t", ScriptMode::TimeBased).expect("compile");
        let v = eng.evaluate(999.0, 2.5).expect("eval");
        assert!(
            (v - 2.5).abs() < 1e-4,
            "t-only script should return t, got {v}"
        );
    }
}
