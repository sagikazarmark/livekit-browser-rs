use super::*;

use super::local_audio::{LocalAudioTrackInner, LocalAudioTrackLifecycle};
use crate::room::RemoteMediaTrackInner;

/// One owned level and waveform sample from an [`AudioAnalyser`].
#[derive(Clone, Debug, PartialEq)]
pub struct AudioAnalysis {
    level: f32,
    waveform: Vec<f32>,
}

impl AudioAnalysis {
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn from_waveform(waveform: Vec<f32>) -> Self {
        let mean_square = if waveform.is_empty() {
            0.0
        } else {
            waveform
                .iter()
                .map(|sample| f64::from(*sample) * f64::from(*sample))
                .sum::<f64>()
                / waveform.len() as f64
        };
        Self {
            level: mean_square.sqrt().clamp(0.0, 1.0) as f32,
            waveform,
        }
    }

    /// Returns the root-mean-square level normalized to `0.0..=1.0`.
    pub fn level(&self) -> f32 {
        self.level
    }

    /// Returns normalized time-domain samples owned by this Rust frame.
    pub fn waveform(&self) -> &[f32] {
        &self.waveform
    }

    /// Consumes this frame and returns its owned normalized time-domain samples.
    pub fn into_waveform(self) -> Vec<f32> {
        self.waveform
    }
}

/// A browser-thread-local Web Audio analyser for one exact Local or Remote Media Track source.
///
/// Clones share one Web Audio graph. Explicit close is terminal for every clone, and dropping the
/// final clone releases the graph as a fallback. Ending or replacing the source also terminates the
/// analyser.
#[derive(Clone)]
pub struct AudioAnalyser {
    inner: Rc<AudioAnalyserInner>,
}

impl AudioAnalyser {
    /// Whether this analyser and its exact source track remain available.
    pub fn is_active(&self) -> bool {
        self.inner.state.active.get() && self.inner.source.is_active()
    }

    /// Reads one owned Rust frame containing level and waveform data.
    // The body dispatches on `cfg`, so each branch returns explicitly rather than relying on which
    // branch happens to be compiled last.
    #[allow(clippy::needless_return)]
    pub fn read(&self) -> Result<AudioAnalysis, Error> {
        if !self.is_active() {
            self.inner.state.active.set(false);
            #[cfg(target_arch = "wasm32")]
            adapter::drop_audio_analyser(self.inner.analyser_id);
            return Err(Error::InvalidState {
                operation: "read Audio Analyser",
                reason: "Audio Analyser or source track is terminal",
            });
        }

        #[cfg(target_arch = "wasm32")]
        {
            let samples = adapter::read_audio_analyser(self.inner.analyser_id)
                .map_err(AdapterError::from_cause)
                .map_err(Error::AudioAnalysisFailed)?;
            return Ok(AudioAnalysis::from_waveform(samples.to_vec()));
        }

        #[cfg(all(not(target_arch = "wasm32"), test))]
        return Ok(AudioAnalysis::from_waveform(vec![0.0]));

        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        Err(Error::UnsupportedTarget)
    }

    /// Makes every clone terminal and releases its Web Audio graph.
    ///
    /// Terminal state is installed when this method is called. Await the returned local future for
    /// deterministic `AudioContext` closure.
    pub fn close(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        let newly_closed = self.inner.state.active.replace(false);
        #[cfg(target_arch = "wasm32")]
        if newly_closed {
            self.inner.start_close();
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = newly_closed;

        let _inner = Rc::clone(&self.inner);
        Box::pin(async move {
            #[cfg(target_arch = "wasm32")]
            _inner
                .await_close()
                .await
                .map_err(Error::AudioAnalysisFailed)?;
            Ok(())
        })
    }

    #[cfg(all(not(target_arch = "wasm32"), test))]
    pub(crate) fn test_analyser(source: AudioAnalyserSource) -> Self {
        let state = Rc::new(AudioAnalyserState {
            active: Cell::new(true),
        });
        source.register(&state);
        Self {
            inner: Rc::new(AudioAnalyserInner { state, source }),
        }
    }
}

impl fmt::Debug for AudioAnalyser {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AudioAnalyser")
            .field("active", &self.is_active())
            .finish()
    }
}

