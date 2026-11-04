use crate::composite::{FrameDone, FrameImage, Scene};
use anyhow::Result;
use klad_brush::BrushSet;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

pub const BUDGET: usize = 640 << 20;
pub const HEAVY: usize = 512 << 20;

pub type Notify = Arc<dyn Fn() + Send + Sync>;

struct Entry {
    bytes: usize,
    used: u64,
}

struct Job {
    key: u64,
    frame: u32,
    rx: Receiver<Result<FrameDone>>,
}

pub struct FrameCache {
    pub budget: usize,
    pub level: u32,
    pub max_jobs: usize,
    pub heavy: bool,
    pub disk: Option<std::path::PathBuf>,
    entries: BTreeMap<u64, Entry>,
    uploads: Vec<FrameImage>,
    drops: Vec<u64>,
    jobs: Vec<Job>,
    keys: BTreeMap<(u32, bool), u64>,
    tick: u64,
    notify: Notify,
    pub error: Option<String>,
}

impl FrameCache {
    pub fn new(notify: Notify) -> Self {
        Self {
            budget: BUDGET,
            level: 0,
            max_jobs: 2,
            heavy: false,
            disk: None,
            entries: BTreeMap::new(),
            uploads: Vec::new(),
            drops: Vec::new(),
            jobs: Vec::new(),
            keys: BTreeMap::new(),
            tick: 0,
            notify,
            error: None,
        }
    }

    pub fn changed(&mut self) {
        self.keys.clear();
    }

    pub fn set_level(&mut self, level: u32) {
        if level != self.level {
            self.level = level;
            self.keys.clear();
        }
    }

    pub fn key(&mut self, scene: &mut Scene, frame: u32, onion: bool, brushes: &BrushSet) -> u64 {
        if let Some(key) = self.keys.get(&(frame, onion)) {
            return *key;
        }
        let key = scene.frame_key(frame, onion, self.level, brushes.fingerprint());
        self.keys.insert((frame, onion), key);
        key
    }

    pub fn on_disk(&self, key: u64) -> bool {
        self.disk
            .as_deref()
            .is_some_and(|dir| crate::composite::picture_path(dir, key).exists())
    }

    pub fn has(&self, key: u64) -> bool {
        self.entries.contains_key(&key)
    }

    pub fn touch(&mut self, key: u64) {
        self.tick += 1;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.used = self.tick;
        }
    }

    pub fn pending(&self, key: u64) -> bool {
        self.jobs.iter().any(|j| j.key == key)
    }

    pub fn jobs(&self) -> usize {
        self.jobs.len()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.entries.values().map(|e| e.bytes).sum()
    }

    pub fn request(&mut self, scene: &mut Scene, frame: u32, onion: bool, brushes: &BrushSet, urgent: bool) -> bool {
        let key = self.key(scene, frame, onion, brushes);
        if self.has(key) || self.pending(key) {
            return true;
        }
        let limit = match self.heavy {
            true => 1,
            false => self.max_jobs + usize::from(urgent),
        };
        if self.jobs.len() >= limit {
            return false;
        }
        let request = scene.frame_request(frame, onion, self.level, brushes);
        let (tx, rx) = channel();
        let notify = self.notify.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::background().install(|| request.run()));
            notify();
        });
        self.jobs.push(Job { key, frame, rx });
        true
    }

    pub fn collect(&mut self, scene: &mut Scene) -> Vec<u32> {
        let mut done = Vec::new();
        let jobs = std::mem::take(&mut self.jobs);
        self.jobs = jobs
            .into_iter()
            .filter_map(|job| match job.rx.try_recv() {
                Ok(Ok(result)) => {
                    self.heavy |= result.loaded_bytes() > HEAVY;
                    let image = scene.absorb(result);
                    self.tick += 1;
                    self.entries.insert(
                        image.key,
                        Entry {
                            bytes: image.bytes(),
                            used: self.tick,
                        },
                    );
                    self.uploads.push(image);
                    done.push(job.frame);
                    None
                }
                Ok(Err(e)) => {
                    self.error = Some(format!("frame {}: {e:#}", job.frame));
                    None
                }
                Err(TryRecvError::Empty) => Some(job),
                Err(TryRecvError::Disconnected) => {
                    self.error = Some(format!("the job for frame {} stopped", job.frame));
                    None
                }
            })
            .collect();
        done
    }

    fn remove(&mut self, key: u64) {
        self.entries.remove(&key);
        self.uploads.retain(|u| u.key != key);
        self.drops.push(key);
    }

    pub fn prune(&mut self, keep: &BTreeSet<u64>) {
        let mut total = self.bytes();
        if total <= self.budget {
            return;
        }
        let mut order: Vec<(u64, u64, usize)> = self
            .entries
            .iter()
            .filter(|(k, _)| !keep.contains(*k))
            .map(|(k, e)| (e.used, *k, e.bytes))
            .collect();
        order.sort();
        order.into_iter().for_each(|(_, key, bytes)| {
            if total > self.budget {
                self.remove(key);
                total -= bytes;
            }
        });
    }

    pub fn forget_unused(&mut self, live: &BTreeSet<u64>) -> usize {
        let gone: Vec<u64> = self.entries.keys().filter(|k| !live.contains(*k)).copied().collect();
        gone.iter().for_each(|k| self.remove(*k));
        gone.len()
    }

    pub fn take(&mut self) -> (Vec<FrameImage>, Vec<u64>) {
        (std::mem::take(&mut self.uploads), std::mem::take(&mut self.drops))
    }
}

