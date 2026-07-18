pub mod envelope;
pub mod filter;
pub mod voice;

use self::filter::FilterType;
use self::voice::Voice;
use crate::script::LuaContext;
use crate::script::ScriptMode;
use crate::wavetable::{SharedWavetable, WAVETABLE_SIZE};
use arc_swap::ArcSwap;
use smallvec::SmallVec;
#[cfg(feature = "time-buffer")]
use std::sync::Arc;

pub const MAX_VOICES: usize = 16;

// Lock-free, realtime-safe. Audio thread reads, background thread writes.
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

/// Lock-free slot for a pre-rendered time buffer.
#[cfg(feature = "time-buffer")]
pub struct TimeBufferSlot {
    inner: ArcSwap<Vec<f32>>,
}

#[cfg(feature = "time-buffer")]
impl TimeBufferSlot {
    pub fn new(buf: Vec<f32>) -> Self {
        Self {
            inner: ArcSwap::from_pointee(buf),
        }
    }
    #[inline]
    pub fn load(&self) -> arc_swap::Guard<Arc<Vec<f32>>> {
        self.inner.load()
    }
    pub fn store(&self, buf: Arc<Vec<f32>>) {
        self.inner.store(buf);
    }
}

#[derive(Debug, Clone, Copy)]
enum NoteCmd {
    On { note: u8, velocity: f32 },
    Off { note: u8 },
}

pub struct SynthEngine {
    voices: [Voice; MAX_VOICES],
    pending: SmallVec<[NoteCmd; 32]>,
    sample_rate: f32,
    volume: f32,
    filter_cutoff: f32,
    filter_resonance: f32,
    filter_type: FilterType,
    unison_voices: usize,
    detune_cents: f32,
    stereo_width: f32,
    glide_time: f32,
    pub script_mode: ScriptMode,
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
            script_mode: ScriptMode::Wavetable,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: f32) {
        self.pending.push(NoteCmd::On { note, velocity });
    }

    pub fn note_off(&mut self, note: u8) {
        self.pending.push(NoteCmd::Off { note });
    }

    pub fn process_events(&mut self) {
        let commands: SmallVec<[NoteCmd; 32]> = self.pending.drain(..).collect();
        for cmd in commands {
            match cmd {
                NoteCmd::On { note, velocity } => self.voice_on(note, velocity),
                NoteCmd::Off { note } => self.voice_off(note),
            }
        }
    }

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

        for v in self.voices.iter_mut() {
            v.set_filter(cutoff, resonance, filter_type);
            v.set_unison(self.unison_voices, detune, width);
            v.env.set_attack(attack);
            v.env.set_decay(decay);
            v.env.set_sustain(sustain);
            v.env.set_release(release);
        }
    }

    #[inline]
    pub fn set_volume(&mut self, vol: f32) {
        self.volume = vol;
    }

    fn allocate_voice(&mut self) -> Option<usize> {
        for (i, v) in self.voices.iter().enumerate() {
            if !v.active {
                return Some(i);
            }
        }
        for (i, v) in self.voices.iter().enumerate() {
            if v.finished() {
                return Some(i);
            }
        }
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

    #[inline]
    pub fn process_sample(
        &mut self,
        wavetable: &SharedWavetable,
        time_eval: Option<&LuaContext>,
    ) -> (f32, f32) {
        let mut l = 0.0f32;
        let mut r = 0.0f32;

        match self.script_mode {
            ScriptMode::Wavetable => {
                let data = wavetable.as_ref();
                let n = self.unison_voices;
                for v in self.voices.iter_mut() {
                    let (vl, vr) = v.process(data, n);
                    l += vl;
                    r += vr;
                }
            }
            ScriptMode::TimeBased => {
                if let Some(ctx) = time_eval {
                    for v in self.voices.iter_mut() {
                        let (vl, vr) = v.process_time(ctx);
                        l += vl;
                        r += vr;
                    }
                }
            }
        }

        let vol = self.volume;
        let l_out = (l * vol).tanh();
        let r_out = (r * vol).tanh();
        (l_out, r_out)
    }

    /// Buffer-based time-domain synthesis.
    #[cfg(feature = "time-buffer")]
    #[inline]
    pub fn process_sample_buf(
        &mut self,
        wavetable: &SharedWavetable,
        time_buf: Option<&[f32]>,
    ) -> (f32, f32) {
        match self.script_mode {
            ScriptMode::Wavetable => {
                let data = wavetable.as_ref();
                let n = self.unison_voices;
                let mut l = 0.0;
                let mut r = 0.0;
                for v in self.voices.iter_mut() {
                    let (vl, vr) = v.process(data, n);
                    l += vl;
                    r += vr;
                }
                let vol = self.volume;
                ((l * vol).tanh(), (r * vol).tanh())
            }
            ScriptMode::TimeBased => {
                if let Some(buf) = time_buf {
                    let mut l = 0.0;
                    let mut r = 0.0;
                    for v in self.voices.iter_mut() {
                        let (vl, vr) = v.process_time_buffer(buf);
                        l += vl;
                        r += vr;
                    }
                    let vol = self.volume;
                    ((l * vol).tanh(), (r * vol).tanh())
                } else {
                    (0.0, 0.0)
                }
            }
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        for v in self.voices.iter_mut() {
            v.sample_rate = sr;
            v.env.set_sample_rate(sr);
            v.filt.set_sample_rate(sr);
        }
    }

    #[allow(dead_code)]
    pub fn active_voice_count(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
}
