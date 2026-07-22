mod app;
pub mod knob_style;
mod popup;
pub mod theme;
mod waveform;

pub use app::OscillaGui;

use crate::OscillaParams;
use crate::OscillaTask;
#[cfg(feature = "time-buffer")]
use crate::dsp::TimeBufferSlot;
use crate::dsp::WavetableSlot;
use crate::dsp::filter::FilterType;
use crate::script::{ScriptCompiler, ScriptMode};
use crate::wavetable::SCOPE_SIZE;
use arc_swap::ArcSwap;
use iced_code_editor::CodeEditor;
use nice_plug::prelude::*;
use nice_plug_iced::iced::PollSubNotifier;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::Mutex;

/// Thread-safe handle for `CodeEditor`.
///
/// `CodeEditor` contains `Rc` and is `!Send`, but the editor is only ever
/// accessed from the GUI thread. This wrapper provides an `unsafe impl Send`
/// so it can be stored in `EditorState<T: Send>`.
pub struct EditorHandle(pub CodeEditor);

// SAFETY: EditorHandle is only ever accessed on the GUI thread.
unsafe impl Send for EditorHandle {}
unsafe impl Sync for EditorHandle {}

impl Deref for EditorHandle {
    type Target = CodeEditor;
    fn deref(&self) -> &CodeEditor {
        &self.0
    }
}

impl DerefMut for EditorHandle {
    fn deref_mut(&mut self) -> &mut CodeEditor {
        &mut self.0
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Poll,
    EditorEvent(iced_code_editor::Message),
    CompileScript,
    SelectAll,
    LoseEditorFocus,

    VolumeGestured(iced_audio::Gesture),
    AttackGestured(iced_audio::Gesture),
    DecayGestured(iced_audio::Gesture),
    SustainGestured(iced_audio::Gesture),
    ReleaseGestured(iced_audio::Gesture),
    CutoffGestured(iced_audio::Gesture),
    ResonanceGestured(iced_audio::Gesture),
    UnisonVoicesGestured(iced_audio::Gesture),
    DetuneGestured(iced_audio::Gesture),
    WidthGestured(iced_audio::Gesture),
    GlideGestured(iced_audio::Gesture),

    FilterTypeChanged(FilterType),
    ScriptModeChanged(ScriptMode),
    SavePreset,
    LoadPreset,
    SaveNameChanged(String),
    ConfirmSave,
    SelectPreset(String),
    ConfirmLoad,
    LoadFromList(String),
    ClosePopup,
}

pub struct OscillaEditorState {
    pub params: Arc<OscillaParams>,
    pub wavetable_slot: Arc<WavetableSlot>,
    pub lua_source_slot: Arc<ArcSwap<String>>,
    #[cfg(feature = "time-buffer")]
    pub time_buffer_slot: Arc<TimeBufferSlot>,
    pub peak_output: Arc<AtomicF32>,
    pub scope_buffer: Arc<Mutex<(Box<[f32; SCOPE_SIZE]>, usize)>>,
    pub notifier: PollSubNotifier,
    pub compiler: Arc<ScriptCompiler>,
    pub sample_rate: Arc<AtomicF32>,
    pub async_executor: Arc<dyn Fn(OscillaTask) + Send + Sync>,
    pub editor_handle: EditorHandle,
}