pub struct Player {
    start: Instant,
    first: u64,
    pub from: u32,
    pub to: u32,
    pub fps: u32,
    pub once: bool,
    shown: Option<u64>,
    pub drops: Vec<u32>,
}

impl Player {
    pub fn new(now: Instant, frame: u32, (from, to): (u32, u32), fps: u32, once: bool) -> Self {
        let first = match (from..=to).contains(&frame) && !(once && frame == to && from < to) {
            true => (frame - from) as u64,
            false => 0,
        };
        Self {
            start: now,
            first,
            from,
            to,
            fps: fps.max(1),
            once,
            shown: None,
            drops: Vec::new(),
        }
    }

    fn len(&self) -> u64 {
        (self.to - self.from + 1) as u64
    }

    pub fn index_at(&self, now: Instant) -> u64 {
        let elapsed = now.saturating_duration_since(self.start).as_nanos();
        self.first + (elapsed * self.fps as u128 / 1_000_000_000) as u64
    }

    fn due(&self, now: Instant) -> u64 {
        match self.once {
            true => self.index_at(now).min(self.len() - 1),
            false => self.index_at(now),
        }
    }

    pub fn finished(&self, now: Instant) -> bool {
        let at = self.index_at(now);
        self.once && at >= self.len() && (self.shown == Some(self.len() - 1) || at >= self.len() + self.fps as u64)
    }

    pub fn frame_of(&self, index: u64) -> u32 {
        match self.once {
            true => self.from + index.min(self.len() - 1) as u32,
            false => self.from + (index % self.len()) as u32,
        }
    }

    pub fn shown(&self) -> Option<u32> {
        self.shown.map(|i| self.frame_of(i))
    }

    fn loop_of(&self, index: u64) -> usize {
        ((index - self.first) / self.len()) as usize
    }

    pub fn tick(&mut self, now: Instant, mut ready: impl FnMut(u32) -> bool) -> Option<u32> {
        let due = self.due(now);
        if self.shown == Some(due) || !ready(self.frame_of(due)) {
            return None;
        }
        let next = self.shown.map_or(self.first, |s| s + 1);
        let loops = self.loop_of(due) + 1;
        if self.drops.len() < loops {
            self.drops.resize(loops, 0);
        }
        (next..due).for_each(|i| {
            let at = self.loop_of(i);
            self.drops[at] += 1;
        });
        self.shown = Some(due);
        Some(self.frame_of(due))
    }

    pub fn ahead(&self, now: Instant, count: usize) -> Vec<u32> {
        let due = self.due(now);
        let mut seen = BTreeSet::new();
        (due..due + count as u64)
            .map(|i| self.frame_of(i))
            .filter(|f| seen.insert(*f))
            .collect()
    }

    pub fn next_in(&self, now: Instant) -> Duration {
        let next = self.index_at(now) + 1 - self.first;
        let at = self.start + Duration::from_nanos((next as u128 * 1_000_000_000 / self.fps as u128) as u64);
        at.saturating_duration_since(now)
    }

    pub fn summary(&self) -> String {
        let total: u32 = self.drops.iter().sum();
        let recent: Vec<String> = self
            .drops
            .iter()
            .enumerate()
            .rev()
            .take(3)
            .rev()
            .map(|(i, d)| format!("loop {}: {d}", i + 1))
            .collect();
        format!("dropped {total} ({})", recent.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_skips_rather_than_drifts_and_counts_drops_per_loop() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut p = Player::new(t0, 2, (0, 3), 10, false);
        assert_eq!(p.tick(at(0), |_| true), Some(2));
        assert_eq!(p.tick(at(50), |_| true), None);
        assert_eq!(p.tick(at(100), |_| true), Some(3));
        assert_eq!(p.tick(at(199), |_| true), None);
        assert_eq!(p.tick(at(200), |_| true), Some(0));
        assert_eq!(p.drops, [0]);
        assert_eq!(p.tick(at(300), |f| f != 1), None);
        assert_eq!(p.tick(at(410), |_| true), Some(2));
        assert_eq!(p.drops, [1, 0]);
        assert_eq!(p.tick(at(1000), |_| true), Some(0));
        assert_eq!(p.drops, [1, 3, 2]);
        assert_eq!(p.shown(), Some(0));
        assert_eq!(p.next_in(at(1000)), Duration::from_millis(100));
        assert_eq!(p.ahead(at(1000), 6), [0, 1, 2, 3]);
        assert!(p.summary().starts_with("dropped 6"));
        let outside = Player::new(t0, 9, (4, 7), 12, false);
        assert_eq!(outside.frame_of(outside.index_at(t0)), 4);
    }

    #[test]
    fn a_clip_that_plays_once_stops_on_its_last_frame() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut p = Player::new(t0, 5, (4, 7), 10, true);
        assert_eq!(p.tick(at(0), |_| true), Some(5));
        assert_eq!(p.tick(at(100), |_| true), Some(6));
        assert_eq!(p.tick(at(200), |_| true), Some(7));
        assert!(!p.finished(at(250)));
        assert_eq!(p.tick(at(260), |_| true), None);
        assert_eq!(p.ahead(at(260), 4), [7]);
        assert!(p.finished(at(300)));
        assert_eq!((p.shown(), p.drops.clone()), (Some(7), vec![0]));
        let again = Player::new(t0, 7, (4, 7), 10, true);
        assert_eq!(
            again.frame_of(again.index_at(t0)),
            4,
            "from its last frame it starts over"
        );
        let late = Player::new(t0, 4, (4, 7), 10, true);
        assert!(
            !late.finished(at(500)) && late.finished(at(1400)),
            "a last frame that never came is not waited on"
        );
    }
}
