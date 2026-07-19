//! LuaJIT-based scripting engine for Oscilla.
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
//! amplitude value.  The expression is evaluated per-sample in the
//! audio callback using LuaJIT's trace compiler for near-native speed.
//!
//! The engine exposes DSP-friendly functions in two modules:
//! - `math.*` — standard math: `sin`, `cos`, `exp`, `pi`, etc.
//! - `dsp.*` — custom waveshaping: `saw`, `square`, `triangle`, `noise`, `fold`, `clip`
//! - `math.*` — standard + utility: `sin`, `cos`, `exp`, `lerp`, `rand`, `min`, `max`, etc.
//!
//! Example patches (wavetable mode):
//! - `function main(x) return math.sin(x) + math.sin(x*3)*0.25 end`
//! - `function main(x) return math.tanh(dsp.saw(x) + dsp.noise(x)*0.05) end`
//! - `function main(x) return dsp.fold(math.sin(x*2), 0.5) end`
//!
//! Example patches (time-based mode):
//! - `function main(t) return math.sin(t * math.pi * 2 * 440) end`
//! - `function main(t) return math.sin(t * math.pi * 2 * 55) * math.sin(t * math.pi * 2 * 66) end`
//! - `function main(t) return math.sin(t * math.pi * 2 * 220) * math.exp(-t * 3) end`
//!

use mlua::{Function, Lua, Value, Variadic};
use std::sync::Mutex;

/// Which variable the script is a function of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptMode {
    /// Script uses `x` (phase 0..2pi); output is a single-cycle wavetable.
    Wavetable,
    /// Script uses `t` (seconds since note-on); output is real-time evaluated.
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

// DSP primitives (pure Rust, registered into Lua as globals)

fn phase_noise(x: f32) -> f32 {
    let n = (x.sin() * 43_758.547).fract();
    n * 2.0 - 1.0
}

fn saw(x: f32) -> f32 {
    let t = x / (2.0 * std::f32::consts::PI);
    2.0 * (t - (t + 0.5).floor())
}

fn square_default(x: f32) -> f32 {
    let t = x / (2.0 * std::f32::consts::PI);
    if t.fract() < 0.5 { 1.0 } else { -1.0 }
}

fn square_pw(x: f32, pw: f32) -> f32 {
    let t = x / (2.0 * std::f32::consts::PI);
    if t.fract() < pw.clamp(0.01, 0.99) {
        1.0
    } else {
        -1.0
    }
}

fn triangle(x: f32) -> f32 {
    let t = x / (2.0 * std::f32::consts::PI);
    4.0 * (t - (t + 0.75).floor() + 0.25).abs() - 1.0
}

