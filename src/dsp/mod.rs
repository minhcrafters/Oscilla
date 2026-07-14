//! Polyphonic synthesizer engine.
//!
//! Manages a pool of voices with voice stealing, MIDI-to-frequency
//! conversion, unison, glide, and output mixing. All hot-path code
//! is allocation-free and uses only stack-local state.

pub mod envelope;
pub mod filter;
pub mod voice;

use self::filter::FilterType;
use self::voice::Voice;
use crate::wavetable::{SharedWavetable, WAVETABLE_SIZE};
use arc_swap::ArcSwap;
use smallvec::SmallVec;
/// Maximum number of polyphonic voices.
pub const MAX_VOICES: usize = 16;

/// Atomically swappable wavetable container.
///
/// The audio thread reads the current wavetable via `load()`, while
/// a background compilation thread writes a new one via `store()`.
/// Both operations are lock-free and realtime-safe.
pub struct WavetableSlot {
    inner: ArcSwap<Box<[f32; WAVETABLE_SIZE]>>,
}

impl WavetableSlot {
    pub fn new(table: SharedWavetable) -> Self {
        Self {
            inner: ArcSwap::from(table),
        }
    }

    #[inline]
    pub fn load(&self) -> SharedWavetable {
        self.inner.load_full()
    }

    pub fn store(&self, table: SharedWavetable) {
        self.inner.store(table);
    }
}

/// A pending note command queued during MIDI processing.
#[derive(Debug, Clone, Copy)]
enum NoteCmd {
    On { note: u8, velocity: f32 },
    Off { note: u8 },
}

/// The main polyphonic synthesizer engine.
///
/// Owns a fixed-size pool of [`Voice`]s and coordinates MIDI note
/// allocation, parameter updates, and per-sample audio generation.
pub struct SynthEngine {
    voices: [Voice; MAX_VOICES],
    pending: SmallVec<[NoteCmd; 32]>,
    sample_rate: f32,
    volume: f32,

    // Cached parameter values.
    filter_cutoff: f32,
    filter_resonance: f32,
    filter_type: FilterType,
    unison_voices: usize,
    detune_cents: f32,
    stereo_width: f32,
    glide_time: f32,
}

impl SynthEngine {
    pub fn new(sample_rate: f32) -> Self {
        let voices = std::array::from_fn(|_| Voice::new(sample_rate));
        Self {
            voices,
            pending: SmallVec::new(),
            sample_rate,
            volume: 0.8,
            filter_cutoff: 800.0,
            filter_resonance: 0.5,
            filter_type: FilterType::LowPass,
            unison_voices: 1,
            detune_cents: 10.0,
            stereo_width: 0.5,
            glide_time: 0.05,
        }
    }

    /// Queue a note-on event (processed by `process_events`).
    pub fn note_on(&mut self, note: u8, velocity: f32) {
        self.pending.push(NoteCmd::On { note, velocity });
    }

    /// Queue a note-off event.
    pub fn note_off(&mut self, note: u8) {
        self.pending.push(NoteCmd::Off { note });
    }

    /// Process all queued note commands, allocating and freeing voices.
    /// Call once per block **after** all MIDI events have been queued.
    pub fn process_events(&mut self) {
        // Collect commands first to end the mutable borrow on self.pending
        // before calling methods that borrow self again.
        let commands: SmallVec<[NoteCmd; 32]> = self.pending.drain(..).collect();
        for cmd in commands {
            match cmd {
                NoteCmd::On { note, velocity } => self.voice_on(note, velocity),
                NoteCmd::Off { note } => self.voice_off(note),
            }
        }
    }

    /// Update "slow" parameters: filter, envelope, unison, glide.
    ///
    /// Call this **once per block** before the sample loop.  Pushing these
    /// settings to all active voices is moderately expensive (it iterates over
    /// all voices), so doing it every sample is wasteful.
    #[allow(clippy::too_many_arguments)]
    pub fn update_block_params(
        &mut self,
        attack: f32,
        decay: f32,
        sustain: f32,
        release: f32,
        cutoff: f32,
        resonance: f32,
        filter_type: FilterType,
        unison: usize,
        detune: f32,
        width: f32,
        glide: f32,
    ) {
        self.filter_cutoff = cutoff;
        self.filter_resonance = resonance;
        self.filter_type = filter_type;
        self.unison_voices = unison.clamp(1, voice::MAX_UNISON);
        self.detune_cents = detune;
        self.stereo_width = width;
        self.glide_time = glide;

        // Push to all voices (including inactive ones so they pick up
        // settings when they are next triggered).
        for v in self.voices.iter_mut() {
            v.set_filter(cutoff, resonance, filter_type);
            v.set_unison(self.unison_voices, detune, width);
            v.env.set_attack(attack);
            v.env.set_decay(decay);
            v.env.set_sustain(sustain);
            v.env.set_release(release);
        }
    }

    /// Set the master volume.  Cheap enough to call per-sample alongside
    /// the smoother so volume automation remains sample-accurate.
    #[inline]
    pub fn set_volume(&mut self, vol: f32) {
        self.volume = vol;
    }

    /// Find a free voice or steal the best candidate.
    fn allocate_voice(&mut self) -> Option<usize> {
        // Prefer completely inactive voices.
        for (i, v) in self.voices.iter().enumerate() {
            if !v.active {
                return Some(i);
            }
        }
        // Then prefer finished (released) voices.
        for (i, v) in self.voices.iter().enumerate() {
            if v.finished() {
                return Some(i);
            }
        }
        // Steal the oldest voice by age.
        self.voices
            .iter()
            .enumerate()
            .max_by_key(|(_, v)| v.age)
            .map(|(i, _)| i)
    }

    fn voice_on(&mut self, note: u8, velocity: f32) {
        if let Some(idx) = self.allocate_voice() {
            self.voices[idx].note_on(note, velocity, self.sample_rate, self.glide_time);
            self.voices[idx].set_filter(
                self.filter_cutoff,
                self.filter_resonance,
                self.filter_type,
            );
            self.voices[idx].set_unison(self.unison_voices, self.detune_cents, self.stereo_width);
        }
    }

    fn voice_off(&mut self, note: u8) {
        for v in self.voices.iter_mut() {
            if v.active && v.note == note {
                v.note_off();
            }
        }
    }

    /// Process a single stereo sample. Returns `(left, right)`.
    #[inline]
    pub fn process_sample(&mut self, table: &SharedWavetable) -> (f32, f32) {
        let data = table.as_ref();
        let mut l = 0.0f32;
        let mut r = 0.0f32;

        let n = self.unison_voices;
        for v in self.voices.iter_mut() {
            let (vl, vr) = v.process(data, n);
            l += vl;
            r += vr;
        }

        let vol = self.volume;

        // Tanh soft-clip: smooth saturation at high amplitudes.
        // Under normal operating conditions the synth stays well below ±1.0
        // so this is transparent; it only activates as a last-resort safety
        // net if voices somehow still produce runaway values.
        let l_out = (l * vol).tanh();
        let r_out = (r * vol).tanh();
        (l_out, r_out)
    }

    /// Notify all voices of a sample-rate change.
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        for v in self.voices.iter_mut() {
            v.env.set_sample_rate(sr);
            v.filt.set_sample_rate(sr);
        }
    }

    /// Number of currently active voices (for UI display).
    #[allow(dead_code)]
    pub fn active_voice_count(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
}
