use klad_core::rng::hash2_unit;

pub const GRAIN_SIZE: usize = 512;

#[derive(Clone, Debug)]
pub struct Grain {
    width: usize,
    height: usize,
    data: Vec<f32>,
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn value_noise(x: usize, y: usize, period: usize, seed: u32) -> f32 {
    let cell = GRAIN_SIZE / period;
    let (gx, gy) = (x / cell, y / cell);
    let (fx, fy) = (
        smooth((x % cell) as f32 / cell as f32),
        smooth((y % cell) as f32 / cell as f32),
    );
    let corner = |i: usize, j: usize| hash2_unit((i % period) as i32, (j % period) as i32, seed);
    let top = corner(gx, gy) + (corner(gx + 1, gy) - corner(gx, gy)) * fx;
    let bottom = corner(gx, gy + 1) + (corner(gx + 1, gy + 1) - corner(gx, gy + 1)) * fx;
    top + (bottom - top) * fy
}

impl Grain {
    pub fn paper(seed: u32, feature: usize) -> Self {
        let octaves: Vec<(usize, f32)> = [8usize, 4, 2, 1]
            .iter()
            .zip([0.5f32, 0.25, 0.15, 0.1])
            .map(|(m, a)| (GRAIN_SIZE / (feature * m).clamp(1, GRAIN_SIZE), a))
            .collect();
        let raw: Vec<f32> = (0..GRAIN_SIZE * GRAIN_SIZE)
            .map(|i| {
                let (x, y) = (i % GRAIN_SIZE, i / GRAIN_SIZE);
                octaves
                    .iter()
                    .enumerate()
                    .map(|(k, &(period, amp))| value_noise(x, y, period, seed + k as u32) * amp)
                    .sum()
            })
            .collect();
        let (lo, hi) = raw
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        Self {
            width: GRAIN_SIZE,
            height: GRAIN_SIZE,
            data: raw.iter().map(|v| (v - lo) / (hi - lo)).collect(),
        }
    }

    pub fn from_picture(picture: &crate::images::Picture) -> Self {
        Self {
            width: picture.width.max(1),
            height: picture.height.max(1),
            data: picture.data.clone(),
        }
    }

    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.data[(y % self.height) * self.width + (x % self.width)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grain_is_normalised_and_tiles() {
        let grain = Grain::paper(5, 4);
        assert!(grain.data.iter().all(|v| (0.0..=1.0).contains(v)));
        assert_eq!(grain.at(3, 7), grain.at(3 + GRAIN_SIZE, 7 + GRAIN_SIZE));
        let edge = (grain.at(GRAIN_SIZE - 1, 10) - grain.at(0, 10)).abs();
        assert!(edge < 0.2);
    }
}
