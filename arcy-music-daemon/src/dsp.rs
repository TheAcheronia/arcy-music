use rodio::Source;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use arcy_music_core::EqConfig;

/// Second-order Biquad IIR Filter
#[derive(Debug, Clone)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    // Channel state: (x1, x2, y1, y2)
    state: [(f32, f32, f32, f32); 2],
}

impl Biquad {
    pub fn new() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            state: [(0.0, 0.0, 0.0, 0.0); 2],
        }
    }

    /// Peaking EQ filter (RBJ Audio EQ Cookbook)
    pub fn set_peaking(&mut self, sample_rate: f32, freq: f32, q: f32, gain_db: f32) {
        if gain_db.abs() < 0.05 {
            // Flat pass-through
            self.b0 = 1.0;
            self.b1 = 0.0;
            self.b2 = 0.0;
            self.a1 = 0.0;
            self.a2 = 0.0;
            return;
        }

        let a = 10.0f32.powf(gain_db / 40.0);
        let omega = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cos_w = omega.cos();

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w;
        let a2 = 1.0 - alpha / a;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    pub fn process(&mut self, sample: f32, channel: usize) -> f32 {
        let ch = channel.min(1);
        let (x1, x2, y1, y2) = self.state[ch];

        let out = self.b0 * sample + self.b1 * x1 + self.b2 * x2 - self.a1 * y1 - self.a2 * y2;

        self.state[ch] = (sample, x1, out, y1);
        out
    }
}

/// 5-Band Graphic Equalizer:
/// Band 0: 60 Hz (Sub-Bass)
/// Band 1: 250 Hz (Bass / Punch)
/// Band 2: 1000 Hz (Midrange / Vocals)
/// Band 3: 4000 Hz (Presence / Treble)
/// Band 4: 12000 Hz (Air / Highs)
pub struct Equalizer {
    filters: [Biquad; 5],
    sample_rate: f32,
    pub spatial_mode: String,
}

impl Equalizer {
    pub const FREQUENCIES: [f32; 5] = [60.0, 250.0, 1000.0, 4000.0, 12000.0];

    pub fn new(sample_rate: f32) -> Self {
        Self {
            filters: std::array::from_fn(|_| Biquad::new()),
            sample_rate,
            spatial_mode: "off".to_string(),
        }
    }

    pub fn update_config(&mut self, config: &EqConfig) {
        self.spatial_mode = config.spatial_mode.clone();

        if !config.enabled {
            for f in &mut self.filters {
                f.b0 = 1.0;
                f.b1 = 0.0;
                f.b2 = 0.0;
                f.a1 = 0.0;
                f.a2 = 0.0;
            }
            return;
        }

        for (i, (&freq, &gain_db)) in Self::FREQUENCIES.iter().zip(config.bands.iter()).enumerate() {
            let clamped_gain = gain_db.clamp(-12.0, 12.0);
            self.filters[i].set_peaking(self.sample_rate, freq, 1.0, clamped_gain);
        }
    }

    #[inline]
    pub fn process(&mut self, mut sample: f32, channel: usize) -> f32 {
        for filter in &mut self.filters {
            sample = filter.process(sample, channel);
        }
        sample
    }
}

/// A Rodio Source wrapper that applies DSP equalizer filters, spatial surround rendering
/// (Dolby Atmos & Eclipsa IAMF), and acts as a hard limiter to protect speakers and ears.
pub struct SafeAudioWrapper<I> {
    inner: I,
    eq: Equalizer,
    eq_config: Arc<RwLock<EqConfig>>,
    channel_index: usize,
    num_channels: usize,
    sample_counter: usize,
    pending_right: Option<f32>,
    prev_left: f32,
    prev_right: f32,
}

impl<I> SafeAudioWrapper<I>
where
    I: Source<Item = f32>,
{
    pub fn new(inner: I, eq_config: Arc<RwLock<EqConfig>>) -> Self {
        let sample_rate = inner.sample_rate().get() as f32;
        let num_channels = inner.channels().get() as usize;
        let mut eq = Equalizer::new(sample_rate);
        if let Ok(cfg) = eq_config.read() {
            eq.update_config(&cfg);
        }
        Self {
            inner,
            eq,
            eq_config,
            channel_index: 0,
            num_channels,
            sample_counter: 0,
            pending_right: None,
            prev_left: 0.0,
            prev_right: 0.0,
        }
    }
}

impl<I> Iterator for SafeAudioWrapper<I>
where
    I: Source<Item = f32>,
{
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // Return pre-calculated right channel sample if present
        if let Some(r) = self.pending_right.take() {
            return Some(r);
        }

        let raw_left = self.inner.next()?;
        if raw_left.is_nan() || raw_left.is_infinite() {
            return Some(0.0);
        }

        // Live EQ update check every ~50ms (2048 samples)
        self.sample_counter = self.sample_counter.wrapping_add(1);
        if self.sample_counter % 2048 == 0 {
            if let Ok(cfg) = self.eq_config.read() {
                self.eq.update_config(&cfg);
            }
        }

        // If stereo and spatial processing is active
        if self.num_channels == 2 && self.eq.spatial_mode != "off" {
            let raw_right = match self.inner.next() {
                Some(r) if !r.is_nan() && !r.is_infinite() => r,
                Some(_) => 0.0,
                None => 0.0,
            };

            let left_eq = self.eq.process(raw_left, 0);
            let right_eq = self.eq.process(raw_right, 1);

            let (out_l, out_r) = match self.eq.spatial_mode.as_str() {
                "dolby_atmos" => {
                    let mid = (left_eq + right_eq) * 0.5;
                    let side = (left_eq - right_eq) * 0.5;
                    let wide_side = side * 1.35;
                    let l = mid + wide_side - self.prev_right * 0.07;
                    let r = mid - wide_side - self.prev_left * 0.07;
                    self.prev_left = left_eq;
                    self.prev_right = right_eq;
                    (l, r)
                }
                "eclipsa" => {
                    let mid = (left_eq + right_eq) * 0.5;
                    let side = (left_eq - right_eq) * 0.5;
                    let l = mid * 0.92 + side * 1.25 + self.prev_right * 0.06;
                    let r = mid * 0.92 - side * 1.25 + self.prev_left * 0.06;
                    self.prev_left = left_eq;
                    self.prev_right = right_eq;
                    (l, r)
                }
                _ => (left_eq, right_eq),
            };

            self.pending_right = Some(out_r.clamp(-1.0, 1.0));
            return Some(out_l.clamp(-1.0, 1.0));
        }

        // Standard or Mono playback
        let channel = self.channel_index;
        self.channel_index = (self.channel_index + 1) % self.num_channels;

        let processed = self.eq.process(raw_left, channel);

        // Preamp gain & Hard limiter [-1.0, 1.0]
        Some(processed.clamp(-1.0, 1.0))
    }
}

impl<I> Source for SafeAudioWrapper<I>
where
    I: Source<Item = f32>,
{
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    #[inline]
    fn channels(&self) -> std::num::NonZero<u16> {
        self.inner.channels()
    }

    #[inline]
    fn sample_rate(&self) -> std::num::NonZero<u32> {
        self.inner.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    #[inline]
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.channel_index = 0;
        self.pending_right = None;
        self.prev_left = 0.0;
        self.prev_right = 0.0;
        self.inner.try_seek(pos)
    }
}
