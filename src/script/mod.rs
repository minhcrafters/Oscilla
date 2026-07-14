//! Rhai-based scripting engine for Oscilla.
//!
//! Users write a small "shader for sound" that defines a waveform function
//! mapping a phase value `x` (0..2π) to an amplitude value.
//!
//! The engine exposes DSP-friendly math functions:
//! `sin`, `cos`, `saw`, `square`, `triangle`, `noise`, `fold`, `clip`, `tanh`,
//! `abs`, `pow`, `log`, `exp`, `floor`, `ceil`, `round`, `fract`.
//!
//! Example patches:
//! - `sin(x) + sin(x*3)*0.25`
//! - `tanh(saw(x) + noise(x)*0.05)`
//! - `fold(sin(x*2), 0.5)`
//!
//! ## Integer / float mixing
//!
//! Rhai is a dynamically-typed scripting language where integer literals
//! (`3`, `6`) and float literals (`0.05`, `1.0`) are distinct types (`INT`
//! and `FLOAT`).
//! 
//! By enabling the `f32_float` feature in `Cargo.toml`, Rhai's internal `FLOAT`
//! type is set to `f32`, which matches our DSP code perfectly.
//! We register `INT`-accepting overloads for every DSP function and every math
//! operator involving `f32` so that mixed arithmetic like `x * 3` or `sin(x) * 2`
//! works gracefully without type errors.

use rhai::{Engine, INT, Scope, AST};
use std::sync::Arc;
use std::sync::Mutex;

// ── DSP primitive functions ───────────────────────────────────────────

/// Simple pseudo-random function based on phase for deterministic "noise".
fn phase_noise(x: f32) -> f32 {
    let n = (x.sin() * 43758.5453).fract();
    n * 2.0 - 1.0
}

/// Sawtooth wave: maps phase 0..2π to -1..1.
fn saw(x: f32) -> f32 {
    let t = x / (2.0 * std::f32::consts::PI);
    2.0 * (t - (t + 0.5).floor())
}

