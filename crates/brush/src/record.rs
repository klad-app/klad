use klad_core::stroke::{PRESSURE_STEPS, Sample};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PressureCurve {
    pub low: f32,
    pub high: f32,
    pub gamma: f32,
}

impl Default for PressureCurve {
    fn default() -> Self {
        Self {
            low: 0.0,
            high: 1.0,
            gamma: 1.0,
        }
    }
}

impl PressureCurve {
    pub fn apply(&self, raw: f32) -> f32 {
        if raw <= 0.0 {
            return 0.0;
        }
        let span = (self.high - self.low).max(0.01);
        let t = ((raw - self.low) / span).clamp(0.0, 1.0);
        libm::powf(t, self.gamma).max(1.0 / PRESSURE_STEPS)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Raw {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub tilt: (f32, f32),
    pub dt_ms: f32,
}

pub struct Recorder {
    curve: PressureCurve,
    window_ms: f32,
    smoothing_ms: f32,
    recent: VecDeque<(f32, Raw)>,
    now: f32,
    pressure: Option<f32>,
}

impl Recorder {
    pub fn new(curve: PressureCurve, window_ms: f32, smoothing_ms: f32) -> Self {
        Self {
            curve,
            window_ms: window_ms.max(0.0),
            smoothing_ms: smoothing_ms.max(0.0),
            recent: VecDeque::new(),
            now: 0.0,
            pressure: None,
        }
    }

    pub fn push(&mut self, raw: Raw) -> Sample {
        let dt = raw.dt_ms.max(0.0);
        self.now += dt;
        let target = self.curve.apply(raw.pressure);
        let pressure = match self.pressure {
            Some(prev) if self.smoothing_ms > 0.0 => {
                prev + (target - prev) * (1.0 - libm::expf(-dt / self.smoothing_ms))
            }
            _ => target,
        };
        self.pressure = Some(pressure);
        self.recent.push_back((self.now, raw));
        while self.recent.len() > 1 && self.recent.front().is_some_and(|(t, _)| self.now - t > self.window_ms) {
            self.recent.pop_front();
        }
        let (x, y) = self.centre();
        Sample::quantize(x, y, pressure, raw.tilt, dt)
    }

    fn centre(&self) -> (f32, f32) {
        let n = self.recent.len().max(1) as f32;
        let (x, y) = self
            .recent
            .iter()
            .fold((0.0f32, 0.0f32), |(x, y), (_, r)| (x + r.x, y + r.y));
        (x / n, y / n)
    }

    pub fn finish(&mut self) -> Vec<Sample> {
        let pressure = self.pressure.unwrap_or(0.0);
        let tilt = self.recent.back().map_or((0.0, 0.0), |(_, r)| r.tilt);
        let mut out = Vec::new();
        while self.recent.len() > 1 {
            self.recent.pop_front();
            let (x, y) = self.centre();
            out.push(Sample::quantize(x, y, pressure, tilt, 0.0));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(x: f32, y: f32, pressure: f32) -> Raw {
        Raw {
            x,
            y,
            pressure,
            tilt: (0.0, 0.0),
            dt_ms: 4.5,
        }
    }

    #[test]
    fn calibration_reaches_full_pressure_and_keeps_zero_for_no_reading() {
        let heavy = PressureCurve {
            low: 0.0,
            high: 0.88,
            gamma: 1.0,
        };
        assert_eq!(heavy.apply(0.88), 1.0);
        assert_eq!(heavy.apply(0.95), 1.0);
        assert!((heavy.apply(0.44) - 0.5).abs() < 1e-6);
        assert_eq!(heavy.apply(0.0), 0.0);
        let floor = PressureCurve {
            low: 0.2,
            high: 0.8,
            gamma: 2.0,
        };
        let light = Sample::quantize(0.0, 0.0, floor.apply(0.05), (0.0, 0.0), 0.0);
        assert_eq!(light.pressure, 1);
        assert!(floor.apply(0.5) < 0.5 * floor.apply(0.8));
    }

    #[test]
    fn without_filters_samples_pass_straight_through() {
        let mut rec = Recorder::new(PressureCurve::default(), 0.0, 0.0);
        let points = [raw(1.0, 2.0, 0.3), raw(5.5, 2.25, 0.6), raw(9.0, 4.0, 0.2)];
        let out: Vec<Sample> = points.iter().map(|p| rec.push(*p)).collect();
        let direct: Vec<Sample> = points
            .iter()
            .map(|p| Sample::quantize(p.x, p.y, p.pressure, p.tilt, p.dt_ms))
            .collect();
        assert_eq!(out, direct);
        assert!(rec.finish().is_empty());
    }

    #[test]
    fn the_stabiliser_irons_out_wobble_and_catches_up_on_pen_up() {
        let wobbly: Vec<Raw> = (0..200)
            .map(|i| raw(i as f32 * 2.0, 100.0 + if i % 2 == 0 { 3.0 } else { -3.0 }, 0.5))
            .collect();
        let mut rec = Recorder::new(PressureCurve::default(), 40.0, 0.0);
        let mut out: Vec<Sample> = wobbly.iter().map(|r| rec.push(*r)).collect();
        let lagging = out.last().unwrap().pos().0;
        assert!(lagging < 398.0 - 5.0, "lags behind the pen: {lagging}");
        out.extend(rec.finish());
        let wobble = out[20..out.len() - 20]
            .iter()
            .map(|s| (s.pos().1 - 100.0).abs())
            .fold(0.0f32, f32::max);
        assert!(wobble < 0.6, "wobble {wobble}");
        let end = out.last().unwrap().pos();
        assert_eq!(end, (398.0, 97.0));
    }

    #[test]
    fn pressure_smoothing_calms_jitter_without_losing_the_level() {
        let jittery: Vec<Raw> = (0..120)
            .map(|i| raw(i as f32, 0.0, if i % 2 == 0 { 0.4 } else { 0.6 }))
            .collect();
        let mut rec = Recorder::new(PressureCurve::default(), 0.0, 20.0);
        let out: Vec<f32> = jittery.iter().map(|r| rec.push(*r).pressure()).collect();
        let tail = &out[60..];
        let (lo, hi) = tail
            .iter()
            .fold((1.0f32, 0.0f32), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        assert!(hi - lo < 0.05, "range {lo}..{hi}");
        assert!((tail.iter().sum::<f32>() / tail.len() as f32 - 0.5).abs() < 0.02);
    }
}