fn fold(x: f32, threshold: f32) -> f32 {
    let t = threshold.max(0.001);
    let mut y = x;
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

fn clip(x: f32, level: f32) -> f32 {
    x.clamp(-level, level)
}

/// Lua-facing lerp: `math.lerp(table, t)` where `table` is an array of
/// values and `t` is the interpolation factor in [0, 1].
fn lerp_table(table: mlua::Table, t: f32) -> mlua::Result<f32> {
    let t = t.clamp(0.0, 1.0);
    let len = table.len()? as usize;
    if len == 0 {
        return Ok(0.0);
    }
    if len == 1 {
        return Ok(table.get::<f32>(1)?);
    }
    let pos = t * (len - 1) as f32;
    let idx = (pos.floor() as usize).min(len - 2);
    let frac = pos - idx as f32;
    let a: f32 = table.get(idx + 1)?;
    let b: f32 = table.get(idx + 2)?;
    Ok(a + (b - a) * frac)
}

/// Register custom oscillator DSP functions as a `dsp` Lua module.
/// User scripts access them via `dsp.saw(x)`, etc.
fn register_dsp_table(lua: &Lua) -> mlua::Result<()> {
    let dsp = lua.create_table()?;

    dsp.set("saw", lua.create_function(|_, x: f32| Ok(saw(x)))?)?;
    dsp.set(
        "square",
        lua.create_function(|_, args: Variadic<f32>| {
            if args.len() == 1 {
                Ok(square_default(args[0]))
            } else {
                Ok(square_pw(args[0], args[1]))
            }
        })?,
    )?;
    dsp.set(
        "triangle",
        lua.create_function(|_, x: f32| Ok(triangle(x)))?,
    )?;
    dsp.set(
        "noise",
        lua.create_function(|_, x: f32| Ok(phase_noise(x)))?,
    )?;
    dsp.set(
        "fold",
        lua.create_function(|_, (x, t): (f32, f32)| Ok(fold(x, t)))?,
    )?;
    dsp.set(
        "clip",
        lua.create_function(|_, (x, l): (f32, f32)| Ok(clip(x, l)))?,
    )?;

    lua.globals().set("dsp", dsp)?;
    Ok(())
}

/// Compile a user script — validates that the expression parses.
/// Returns Ok(()) if the script is valid Lua.
pub fn validate_script(source: &str) -> Result<(), String> {
    let lua = Lua::new();
    register_dsp_table(&lua).map_err(|e| format!("Init error: {e}"))?;
    register_math_table(&lua).map_err(|e| format!("Init error: {e}"))?;

    let wrapped = wrap_script(source, "x");
    lua.load(&wrapped)
        .eval::<Value>()
        .map_err(|e| format!("Compilation error: {e}"))?;

    Ok(())
}

/// Wrap user source so that the `main` function is returned directly.
/// The user's code runs once at load time (defining globals, helpers),
/// and `main` is extracted as the callable function.
fn wrap_script(source: &str, _var: &str) -> String {
    format!("{source}\nreturn main")
}

/// Register `math.sin`, `math.pi`, etc. as a `math` global table.
fn register_math_table(lua: &Lua) -> mlua::Result<()> {
    let math = lua.create_table()?;
    math.set("pi", std::f32::consts::PI)?;
    math.set("tau", std::f32::consts::TAU)?;
    math.set("e", std::f32::consts::E)?;

    macro_rules! mfn {
        ($name:expr, $func:expr) => {
            math.set($name, lua.create_function(|_, x: f32| Ok($func(x)))?)?;
        };
    }
    mfn!("sin", f32::sin);
    mfn!("cos", f32::cos);
    mfn!("tan", f32::tan);
    mfn!("tanh", f32::tanh);
    mfn!("abs", f32::abs);
    mfn!("exp", f32::exp);
    mfn!("sqrt", f32::sqrt);
    mfn!("floor", f32::floor);
    mfn!("ceil", f32::ceil);
    mfn!("round", f32::round);
    math.set("fract", lua.create_function(|_, x: f32| Ok(x.fract()))?)?;
    math.set("log", lua.create_function(|_, x: f32| Ok(x.ln()))?)?;
    math.set("log10", lua.create_function(|_, x: f32| Ok(x.log10()))?)?;

    math.set(
        "lerp",
        lua.create_function(|_, (table, t): (mlua::Table, f32)| lerp_table(table, t))?,
    )?;
    math.set(
        "rand",
        lua.create_function(|_, ()| Ok(rand::random::<f32>()))?,
    )?;
    math.set(
        "min",
        lua.create_function(|_, (a, b): (f32, f32)| Ok(a.min(b)))?,
    )?;
    math.set(
        "max",
        lua.create_function(|_, (a, b): (f32, f32)| Ok(a.max(b)))?,
    )?;

    lua.globals().set("math", math)?;
    Ok(())
}

/// A live Lua context holding a compiled function ready for per-sample calls.
///
/// Created on the audio thread when a new script needs to be evaluated.
pub struct LuaContext {
    // Lifetime anchor of `func`, references internal Lua FFI state.
    #[allow(dead_code)]
    lua: Lua,
    func: Function,
}

impl LuaContext {
    /// Compile a user script into a callable Lua function.
    pub fn compile(source: &str, mode: ScriptMode) -> Result<Self, String> {
        let lua = Lua::new();
        register_dsp_table(&lua).map_err(|e| format!("Init error: {e}"))?;
        register_math_table(&lua).map_err(|e| format!("Init error: {e}"))?;

        let var = match mode {
            ScriptMode::Wavetable => "x",
            ScriptMode::TimeBased => "t",
        };

        let wrapped = wrap_script(source, var);

        let func: Function = lua
            .load(&wrapped)
            .eval()
            .map_err(|e| format!("Compilation error: {e}"))?;

        Ok(Self { lua, func })
    }

    /// Evaluate the function at phase `x`.
    #[inline]
    pub fn eval_x(&self, x: f32) -> f32 {
        self.func.call::<f32>(x).unwrap_or(0.0)
    }

    /// Evaluate the function at time `t`.
    #[inline]
    pub fn eval_t(&self, t: f32) -> f32 {
        self.func.call::<f32>(t).unwrap_or(0.0)
    }
}

/// Duration of pre-rendered time buffers (seconds).
#[cfg(feature = "time-buffer")]
pub const TIME_BUFFER_DURATION: f32 = 8.0;

/// Generate a time-domain buffer by evaluating the Lua expression at
/// regular time intervals using parallel rayon chunks.  Each thread
/// gets its own LuaJIT instance so evaluation is lock-free.
#[cfg(feature = "time-buffer")]
pub fn generate_time_buffer(source: &str, sample_rate: f32) -> Result<Vec<f32>, String> {
    use rayon::prelude::*;
    use std::sync::Mutex;

    // Validate the script once first.
    let lua = Lua::new();
    register_dsp_table(&lua).map_err(|e| format!("Init: {e}"))?;
    register_math_table(&lua).map_err(|e| format!("Init: {e}"))?;
    let wrapped = wrap_script(source, "t");
    lua.load(&wrapped)
        .eval::<Value>()
        .map_err(|e| format!("Compile: {e}"))?;
    drop(lua);

    let len = (sample_rate * TIME_BUFFER_DURATION) as usize;
    let inv_sr = 1.0 / sample_rate;
    let mut data: Vec<f32> = vec![0.0; len];

    let num_threads = rayon::current_num_threads();
    let chunk_size = (len / num_threads).max(4096);
    let first_err: Mutex<Option<String>> = Mutex::new(None);

    data.par_chunks_mut(chunk_size)
        .enumerate()
        .for_each(|(chunk_idx, chunk)| {
            // Each thread creates its own Lua instance.
            let tlua = Lua::new();
            if register_dsp_table(&tlua).is_err() || register_math_table(&tlua).is_err() {
                return;
            }
            let func: Function = match tlua.load(&wrapped).eval() {
                Ok(f) => f,
                Err(e) => {
                    let mut g = first_err.lock().unwrap();
                    if g.is_none() {
                        *g = Some(format!("Compile: {e}"));
                    }
                    return;
                }
            };

            let base = chunk_idx * chunk_size;
            for (j, s) in chunk.iter_mut().enumerate() {
                let t = (base + j) as f32 * inv_sr;
                *s = func.call::<f32>((t,)).unwrap_or(0.0).clamp(-4.0, 4.0);
            }
        });

    if let Some(err) = first_err.into_inner().unwrap() {
        return Err(err);
    }

    // Normalize.
    let peak = data.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
    if peak > 1e-10 {
        let scale = 1.0 / peak;
        data.par_iter_mut().for_each(|s| *s *= scale);
    }

    Ok(data)
}

/// Thread-safe wrapper for deferred compilation.
///
/// Compilation happens on a background thread (via the task executor).
/// The compiled source and mode are stored here; the audio thread creates
/// a live `LuaContext` from them when it's time to evaluate.
pub struct ScriptCompiler {
    /// The last successfully compiled source code.
    compiled_source: Mutex<Option<String>>,
    /// The mode the source was compiled for.
    mode: Mutex<Option<ScriptMode>>,
    /// Last error message, for GUI display.
    last_error: Mutex<Option<String>>,
}

impl Default for ScriptCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptCompiler {
    pub fn new() -> Self {
        Self {
            compiled_source: Mutex::new(None),
            mode: Mutex::new(None),
            last_error: Mutex::new(None),
        }
    }

    pub fn current_mode(&self) -> Option<ScriptMode> {
        *self.mode.lock().unwrap()
    }

    pub fn store_error(&self, msg: String) {
        *self.last_error.lock().unwrap() = Some(msg);
    }

    pub fn take_last_error(&self) -> Option<String> {
        self.last_error.lock().unwrap().take()
    }

    /// Compile a script (validate it). Returns Ok on success.
    pub fn compile(&self, source: &str, mode: ScriptMode) -> Result<(), String> {
        validate_script(source)?;
        *self.compiled_source.lock().unwrap() = Some(source.to_string());
        *self.mode.lock().unwrap() = Some(mode);
        self.last_error.lock().unwrap().take();
        Ok(())
    }

    /// Return the last compiled source (if any) for the audio thread
    /// to create a live `LuaContext` from.
    pub fn get_source(&self) -> Option<String> {
        self.compiled_source.lock().unwrap().clone()
    }

    pub fn get_mode(&self) -> Option<ScriptMode> {
        *self.mode.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sine_validate() {
        let src = "function main(x)\n    return math.sin(x)\nend";
        assert!(validate_script(src).is_ok());
    }

    #[test]
    fn test_sine_context() {
        let src = "function main(x)\n    return math.sin(x)\nend";
        let ctx = LuaContext::compile(src, ScriptMode::Wavetable).unwrap();
        let val = ctx.eval_x(std::f32::consts::PI / 2.0);
        assert!((val - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_sine_time() {
        let src = "function main(t)\n    return math.sin(t * math.pi * 2 * 440)\nend";
        let ctx = LuaContext::compile(src, ScriptMode::TimeBased).unwrap();
        assert!(ctx.eval_t(0.0).abs() < 0.001);
        let t = 1.0 / (4.0 * 440.0);
        assert!((ctx.eval_t(t) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_complex() {
        let src = "function main(x)\n    return math.sin(x) + math.sin(x*3)*0.25\nend";
        let ctx = LuaContext::compile(src, ScriptMode::Wavetable).unwrap();
        ctx.eval_x(1.0);
    }

    #[test]
    fn test_dsp_functions() {
        for (expr, label) in &[
            ("dsp.saw(x)", "saw"),
            ("dsp.triangle(x)", "triangle"),
            ("dsp.noise(x)", "noise"),
            ("dsp.square(x)", "square"),
            ("dsp.square(x, 0.3)", "square_pw"),
            ("dsp.fold(math.sin(x*2), 0.5)", "fold"),
            ("dsp.clip(x, 0.5)", "clip"),
            ("math.rand()", "rand"),
        ] {
            let src = format!("function main(x)\n    return {expr}\nend");
            assert!(validate_script(&src).is_ok(), "failed: {label}");
        }
    }

    #[test]
    fn test_time_am() {
        let src = "function main(t)\n    return math.sin(t * math.pi * 2 * 55) * math.sin(t * math.pi * 2 * 66) * math.exp(-t * 3)\nend";
        let ctx = LuaContext::compile(src, ScriptMode::TimeBased).unwrap();
        assert!(ctx.eval_t(0.0).abs() < 0.001);
    }

    #[test]
    fn test_time_decay() {
        let src =
            "function main(t)\n    return math.sin(t * math.pi * 2 * 220) * math.exp(-t * 3)\nend";
        let ctx = LuaContext::compile(src, ScriptMode::TimeBased).unwrap();
        assert!(ctx.eval_t(0.0).abs() < 0.001);
        let v1 = ctx.eval_t(1.0);
        assert!(v1.abs() <= 1.0);
        assert!(ctx.eval_t(0.25).abs() < 0.01);
    }

    #[test]
    fn test_multi_line_script() {
        let source = r#"
local freq = 440.0
local fc = freq
local fm = freq * 14.0
local Ac = 1.0
local I0 = 5.0
local Tc = 1.5
local Tm = 0.08

function main(t)
    local carrier_env = math.exp(-t / Tc)
    local mod_env = math.exp(-t / Tm)
    local phase = 2.0 * math.pi * fc * t
                + I0 * mod_env * math.sin(2.0 * math.pi * fm * t)
    return Ac * carrier_env * math.sin(phase)
end
"#;
        assert!(validate_script(source).is_ok());
        let ctx = LuaContext::compile(source, ScriptMode::TimeBased).unwrap();
        assert!(ctx.eval_t(0.0).abs() < 0.01);
        let val = ctx.eval_t(0.01);
        assert!(val.abs() > 0.0);
    }

    #[test]
    fn test_main_function() {
        let source = r#"
function main(t)
    return math.sin(t * math.pi * 2 * 440)
end
"#;
        let ctx = LuaContext::compile(source, ScriptMode::TimeBased).unwrap();
        assert!(ctx.eval_t(0.0).abs() < 0.001);
        let t = 1.0 / (4.0 * 440.0);
        assert!((ctx.eval_t(t) - 1.0).abs() < 0.01);
    }
}
