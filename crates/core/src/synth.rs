use crate::color::Rgba;
use crate::rng::Rng;
use crate::stroke::{Sample, Stroke};
use libm::{cosf, sinf};

#[derive(Clone, Debug)]
pub struct Synth {
    pub width: f32,
    pub height: f32,
    pub rate_hz: f32,
    pub min_len: f32,
    pub max_len: f32,
    pub sizes: Vec<f32>,
    pub brushes: Vec<String>,
    pub palette: Vec<Rgba>,
}

#[derive(Clone, Copy, Debug)]
pub struct Gesture {
    pub start: (f32, f32),
    pub heading: f32,
    pub length: f32,
    pub speed: f32,
    pub curl: f32,
    pub pressure: f32,
    pub attack: f32,
    pub release: f32,
    pub tilt: (f32, f32),
}

impl Synth {
    pub fn canvas(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            rate_hz: 200.0,
            min_len: 15.0,
            max_len: 600.0,
            sizes: vec![4.0, 10.0, 24.0, 60.0],
            brushes: ["pencil", "ink", "soft", "chalk"].map(String::from).to_vec(),
            palette: vec![
                [0.12, 0.10, 0.14, 1.0],
                [0.85, 0.80, 0.70, 1.0],
                [0.30, 0.45, 0.62, 1.0],
                [0.72, 0.35, 0.28, 1.0],
                [0.40, 0.55, 0.32, 1.0],
            ],
        }
    }

    pub fn strokes(&self, count: usize, seed: u64) -> Vec<Stroke> {
        let mut rng = Rng::new(seed);
        (0..count).map(|i| self.stroke(i as u32, &mut rng)).collect()
    }

    pub fn stroke(&self, id: u32, rng: &mut Rng) -> Stroke {
        let t = rng.unit();
        let length = self.min_len + (self.max_len - self.min_len) * t * t;
        let gesture = Gesture {
            start: (rng.range(0.0, self.width), rng.range(0.0, self.height)),
            heading: rng.range(0.0, std::f32::consts::TAU),
            length,
            speed: rng.range(300.0, 1500.0),
            curl: rng.range(0.5, 6.0),
            pressure: rng.range(0.3, 1.0),
            attack: rng.range(0.05, 0.3),
            release: rng.range(0.05, 0.4),
            tilt: (rng.range(-0.6, 0.6), rng.range(-0.6, 0.6)),
        };
        let brush = &self.brushes[rng.below(self.brushes.len())];
        let size = self.sizes[rng.below(self.sizes.len())] * rng.range(0.7, 1.4);
        let color = self.palette[rng.below(self.palette.len())];
        let seed = rng.next_u64() as u32;
        let mut stroke = Stroke::new(id, brush, color, size, seed);
        stroke.samples = gesture.samples(self.rate_hz, rng);
        stroke
    }
}

impl Gesture {
    pub fn samples(&self, rate_hz: f32, rng: &mut Rng) -> Vec<Sample> {
        let duration = self.length / self.speed;
        let n = ((duration * rate_hz).ceil() as usize).max(2);
        let dt = 1.0 / rate_hz;
        let weights: Vec<f32> = (0..n)
            .map(|i| 0.3 + 0.7 * sinf(std::f32::consts::PI * (i as f32 + 0.5) / n as f32))
            .collect();
        let total: f32 = weights.iter().sum();
        let mut pos = self.start;
        let mut heading = self.heading;
        let mut turn = 0.0f32;
        let mut tilt = self.tilt;
        weights
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let t = i as f32 / (n - 1) as f32;
                let envelope = (t / self.attack).min(1.0) * ((1.0 - t) / self.release).min(1.0);
                let pressure = self.pressure * (0.15 + 0.85 * envelope) + rng.signed() * 0.02;
                let tremor = (rng.signed() * 0.15, rng.signed() * 0.15);
                let sample = Sample::quantize(
                    pos.0 + tremor.0,
                    pos.1 + tremor.1,
                    pressure,
                    tilt,
                    if i == 0 { 0.0 } else { dt * 1000.0 },
                );
                turn = (turn + rng.signed() * self.curl * dt) * 0.97;
                heading += turn;
                let step = self.length * w / total;
                pos = (pos.0 + cosf(heading) * step, pos.1 + sinf(heading) * step);
                tilt = (
                    (tilt.0 + rng.signed() * 0.01).clamp(-1.0, 1.0),
                    (tilt.1 + rng.signed() * 0.01).clamp(-1.0, 1.0),
                );
                sample
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_strokes() {
        let synth = Synth::canvas(1024.0, 1024.0);
        assert_eq!(synth.strokes(50, 11), synth.strokes(50, 11));
        assert_ne!(synth.strokes(50, 11), synth.strokes(50, 12));
    }

    #[test]
    fn strokes_have_tapered_pressure() {
        let strokes = Synth::canvas(1024.0, 1024.0).strokes(20, 5);
        assert!(strokes.iter().all(|s| s.samples.len() >= 2));
        assert!(strokes.iter().filter(|s| s.samples.len() >= 10).all(|s| {
            let peak = s.samples.iter().map(|v| v.pressure).max().unwrap();
            s.samples[0].pressure < peak
        }));
    }
}