/// Square wave with explicit pulse-width.
fn square(x: f32, pw: f32) -> f32 {
    let t = x / (2.0 * std::f32::consts::PI);
    let frac = t.fract();
    if frac < pw.clamp(0.01, 0.99) { 1.0 } else { -1.0 }
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

// ── Script engine ─────────────────────────────────────────────────────

/// Script engine that compiles and evaluates Oscilla waveform scripts.
pub struct ScriptEngine {
    engine: Engine,
    ast: AST,
}

impl ScriptEngine {
    /// Compile a user script into an evaluable function.
    ///
    /// The script should be an expression (or series of expressions) that
    /// evaluates to a float value representing the waveform amplitude
    /// at phase `x`.  The variable `x` is pre-bound in the scope as `f32`.
    ///
    /// Integer and float literals can be freely mixed: `sin(x*3)*0.25` works
    /// exactly as users expect.
    pub fn compile(source: &str) -> Result<Self, String> {
        let mut engine = Engine::new();

        // ── Register DSP / waveshaping functions ─────────────────────
        // We use rhai::Dynamic to seamlessly accept f32, f64, or INT
        // and avoid missing function signatures.

        engine.register_fn("saw", |x: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            Ok(rhai::Dynamic::from(saw(as_f32(x)?)))
        });

        engine.register_fn("square", |x: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            Ok(rhai::Dynamic::from(square_default(as_f32(x)?)))
        });
        engine.register_fn("square", |x: rhai::Dynamic, pw: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            Ok(rhai::Dynamic::from(square(as_f32(x)?, as_f32(pw)?)))
        });

        engine.register_fn("triangle", |x: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            Ok(rhai::Dynamic::from(triangle(as_f32(x)?)))
        });

        engine.register_fn("noise", |x: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            Ok(rhai::Dynamic::from(phase_noise(as_f32(x)?)))
        });

        engine.register_fn("fold", |x: rhai::Dynamic, t: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            Ok(rhai::Dynamic::from(fold(as_f32(x)?, as_f32(t)?)))
        });

        engine.register_fn("clip", |x: rhai::Dynamic, l: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            Ok(rhai::Dynamic::from(clip(as_f32(x)?, as_f32(l)?)))
        });

        // ── Core math ────────────────────────────────────────────────────
        macro_rules! reg_math {
            ($name:expr, $func:expr) => {
                engine.register_fn($name, |x: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                    let f = as_f32(x)?;
                    Ok(rhai::Dynamic::from($func(f)))
                });
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

        engine.register_fn("pow", |x: rhai::Dynamic, y: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            Ok(rhai::Dynamic::from(as_f32(x)?.powf(as_f32(y)?)))
        });

        // ── Arithmetic fallback operators ────────────────────────────────
        engine.register_fn("+", |a: rhai::Dynamic, b: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            Ok(rhai::Dynamic::from(as_f32(a)? + as_f32(b)?))
        });
        engine.register_fn("-", |a: rhai::Dynamic, b: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            Ok(rhai::Dynamic::from(as_f32(a)? - as_f32(b)?))
        });
        engine.register_fn("*", |a: rhai::Dynamic, b: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            Ok(rhai::Dynamic::from(as_f32(a)? * as_f32(b)?))
        });
        engine.register_fn("/", |a: rhai::Dynamic, b: rhai::Dynamic| -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
            Ok(rhai::Dynamic::from(as_f32(a)? / as_f32(b)?))
        });

        let ast = engine
            .compile(source)
            .map_err(|e| format!("Compilation error: {e}"))?;

        Ok(Self { engine, ast })
    }

    /// Evaluate the compiled script at a given phase value `x` (0..2π).
    ///
    /// Called 2048 times during wavetable generation.  Rhai AST evaluation
    /// is interpreted but fast enough for offline wavetable generation.
    pub fn evaluate(&self, x: f32) -> Result<f32, String> {
        let mut scope = Scope::new();
        scope.push("x", x);
        // Convenience constants.
        scope.push_constant("pi",  std::f32::consts::PI);
        scope.push_constant("tau", 2.0_f32 * std::f32::consts::PI);
        scope.push_constant("e",   std::f32::consts::E);

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

// ── Thread-safe compiler wrapper ──────────────────────────────────────

/// Thread-safe wrapper for deferred compilation.
///
/// The actual compilation and wavetable generation is done on a background
/// thread, never on the audio callback.
pub struct ScriptCompiler {
    engine: Mutex<Option<ScriptEngine>>,
}

impl ScriptCompiler {
    pub fn new() -> Self {
        Self {
            engine: Mutex::new(None),
        }
    }

    /// Compile a script and store the engine. Returns Ok on success.
    pub fn compile(&self, source: &str) -> Result<(), String> {
        let engine = ScriptEngine::compile(source)?;
        let mut guard = self.engine.lock().unwrap();
        *guard = Some(engine);
        Ok(())
    }

    /// Sample the compiled waveform function into a wavetable.
    /// Returns the shared wavetable ready for the audio thread.
    pub fn generate_wavetable(&self) -> Result<crate::wavetable::SharedWavetable, String> {
        let guard = self.engine.lock().unwrap();
        let engine = guard.as_ref().ok_or("No script compiled")?;

        // Evaluate all 2048 samples, collecting any errors.
        let mut first_err: Option<String> = None;
        let samples: Vec<f32> = (0..crate::wavetable::WAVETABLE_SIZE)
            .map(|i| {
                let x = (i as f32 / crate::wavetable::WAVETABLE_SIZE as f32)
                    * 2.0
                    * std::f32::consts::PI;
                match engine.evaluate(x) {
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
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &str) -> f32 {
        let eng = ScriptEngine::compile(src).expect("compile");
        // Evaluate at x = pi (middle of the waveform)
        eng.evaluate(std::f32::consts::PI).expect("eval")
    }

    #[test]
    fn test_pure_float() {
        // Pure float expressions should work as before.
        let v = eval("sin(x)");
        assert!((v - 0.0_f32).abs() < 1e-5, "sin(π) ≈ 0, got {v}");
    }

    #[test]
    fn test_int_multiplier_in_argument() {
        // sin(x*3) — integer `3` passed as phase multiplier.
        let v = eval("sin(x*3)");
        assert!(v.is_finite(), "sin(x*3) must be finite, got {v}");
    }

    #[test]
    fn test_float_scaling_of_result() {
        // sin(x) * 0.05 — f32 result multiplied by a float literal.
        let v = eval("sin(x) * 0.05");
        assert!(v.abs() <= 0.06, "sin(x)*0.05 out of range: {v}");
    }

    #[test]
    fn test_int_scaling_of_result() {
        // sin(x) * 2 — integer multiplier on f32 result.
        let v = eval("sin(x) * 2");
        assert!(v.is_finite(), "sin(x)*2 must be finite, got {v}");
    }

    #[test]
    fn test_mixed_arithmetic() {
        // 6 * 0.05 — pure integer-float mixed expression.
        let v = eval("6 * 0.05");
        assert!((v - 0.3_f32).abs() < 1e-4, "6 * 0.05 = 0.3, got {v}");
    }

    #[test]
    fn test_complex_patch() {
        // Full patch example from the design doc.
        let v = eval("sin(x) + sin(x*3)*0.25");
        assert!(v.is_finite(), "complex patch must be finite, got {v}");
    }

    #[test]
    fn test_tanh_saw_noise() {
        // tanh(saw(x) + noise(x)*0.05)
        let v = eval("tanh(saw(x)+noise(x)*0.05)");
        assert!(v.is_finite(), "tanh patch must be finite, got {v}");
    }
}
