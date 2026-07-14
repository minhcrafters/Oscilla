//! Single polyphonic voice.
//!
//! Each voice has a phase accumulator for wavetable lookup, its own
//! ADSR envelope, a state-variable filter, and optional unison sub-oscillators
//! with detune and stereo spread.

use super::envelope::Adsr;
use super::filter::{FilterType, Svf};

/// Maximum number of unison voices per note.
pub const MAX_UNISON: usize = 7;

/// Interpolation between wavetable samples.
#[inline]
fn lerp_wave(table: &[f32; crate::wavetable::WAVETABLE_SIZE], phase: f32) -> f32 {
    let idx = phase as usize;
    let frac = phase - idx as f32;
    let a = table[idx];
    let b = table[(idx + 1) % crate::wavetable::WAVETABLE_SIZE];
    a + (b - a) * frac
}

/// Per-voice state machine.
pub struct Voice {
    /// Sample index into the phase-normalized table (0..2048).
    pub phase: f32,
    /// Per-sample phase increment.
    pub inc: f32,
    /// Target increment (for glide).
    pub inc_target: f32,
    /// Glide rate per sample (0 = instant, < 1 = slow).
    pub glide_rate: f32,

    /// Velocity scaling (0..1).
    pub velocity: f32,
    /// Current velocity scaling (smoothed for glide).
    pub cur_velocity: f32,

    /// MIDI note number (for tracking).
    pub note: u8,
    /// Whether this voice is currently playing.
    pub active: bool,
    /// Age counter for voice stealing.
    pub age: u32,

    /// Amplitude envelope.
    pub env: Adsr,
    /// Per-voice filter.
    pub filt: Svf,

    /// Unison sub-oscillator phases.
    pub unison_phases: [f32; MAX_UNISON],
    /// Pre-computed detune factors for each unison voice.
    pub unison_detunes: [f32; MAX_UNISON],
    /// Pan values for each unison voice (-1..1).
    pub unison_pans: [f32; MAX_UNISON],
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            inc: 0.0,
            inc_target: 0.0,
            glide_rate: 1.0,
            velocity: 1.0,
            cur_velocity: 1.0,
            note: 0,
            active: false,
            age: 0,
            env: Adsr::new(sample_rate),
            filt: Svf::new(sample_rate),
            unison_phases: [0.0; MAX_UNISON],
            unison_detunes: [1.0; MAX_UNISON],
            unison_pans: [0.0; MAX_UNISON],
        }
    }

    /// Compute phase increment from a MIDI note number and sample rate.
    #[inline]
    pub fn note_to_inc(note: u8, sample_rate: f32) -> f32 {
        440.0 * (2.0f32).powf((note as f32 - 69.0) / 12.0) / sample_rate
            * crate::wavetable::WAVETABLE_SIZE as f32
    }

    /// Start a note on this voice.
    pub fn note_on(&mut self, note: u8, vel: f32, sr: f32, glide: f32) {
        let target_inc = Self::note_to_inc(note, sr);
        self.note = note;
        self.velocity = vel;

        if self.active && glide < 0.001 {
            // No glide: instant note change while keeping phase continuity.
            self.inc_target = target_inc;
            self.glide_rate = 1.0; // instant
        } else if !self.active || glide < 0.001 {
            // First note or no glide.
            self.inc = target_inc;
            self.inc_target = target_inc;
            self.glide_rate = 1.0;
            self.phase = 0.0;
            self.cur_velocity = vel;
            self.env.note_on();
        } else {
            // Glide from current note.
            self.inc_target = target_inc;
            // glide_rate: samples to reach target. Lower = slower.
            let samples = (glide * sr).max(1.0);
            self.glide_rate = 1.0 - (1.0 / samples).min(0.999);
            self.cur_velocity = self.velocity.min(vel); // fade toward new velocity
            self.env.note_on();
        }

        self.active = true;
        self.age = 0;
    }

    /// Release the note.
    #[inline]
    pub fn note_off(&mut self) {
        self.env.note_off();
    }

    /// Return true when the envelope has finished and voice can be stolen.
    #[inline]
    pub fn finished(&self) -> bool {
        self.env.finished()
    }

    /// Set filter parameters from shared settings.
    #[inline]
    pub fn set_filter(&mut self, cutoff: f32, res: f32, ftype: FilterType) {
        self.filt.set_cutoff(cutoff);
        self.filt.set_resonance(res);
        self.filt.set_type(ftype);
    }

    /// Set unison parameters. Recomputes detune factors and pan values.
    pub fn set_unison(&mut self, voices: usize, detune_cents: f32, width: f32) {
        let n = voices.clamp(1, MAX_UNISON);
        let detune_semitones = detune_cents / 100.0;

        for i in 0..n {
            if n == 1 {
                self.unison_detunes[i] = 1.0;
                self.unison_pans[i] = 0.0;
            } else {
                // Evenly space detune across voices.
                let t = if n == 1 {
                    0.0
                } else {
                    (i as f32 / (n - 1) as f32) * 2.0 - 1.0
                };
                self.unison_detunes[i] = (2.0f32).powf(t * detune_semitones / 12.0);
                self.unison_pans[i] = t * width;
            }
        }
        // Fill remaining with identity.
        for i in n..MAX_UNISON {
            self.unison_detunes[i] = 1.0;
            self.unison_pans[i] = 0.0;
        }
    }

    /// Process one sample for this voice. Returns (left, right) output.
    /// Pan values are mapped linearly.
    #[inline]
    pub fn process(
        &mut self,
        table: &[f32; crate::wavetable::WAVETABLE_SIZE],
        unison_voices: usize,
    ) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }

        // Glide toward target increment.
        if (self.inc_target - self.inc).abs() > 0.0001 {
            self.inc = self.inc * self.glide_rate + self.inc_target * (1.0 - self.glide_rate);
        }

        let n_voices = unison_voices.clamp(1, MAX_UNISON);
        let mut left = 0.0f32;
        let mut right = 0.0f32;

        for i in 0..n_voices {
            // Accumulate phase with correct wraparound (handles negatives too).
            self.unison_phases[i] += self.inc * self.unison_detunes[i];
            let wt_size = crate::wavetable::WAVETABLE_SIZE as f32;
            if self.unison_phases[i] >= wt_size {
                self.unison_phases[i] -= wt_size;
            } else if self.unison_phases[i] < 0.0 {
                self.unison_phases[i] += wt_size;
            }

            let sample = lerp_wave(table, self.unison_phases[i]);

            // Stereo pan.
            let pan = self.unison_pans[i];
            let gain_l = ((1.0 - pan) * 0.5).sqrt();
            let gain_r = ((1.0 + pan) * 0.5).sqrt();

            left += sample * gain_l;
            right += sample * gain_r;
        }

        // Scale by number of voices (power-preserving).
        let norm = 1.0 / (n_voices as f32).sqrt();
        left *= norm;
        right *= norm;

        // Apply envelope and velocity.
        let env_val = self.env.tick();
        let vel = self.cur_velocity;
        let amp = env_val * vel * vel; // velocity squared for musical feel

        left *= amp;
        right *= amp;

        // Apply per-voice filter via mid/side to avoid double-feedback.
        (left, right) = self.filt.process_stereo(left, right);

        // Hard output clamp — last-resort safety net.
        // A well-behaved filter should never reach ±4.0, but if it does
        // (e.g. during a rapid cutoff sweep) this prevents DAW clipping
        // or speaker damage.
        left = left.clamp(-4.0, 4.0);
        right = right.clamp(-4.0, 4.0);

        self.age += 1;

        if self.finished() {
            self.active = false;
        }

        (left, right)
    }
}
