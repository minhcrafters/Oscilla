# Oscilla

**Programmable oscillator synthesiser**

VST3 / CLAP plugin. Uses LuaJIT for near-native evaluation.

## Quickstart

```sh
cargo build --release
```

Copy `target/bundled/oscilla.clap` or `target/bundled/oscilla.vst3` to your DAW's plugin folder.

## Scripting

Every script defines a `main` function. **Wavetable mode** uses `x` (phase 0..2π), **Time mode** uses `t` (seconds).

```lua
-- Wavetable: single-cycle waveform
function main(x)
  return math.sin(x) + math.sin(x * 3) * 0.25
end
```

```lua
-- Time-based: per-sample expression
function main(t)
  return math.sin(t * math.pi * 2 * 440) * math.exp(-t * 3)
end
```

### Available functions

| Module | Functions |
|--------|-----------|
| `math.*` | `sin` `cos` `tan` `tanh` `abs` `exp` `sqrt` `floor` `ceil` `round` `log` `lerp` `rand` `min` `max` `pi` `tau` `e` |
| `dsp.*` | `saw` `square` `triangle` `noise` `fold` `clip` |

## Features

| Flag | Effect |
|------|--------|
| *(default)* | Per-sample evaluation |
| `time-buffer` | Pre-render 8s buffer |

## Presets

Save/Load buttons write `.osc` files to `Documents/Oscilla/`. Presets store all knob positions plus the script.

## Build from source

Requires Rust 1.80+ and cargo-nice-plug (`cargo install cargo-nice-plug`)

```sh
git clone https://git.pychael.me/pychael/oscilla
cd oscilla
cargo run # standalone mode
cargo nice-plug bundle oscilla # export as CLAP/VST3 plugin
```

## License

MIT
