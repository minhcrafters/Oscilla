use super::envelope::Adsr;
use super::filter::{FilterType, Svf};

pub const MAX_UNISON: usize = 7;

#[inline]
fn lerp_wave(table: &[f32; crate::wavetable::WAVETABLE_SIZE], phase: f32) -> f32 {
    let idx = phase as usize;
    let frac = phase - idx as f32;
    let a = table[idx];
    let b = table[(idx + 1) % crate::wavetable::WAVETABLE_SIZE];
    a + (b - a) * frac
}

pub struct Voice {
    pub phase: f32,
    pub inc: f32,
    pub inc_target: f32,
    pub glide_rate: f32,
    pub velocity: f32,
    pub cur_velocity: f32,
    pub note: u8,
    pub active: bool,
    pub age: u32,
    pub env: Adsr,
    pub filt: Svf,
    pub unison_phases: [f32; MAX_UNISON],
    pub unison_detunes: [f32; MAX_UNISON],
    pub unison_pans: [f32; MAX_UNISON],
    pub time_elapsed: f32,
    pub time_buf_pos: f32,
    pub sample_rate: f32,
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

            time_elapsed: 0.0,
            time_buf_pos: 0.0,
            sample_rate,
        }
    }

    #[inline]
    pub fn note_to_inc(note: u8, sample_rate: f32) -> f32 {
        440.0 * (2.0f32).powf((note as f32 - 69.0) / 12.0) / sample_rate
            * crate::wavetable::WAVETABLE_SIZE as f32
    }

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
        self.time_elapsed = 0.0;
        self.time_buf_pos = 0.0;
    }

    #[inline]
    pub fn note_off(&mut self) {
        self.env.note_off();
    }

    #[inline]
    pub fn finished(&self) -> bool {
        self.env.finished()
    }

    #[inline]
    pub fn set_filter(&mut self, cutoff: f32, res: f32, ftype: FilterType) {
        self.filt.set_cutoff(cutoff);
        self.filt.set_resonance(res);
        self.filt.set_type(ftype);
    }

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
        self.time_elapsed += 1.0 / self.sample_rate;

        if self.finished() {
            self.active = false;
        }

        (left, right)
    }

    /// Reads time-domain buffer at a rate proportional to note frequency (1× at A4).
    #[inline]
    pub fn process_time(&mut self, time_buf: &[f32]) -> (f32, f32) {
        if !self.active || time_buf.is_empty() {
            return (0.0, 0.0);
        }

        // Glide toward target increment.
        if (self.inc_target - self.inc).abs() > 0.0001 {
            self.inc = self.inc * self.glide_rate + self.inc_target * (1.0 - self.glide_rate);
        }

        // Compute the read-rate multiplier.
        // At A4 (note 69, 440 Hz), the wavetable increment is:
        //   inc = 440 * wavetable_size / sample_rate
        // For time-based, we want advance = 1.0 sample per sample at A4.
        // So: advance_per_sample = inc / inc_at_A4
        let inc_a4 = Voice::note_to_inc(69, self.sample_rate);
        let advance = if inc_a4 > 0.0 { self.inc / inc_a4 } else { 1.0 };

        self.time_buf_pos += advance;
        let buf_len = time_buf.len() as f32;

        // Wrap around for looping.
        while self.time_buf_pos >= buf_len {
            self.time_buf_pos -= buf_len;
        }
        while self.time_buf_pos < 0.0 {
            self.time_buf_pos += buf_len;
        }

        // Linear interpolation.
        let idx = self.time_buf_pos as usize;
        let frac = self.time_buf_pos - idx as f32;
        let a = time_buf[idx];
        let b = time_buf[(idx + 1) % time_buf.len()];
        let sample = a + (b - a) * frac;

        // Stereo pan (center only — unison doesn't apply in time-based mode).
        let mut left = sample;
        let mut right = sample;

        // Apply envelope and velocity.
        let env_val = self.env.tick();
        let vel = self.cur_velocity;
        let amp = env_val * vel * vel;

        left *= amp;
        right *= amp;

        // Apply per-voice filter.
        (left, right) = self.filt.process_stereo(left, right);

        left = left.clamp(-4.0, 4.0);
        right = right.clamp(-4.0, 4.0);

        self.age += 1;
        self.time_elapsed += 1.0 / self.sample_rate;

        if self.finished() {
            self.active = false;
        }

        (left, right)
    }
}
