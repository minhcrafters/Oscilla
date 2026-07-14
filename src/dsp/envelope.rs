//! ADSR envelope generator.
//!
//! A standard attack-decay-sustain-release envelope. Time values are in
//! seconds and converted to per-sample increments based on the sample rate.
//! All processing is branch-light and allocation-free.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone)]
pub struct Adsr {
    attack_inc: f32,
    decay_inc: f32,
    sustain_level: f32,
    release_inc: f32,

    /// Raw time values stored so `set_sample_rate` can recompute correctly.
    attack_secs: f32,
    decay_secs: f32,
    release_secs: f32,

    value: f32,
    phase: Phase,
    sr: f32,
}

impl Adsr {
    pub fn new(sample_rate: f32) -> Self {
        let mut e = Self {
            attack_inc: 0.0,
            decay_inc: 0.0,
            sustain_level: 0.7,
            release_inc: 0.0,
            attack_secs: 0.01,
            decay_secs: 0.1,
            release_secs: 0.3,
            value: 0.0,
            phase: Phase::Idle,
            sr: sample_rate,
        };
        e.set_attack(0.01);
        e.set_decay(0.1);
        e.set_sustain(0.7);
        e.set_release(0.3);
        e
    }

    pub fn set_attack(&mut self, secs: f32) {
        self.attack_secs = secs.max(0.0001);
        self.attack_inc = if secs > 0.0001 {
            1.0 / (secs * self.sr)
        } else {
            1.0
        };
    }

    pub fn set_decay(&mut self, secs: f32) {
        self.decay_secs = secs.max(0.0001);
        self.decay_inc = if secs > 0.0001 {
            1.0 / (secs * self.sr)
        } else {
            1.0
        };
    }

    pub fn set_sustain(&mut self, level: f32) {
        self.sustain_level = level.clamp(0.0, 1.0);
    }

    pub fn set_release(&mut self, secs: f32) {
        self.release_secs = secs.max(0.0001);
        self.release_inc = if secs > 0.0001 {
            1.0 / (secs * self.sr)
        } else {
            1.0
        };
    }

    /// Update the sample rate and recompute all per-sample increments from the
    /// stored raw time values.  Safe to call even if time values were never
    /// explicitly set (they default to the constructor values).
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr;
        let a = self.attack_secs;
        let d = self.decay_secs;
        let r = self.release_secs;
        self.set_attack(a);
        self.set_decay(d);
        self.set_release(r);
    }

    #[inline]
    pub fn note_on(&mut self) {
        self.phase = Phase::Attack;
    }

    #[inline]
    pub fn note_off(&mut self) {
        self.phase = Phase::Release;
    }

    #[inline]
    pub fn finished(&self) -> bool {
        self.phase == Phase::Idle
    }

    #[inline]
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Advance one sample. Returns the current envelope level.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        match self.phase {
            Phase::Idle => {
                self.value = 0.0;
            }
            Phase::Attack => {
                self.value += self.attack_inc;
                if self.value >= 1.0 {
                    self.value = 1.0;
                    self.phase = Phase::Decay;
                }
            }
            Phase::Decay => {
                self.value -= self.decay_inc * (1.0 - self.sustain_level);
                if self.value <= self.sustain_level {
                    self.value = self.sustain_level;
                    self.phase = Phase::Sustain;
                }
            }
            Phase::Sustain => {
                // hold
            }
            Phase::Release => {
                self.value -= self.release_inc;
                if self.value <= 0.0 {
                    self.value = 0.0;
                    self.phase = Phase::Idle;
                }
            }
        }
        self.value
    }
}
