use super::{Editor, ms};
use crate::composite::{Prepared, picture_path};
use crate::messages::Tone;
use anyhow::Result;
use klad_brush::cache::{CelCache, Write};
use klad_core::rng::{FNV_OFFSET, fnv};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Instant, SystemTime};

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Buffered {
    pub watch: BTreeSet<u32>,
    pub draw: BTreeSet<u32>,
}

type Memo = BTreeMap<PathBuf, (SystemTime, u64, u64)>;
type Ask = (u64, Vec<(String, String)>, PathBuf);
type Planes = Option<Ask>;
type FrameState = (u32, u64, bool, Vec<(String, String)>, Planes, Option<(String, String)>);

pub(super) struct Survey {
    frames: Vec<FrameState>,
    pictures: PathBuf,
    root: PathBuf,
    cache: CelCache,
    holding: BTreeSet<String>,
    memo: Memo,
}

struct Keys<'a> {
    root: &'a Path,
    cache: &'a CelCache,
    memo: Memo,
    held: BTreeMap<String, bool>,
}

impl Keys<'_> {
    fn key(&mut self, layer: &str, cel: &str) -> u64 {
        let path = self.root.join("strokes").join(layer).join(format!("{cel}.strokes"));
        let stamp = std::fs::metadata(&path)
            .ok()
            .and_then(|m| Some((m.modified().ok()?, m.len())));
        match (stamp, self.memo.get(&path)) {
            (Some((at, len)), Some((was, size, key))) if at == *was && len == *size => *key,
            (stamp, _) => {
                let key = self.cache.key(&std::fs::read(&path).unwrap_or_default());
                if let Some((at, len)) = stamp {
                    self.memo.insert(path, (at, len, key));
                }
                key
            }
        }
    }

    fn active_drawn(&mut self, holding: &BTreeSet<String>, active: Option<&(String, String)>) -> bool {
        active.is_none_or(|(l, c)| self.drawn(holding, l, c))
    }

    fn planned(&mut self, planes: Option<&Ask>) -> bool {
        planes.is_some_and(|(base, cels, dir)| {
            let list: Vec<u64> = cels.iter().map(|(l, c)| self.key(l, c)).collect();
            crate::planes::path(dir, crate::planes::key(*base, &list)).exists()
        })
    }

    fn drawn(&mut self, holding: &BTreeSet<String>, layer: &str, cel: &str) -> bool {
        if holding.contains(cel) {
            return true;
        }
        if let Some(known) = self.held.get(cel) {
            return *known;
        }
        let key = self.key(layer, cel);
        let known = self.cache.holds_key(cel, key);
        self.held.insert(cel.to_string(), known);
        known
    }
}

pub(super) fn survey(s: Survey) -> (Buffered, Memo) {
    let mut keys = Keys {
        root: &s.root,
        cache: &s.cache,
        memo: s.memo,
        held: BTreeMap::new(),
    };
    let mut buffered = Buffered::default();
    s.frames
        .iter()
        .for_each(|(frame, key, in_memory, cels, planes, active)| {
            if *in_memory || picture_path(&s.pictures, *key).exists() {
                buffered.watch.insert(*frame);
            }
            let all = cels.iter().all(|(layer, cel)| keys.drawn(&s.holding, layer, cel));
            if all || (keys.active_drawn(&s.holding, active.as_ref()) && keys.planned(planes.as_ref())) {
                buffered.draw.insert(*frame);
            }
        });
    (buffered, keys.memo)
}

pub(super) struct Preparing {
    pub from: u32,
    pub to: u32,
    pub done: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    rx: Receiver<Result<(usize, usize)>>,
    since: Instant,
}

#[derive(Default)]
pub(super) struct BufferState {
    pub shown: Buffered,
    job: Option<Receiver<(Buffered, Memo)>>,
    memo: Memo,
    asked: u64,
    pub preparing: Option<Preparing>,
    pub writes: Arc<AtomicU64>,
}

impl BufferState {
    #[cfg(test)]
    pub fn job_idle(&self) -> bool {
        self.job.is_none()
    }
}

impl Editor {
    fn buffer_signature(&self) -> u64 {
        let numbers = [
            self.scene.bundle.doc.rev,
            self.frames.level as u64,
            self.scene.onion_on as u64,
            self.frames.len() as u64,
            self.buffering.writes.load(Ordering::Relaxed),
            self.scene.holding().len() as u64,
            self.scene.bundle.doc.timeline.frames as u64,
        ];
        fnv(FNV_OFFSET, &numbers.map(u64::to_le_bytes).concat())
    }