pub(crate) struct AudioAnalyserState {
    active: Cell<bool>,
}

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn register_audio_analyser(
    analysers: &RefCell<Vec<Weak<AudioAnalyserState>>>,
    state: &Rc<AudioAnalyserState>,
) {
    analysers
        .borrow_mut()
        .retain(|analyser| analyser.strong_count() > 0);
    analysers.borrow_mut().push(Rc::downgrade(state));
}

pub(crate) fn terminate_audio_analysers(analysers: &RefCell<Vec<Weak<AudioAnalyserState>>>) {
    for analyser in analysers.borrow_mut().drain(..) {
        if let Some(analyser) = analyser.upgrade() {
            analyser.active.set(false);
        }
    }
}

pub(crate) enum AudioAnalyserSource {
    Local(Weak<LocalAudioTrackInner>),
    Remote(Weak<RemoteMediaTrackInner>),
}

impl AudioAnalyserSource {
    fn is_active(&self) -> bool {
        match self {
            Self::Local(track) => track
                .upgrade()
                .is_some_and(|track| track.lifecycle.get() == LocalAudioTrackLifecycle::Active),
            Self::Remote(track) => track.upgrade().is_some_and(|track| !track.is_terminal()),
        }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    fn register(&self, state: &Rc<AudioAnalyserState>) {
        match self {
            Self::Local(track) => {
                if let Some(track) = track.upgrade() {
                    register_audio_analyser(&track.analysers, state);
                }
            }
            Self::Remote(track) => {
                if let Some(track) = track.upgrade() {
                    register_audio_analyser(&track.analysers, state);
                }
            }
        }
    }
}

struct AudioAnalyserInner {
    state: Rc<AudioAnalyserState>,
    source: AudioAnalyserSource,
    #[cfg(target_arch = "wasm32")]
    analyser_id: u32,
    #[cfg(target_arch = "wasm32")]
    close_operation: RefCell<Option<Result<js_sys::Promise, AdapterError>>>,
    #[cfg(target_arch = "wasm32")]
    _terminal_callback: wasm_bindgen::closure::Closure<dyn Fn()>,
}

#[cfg(target_arch = "wasm32")]
impl AudioAnalyserInner {
    fn start_close(&self) {
        let operation =
            adapter::close_audio_analyser(self.analyser_id).map_err(AdapterError::from_cause);
        self.close_operation.borrow_mut().replace(operation);
    }

    async fn await_close(&self) -> Result<(), AdapterError> {
        let operation = self.close_operation.borrow().clone();
        let Some(operation) = operation else {
            return Ok(());
        };
        match operation {
            Ok(operation) => wasm_bindgen_futures::JsFuture::from(operation)
                .await
                .map(|_| ())
                .map_err(AdapterError::from_cause),
            Err(error) => Err(error),
        }
    }
}

impl Drop for AudioAnalyserInner {
    fn drop(&mut self) {
        self.state.active.set(false);
        #[cfg(target_arch = "wasm32")]
        adapter::drop_audio_analyser(self.analyser_id);
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn create_audio_analyser(
    source: AudioAnalyserSource,
    create: impl FnOnce(&js_sys::Function) -> Result<u32, wasm_bindgen::JsValue>,
) -> Result<AudioAnalyser, Error> {
    use wasm_bindgen::JsCast;

    let state = Rc::new(AudioAnalyserState {
        active: Cell::new(true),
    });
    let weak_state = Rc::downgrade(&state);
    let callback = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
        if let Some(state) = weak_state.upgrade() {
            state.active.set(false);
        }
    }) as Box<dyn Fn()>);
    let analyser_id = create(callback.as_ref().unchecked_ref())
        .map_err(AdapterError::from_cause)
        .map_err(Error::AudioAnalysisFailed)?;
    source.register(&state);
    Ok(AudioAnalyser {
        inner: Rc::new(AudioAnalyserInner {
            state,
            source,
            analyser_id,
            close_operation: RefCell::new(None),
            _terminal_callback: callback,
        }),
    })
}
