use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};

const WINDOW: usize = 240;

#[derive(Default)]
pub struct Rolling {
    values: VecDeque<f32>,
}

impl Rolling {
    pub fn push(&mut self, value: f32) {
        if self.values.len() == WINDOW {
            self.values.pop_front();
        }
        self.values.push_back(value);
    }

    pub fn avg(&self) -> f32 {
        self.values.iter().sum::<f32>() / self.values.len().max(1) as f32
    }

    pub fn max(&self) -> f32 {
        self.values.iter().copied().fold(0.0, f32::max)
    }

    pub fn percentile(&self, p: f32) -> f32 {
        let mut sorted: Vec<f32> = self.values.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        sorted
            .get(((sorted.len() as f32 - 1.0) * p).round().max(0.0) as usize)
            .copied()
            .unwrap_or(0.0)
    }

    pub fn spread(&self) -> String {
        if self.values.is_empty() {
            "-".to_string()
        } else {
            format!(
                "median {:.2}  p99 {:.2}  max {:.1}",
                self.percentile(0.5),
                self.percentile(0.99),
                self.max()
            )
        }
    }

    pub fn summary(&self) -> String {
        if self.values.is_empty() {
            "-".to_string()
        } else {
            format!("avg {:.1}  max {:.1}", self.avg(), self.max())
        }
    }
}

#[derive(Default)]
pub struct Stats {
    pub frame_ms: Rolling,
    pub latency_ms: Rolling,
    pub compose_ms: Rolling,
    pub tiles: Rolling,
    pub upload_ms: Rolling,
    pub step_ms: Rolling,
    pub frame_upload_ms: Rolling,
    pub ui_ms: Rolling,
}

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_keeps_a_window() {
        let mut r = Rolling::default();
        (0..WINDOW + 10).for_each(|i| r.push(i as f32));
        assert_eq!(r.max(), (WINDOW + 9) as f32);
        assert_eq!(r.values.len(), WINDOW);
        assert!(r.avg() > 10.0);
        let mut q = Rolling::default();
        [5.0, 1.0, 3.0, 2.0, 4.0].iter().for_each(|v| q.push(*v));
        assert_eq!(
            (q.percentile(0.5), q.percentile(0.99), q.percentile(0.0)),
            (3.0, 5.0, 1.0)
        );
    }
}