    pub(super) fn tend_buffer(&mut self) {
        let finished = self.buffering.job.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some((shown, memo)) = finished {
            self.buffering.shown = shown;
            self.buffering.memo = memo;
            self.buffering.job = None;
        }
        if let Some(result) = self.buffering.preparing.as_ref().and_then(|p| p.rx.try_recv().ok()) {
            let Some(p) = self.buffering.preparing.take() else {
                return;
            };
            match result {
                Ok((frames, drawings)) => self.say(
                    Tone::Done,
                    format!(
                        "frames {} to {} prepared in {:.1} s: {frames} pictures and {drawings} drawings made, the rest were ready",
                        p.from,
                        p.to,
                        ms(p.since) / 1000.0
                    ),
                ),
                Err(e) => self.error(format!("preparing frames: {e:#}")),
            }
        }
        if self.buffering.job.is_some() || !self.cacheable() {
            return;
        }
        let signature = self.buffer_signature();
        if signature == self.buffering.asked {
            return;
        }
        self.buffering.asked = signature;
        let onion = self.scene.onion_on;
        let count = self.scene.bundle.doc.timeline.frames;
        let frames: Vec<FrameState> = (0..count)
            .map(|f| {
                let key = self.frames.key(&mut self.scene, f, onion, &self.brushes);
                let cels = self
                    .scene
                    .bundle
                    .doc
                    .paint_content_at(f)
                    .iter()
                    .map(|(l, c)| (l.id.clone(), c.id.clone()))
                    .collect();
                let planes = self.scene.planes_ask(f).map(|p| (p.base, p.cels, p.dir));
                let active = self
                    .scene
                    .active_layer()
                    .and_then(|l| l.content_at(f).map(|c| (l.id.clone(), c.id.clone())));
                (f, key, self.frames.has(key), cels, planes, active)
            })
            .collect();
        let job = Survey {
            frames,
            pictures: self.scene.pictures(),
            root: self.scene.bundle.root.clone(),
            cache: CelCache::new(&self.scene.bundle, &self.brushes, Write::Never),
            holding: self.scene.holding(),
            memo: std::mem::take(&mut self.buffering.memo),
        };
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(survey(job));
            ctx.request_repaint();
        });
        self.buffering.job = Some(rx);
    }

    pub(super) fn prepare_span(&self) -> (u32, u32) {
        let last = self.scene.bundle.doc.timeline.frames.saturating_sub(1);
        match (self.timeline.range, self.current_clip()) {
            (Some((a, b)), _) => (a.min(last), b.min(last)),
            (None, Some(clip)) => (clip.from.min(last), clip.to.min(last)),
            _ => (0, last),
        }
    }

    pub(super) fn prepare_frames(&mut self) -> Result<()> {
        if let Some(p) = self.buffering.preparing.as_ref() {
            p.stop.store(true, Ordering::Relaxed);
            self.say(Tone::Info, "stopping after the frame being prepared");
            return Ok(());
        }
        let (from, to) = self.prepare_span();
        let (onion, level) = (self.scene.onion_on, self.frames.level);
        let mut seen = BTreeSet::new();
        let requests: Vec<_> = (from..=to)
            .map(|f| {
                let request = self.scene.prepare_request(f, onion, level, &self.brushes);
                let fresh = seen.insert(request.key);
                fresh.then_some(request)
            })
            .collect();
        let done = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let (counter, halt, writes, ctx) = (
            done.clone(),
            stop.clone(),
            self.buffering.writes.clone(),
            self.ctx.clone(),
        );
        std::thread::spawn(move || {
            let result = requests
                .into_iter()
                .try_fold((0usize, 0usize), |(frames, drawings), request| {
                    if halt.load(Ordering::Relaxed) {
                        return Ok((frames, drawings));
                    }
                    let made = match request {
                        Some(r) => crate::background().install(|| r.prepare())?,
                        None => Prepared::Already,
                    };
                    counter.fetch_add(1, Ordering::Relaxed);
                    writes.fetch_add(1, Ordering::Relaxed);
                    ctx.request_repaint();
                    Ok::<_, anyhow::Error>(match made {
                        Prepared::Already => (frames, drawings),
                        Prepared::Drawings(n) => (frames, drawings + n),
                        Prepared::Frame(n) => (frames + 1, drawings + n),
                    })
                });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.buffering.preparing = Some(Preparing {
            from,
            to,
            done,
            stop,
            rx,
            since: Instant::now(),
        });
        self.say(Tone::Info, format!("preparing frames {from} to {to} in the background"));
        Ok(())
    }

    pub(super) fn preparing_text(&self) -> Option<String> {
        self.buffering
            .preparing
            .as_ref()
            .map(|p| format!("preparing {} of {}", p.done.load(Ordering::Relaxed), p.to - p.from + 1))
    }
}
