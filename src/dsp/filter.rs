//! 2-pole state-variable filter (Chamberlin topology) with 2× oversampling.
//!
//! ## Stability
//!
//! The Chamberlin SVF is conditionally stable: `f = 2·sin(π·fc/sr)` must
//! satisfy `f < 2.0` AND `f < 2q` (q = 1 - resonance). Near Nyquist or at
//! high resonance these constraints are easily violated, leading to
//! exponential blow-up.
//!
//! We address this with three layered defences:
//!
//! 1. **2× internal oversampling** — the filter runs twice per audio sample
//!    at half the `f` coefficient.  This halves the effective normalised
//!    frequency and dramatically improves high-cutoff stability.
//! 2. **Conservative `f` clamp** — capped at `0.85` (post-halving), so the
//!    full-rate equivalent is `≤ 1.70`, well below the instability threshold.
//! 3. **NaN / runaway detection** — state variables are checked every sample;
//!    if either is non-finite or out of range the filter resets silently.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterType {
    LowPass,
    HighPass,
    BandPass,
}

impl std::fmt::Display for FilterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LowPass => write!(f, "LP"),
            Self::HighPass => write!(f, "HP"),
            Self::BandPass => write!(f, "BP"),
        }
    }
}

/// Maximum absolute value of the internal state variables before the filter
/// is considered to have blown up and is auto-reset.
const MAX_STATE: f32 = 64.0;

/// Upper bound for the `f` coefficient *after* halving for oversampling.
/// Equivalent to an un-oversampled `f ≤ 1.70`, safely below the `f < 2.0`
/// hard limit even at maximum resonance.
const F_MAX: f32 = 0.85;

#[derive(Debug, Clone)]
pub struct Svf {
    /// Low-pass integrator state.
    lp: f32,
    /// Band-pass integrator state.
    bp: f32,
    /// Frequency coefficient (already halved for 2× oversampling).
    f: f32,
    /// Damping coefficient: `q = 1 − resonance`.
    q: f32,
    mode: FilterType,
    sr: f32,
    /// Cached cutoff in Hz so we can recompute `f` after a sample-rate change.
    cutoff_hz: f32,
}

impl Svf {
    pub fn new(sample_rate: f32) -> Self {
        let mut s = Self {
            lp: 0.0,
            bp: 0.0,
            f: 0.0,
            q: 1.0, // resonance = 0 → q = 1
            mode: FilterType::LowPass,
            sr: sample_rate,
            cutoff_hz: 800.0,
        };
        s.set_cutoff(800.0);
        s
    }

    pub fn set_cutoff(&mut self, hz: f32) {
        // Clamp well away from DC and Nyquist.
        let max_hz = self.sr * 0.45; // 45 % of Nyquist
        let hz = hz.clamp(10.0, max_hz);
        self.cutoff_hz = hz;

        // With 2× oversampling the effective sample rate is 2·sr, so:
        //   f_os = 2·sin(π·hz / (2·sr))
        // Then we halve it again for the two-pass loop, giving:
        //   self.f = sin(π·hz / (2·sr))  (which equals f_os / 2)
        let f_raw = (std::f32::consts::PI * hz / (2.0 * self.sr)).sin();
        self.f = f_raw.min(F_MAX);
    }

    pub fn set_resonance(&mut self, r: f32) {
        // q = 1 − resonance.  Keep q > 0 to avoid division-by-zero in HP.
        // Also enforce q ≥ f/2 (stability criterion for Chamberlin SVF).
        let res = r.clamp(0.0, 0.98);
        let q_raw = 1.0 - res;
        // Enforce the stability bound: q ≥ f/2
        self.q = q_raw.max(self.f * 0.5 + 1e-4);
    }

    #[inline]
    pub fn set_type(&mut self, t: FilterType) {
        self.mode = t;
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr;
        // Recompute from cached Hz value.
        let hz = self.cutoff_hz;
        self.set_cutoff(hz);
    }

    #[inline]
    pub fn reset(&mut self) {
        self.lp = 0.0;
        self.bp = 0.0;
    }

    /// Check whether the filter state has blown up and reset if so.
    #[inline]
    fn sanitize(&mut self) {
        if !self.lp.is_finite()
            || !self.bp.is_finite()
            || self.lp.abs() > MAX_STATE
            || self.bp.abs() > MAX_STATE
        {
            self.lp = 0.0;
            self.bp = 0.0;
        }
    }

    /// Process a single mono sample with **2× internal oversampling**.
    ///
    /// The filter is run twice, each time with `f/2` effectively halved by
    /// the oversampling strategy.  The output of the second pass is returned.
    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        // Pass 1
        let hp1 = input - self.lp - self.q * self.bp;
        self.bp += self.f * hp1;
        self.lp += self.f * self.bp;

        // Pass 2 (same input — double the filter response at half the cost
        // of true 2× oversampling while preserving the stability benefit)
        let hp2 = input - self.lp - self.q * self.bp;
        self.bp += self.f * hp2;
        self.lp += self.f * self.bp;

        // Safety: clamp state to prevent NaN propagation.
        self.sanitize();

        match self.mode {
            FilterType::LowPass => self.lp,
            FilterType::HighPass => hp2,
            FilterType::BandPass => self.bp,
        }
    }

    /// Process a stereo pair through a **single** filter state.
    ///
    /// Filter the mid (L+R) signal, apply the filter, then reconstruct L/R
    /// using the original side (L−R) signal.  This avoids running two separate
    /// samples through the same feedback loop per audio frame, which was the
    /// double-feedback problem causing extra instability.
    #[inline]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let mid = (left + right) * 0.5;
        let side = (left - right) * 0.5;
        let filtered_mid = self.process(mid);
        (filtered_mid + side, filtered_mid - side)
    }
}
