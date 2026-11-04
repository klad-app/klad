use crate::stats::lock;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const CHANGED: i32 = 75;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    Inbetween,
    Export,
    Edit,
    Gaps,
    Arcs,
}

#[derive(Clone)]
pub struct Spec {
    pub tag: Tag,
    pub name: String,
    pub exe: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub reruns: u32,
    pub summary: fn(&str, &str) -> String,
    pub failure: fn(&str, &str) -> String,
}

struct Ended {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

struct Running {
    id: u64,
    spec: Spec,
    started: Instant,
    child: Arc<Mutex<Child>>,
    cancelled: Arc<AtomicBool>,
    rx: Receiver<std::io::Result<Ended>>,
    reruns: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Done(String),
    Failed(String),
    Cancelled,
    Rerun,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub tag: Tag,
    pub name: String,
    pub outcome: Outcome,
    pub took: Duration,
    pub output: String,
}

#[derive(Default)]
pub struct Jobs {
    running: Vec<Running>,
    next: u64,
}

type Launched = (Arc<Mutex<Child>>, Receiver<std::io::Result<Ended>>);

fn launch(spec: &Spec) -> std::io::Result<Launched> {
    let mut child = Command::new(&spec.exe)
        .args(&spec.args)
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let child = Arc::new(Mutex::new(child));
    let waiter = child.clone();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let errors = std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(e) = err.as_mut() {
                let _ = e.read_to_string(&mut text);
            }
            text
        });
        let mut stdout = String::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_string(&mut stdout);
        }
        let stderr = errors.join().unwrap_or_default();
        let status = lock(&waiter).wait();
        let _ = tx.send(status.map(|s| Ended {
            code: s.code(),
            stdout,
            stderr,
        }));
    });
    Ok((child, rx))
}

impl Jobs {
    pub fn start(&mut self, spec: Spec) -> std::io::Result<u64> {
        let (child, rx) = launch(&spec)?;
        self.next += 1;
        self.running.push(Running {
            id: self.next,
            spec,
            started: Instant::now(),
            child,
            cancelled: Arc::new(AtomicBool::new(false)),
            rx,
            reruns: 0,
        });
        Ok(self.next)
    }

    pub fn cancel(&mut self, id: u64) {
        self.running.iter().filter(|r| r.id == id).for_each(|r| {
            r.cancelled.store(true, Ordering::Relaxed);
            let _ = lock(&r.child).kill();
        });
    }

    pub fn busy(&self, tag: Tag) -> bool {
        self.running.iter().any(|r| r.spec.tag == tag)
    }

    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }

    pub fn running(&self, now: Instant) -> Vec<(u64, String, Duration)> {
        self.running
            .iter()
            .map(|r| (r.id, r.spec.name.clone(), now.duration_since(r.started)))
            .collect()
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let mut keep = Vec::new();
        std::mem::take(&mut self.running).into_iter().for_each(|mut r| {
            let result = match r.rx.try_recv() {
                Err(TryRecvError::Empty) => {
                    keep.push(r);
                    return;
                }
                Err(TryRecvError::Disconnected) => Err("the job stopped without a result".to_string()),
                Ok(Err(e)) => Err(format!("cannot run {}: {e}", r.spec.exe.display())),
                Ok(Ok(ended)) => Ok(ended),
            };
            let took = r.started.elapsed();
            let output = match &result {
                Ok(e) => format!("{}{}", e.stdout, e.stderr),
                Err(_) => String::new(),
            };
            let event = |outcome| Event {
                tag: r.spec.tag,
                name: r.spec.name.clone(),
                outcome,
                took,
                output: output.clone(),
            };
            let outcome = match result {
                _ if r.cancelled.load(Ordering::Relaxed) => Outcome::Cancelled,
                Err(text) => Outcome::Failed(text),
                Ok(e) if e.code == Some(0) => Outcome::Done((r.spec.summary)(&e.stdout, &e.stderr)),
                Ok(e) if e.code == Some(CHANGED) && r.reruns < r.spec.reruns => match launch(&r.spec) {
                    Ok((child, rx)) => {
                        events.push(event(Outcome::Rerun));
                        r.child = child;
                        r.rx = rx;
                        r.reruns += 1;
                        keep.push(r);
                        return;
                    }
                    Err(e) => Outcome::Failed(format!("cannot run it again: {e}")),
                },
                Ok(e) => Outcome::Failed((r.spec.failure)(&e.stdout, &e.stderr)),
            };
            events.push(event(outcome));
        });
        self.running = keep;
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::Scene;
    use klad_brush::BrushSet;
    use klad_core::bundle::Bundle;
    use klad_core::doc::{Document, Layer};
    use klad_core::ops::parse_lines;
    use std::path::Path;

    fn shell(name: &str, script: &str, reruns: u32) -> Spec {
        Spec {
            tag: Tag::Export,
            name: name.into(),
            exe: PathBuf::from("sh"),
            args: vec!["-c".into(), script.into()],
            env: Vec::new(),
            reruns,
            summary: |out, _| out.trim().to_string(),
            failure: |_, err| err.trim().to_string(),
        }
    }

    fn until_done(jobs: &mut Jobs, seconds: u64) -> Vec<Event> {
        let start = Instant::now();
        let mut seen = Vec::new();
        while !jobs.is_empty() && start.elapsed() < Duration::from_secs(seconds) {
            seen.extend(jobs.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        seen
    }

    #[test]
    fn jobs_report_done_failed_cancelled_and_run_again_when_the_document_changed() {
        let dir = std::env::temp_dir().join(format!("draw-jobs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("seen");
        let mut jobs = Jobs::default();
        jobs.start(shell("ok", "echo made 3; echo noise >&2", 0)).unwrap();
        jobs.start(shell("bad", "echo broke >&2; exit 3", 0)).unwrap();
        let slow = jobs.start(shell("slow", "sleep 5", 0)).unwrap();
        let again = format!(
            "if [ -f {0} ]; then echo second; else touch {0}; exit 75; fi",
            marker.display()
        );
        jobs.start(shell("again", &again, 2)).unwrap();
        assert!(jobs.busy(Tag::Export) && !jobs.busy(Tag::Inbetween));
        assert_eq!(jobs.running(Instant::now()).len(), 4);
        jobs.cancel(slow);
        let events = until_done(&mut jobs, 10);
        let of = |name: &str| {
            events
                .iter()
                .filter(|e| e.name == name)
                .map(|e| e.outcome.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(of("ok"), [Outcome::Done("made 3".into())]);
        assert_eq!(of("bad"), [Outcome::Failed("broke".into())]);
        assert_eq!(of("slow"), [Outcome::Cancelled]);
        assert_eq!(of("again"), [Outcome::Rerun, Outcome::Done("second".into())]);
        jobs.start(shell("stubborn", "exit 75", 1)).unwrap();
        let events = until_done(&mut jobs, 10);
        assert_eq!(events.len(), 2);
        assert!(matches!(events[1].outcome, Outcome::Failed(_)));
        assert!(jobs.start(shell("x", "true", 0)).is_ok());
        let mut missing = shell("missing", "", 0);
        missing.exe = dir.join("no-such-binary");
        assert!(jobs.start(missing).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn draw_binary() -> PathBuf {
        let exe = std::env::current_exe().unwrap();
        let bin = exe.parent().and_then(Path::parent).unwrap().join("klad");
        assert!(
            bin.exists(),
            "{} is missing: cargo test --workspace builds it (the klad-cli binary test needs it)",
            bin.display()
        );
        bin
    }

    #[test]
    fn an_edit_made_while_an_in_between_job_runs_survives_it() {
        let dir = std::env::temp_dir().join(format!("draw-jobs-tween-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(256, 256);
        doc.timeline.frames = 5;
        doc.layers.push(Layer::paint("l1", "lines", "c1"));
        doc.layers.push(Layer::paint("l2", "notes", "c2"));
        doc.next_id = 10;
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        bundle
            .apply(
                &parse_lines(
                    r#"{"op":"set","id":"c2","hold":1}
{"op":"stroke","layer":"l1","path":"M40 40 L140 60","size":6}
{"op":"cel","dup":"c1","frame":4}"#,
                )
                .unwrap(),
            )
            .map(|a| {
                let key = a.added[0].clone();
                let id = bundle.read_marks("l1", &key).unwrap()[0].id();
                let moved = format!(r#"{{"op":"transform","ids":["{key}:s{id}"],"x":50,"y":80}}"#);
                bundle.apply(&parse_lines(&moved).unwrap()).unwrap();
            })
            .unwrap();
        let mut jobs = Jobs::default();
        let marker = dir.join(".cache/write-delay");
        jobs.start(Spec {
            tag: Tag::Inbetween,
            name: "in-betweens".into(),
            exe: draw_binary(),
            args: [
                "inbetween",
                &dir.to_string_lossy(),
                "--layers",
                "l1",
                "--from",
                "0",
                "--to",
                "4",
            ]
            .map(String::from)
            .to_vec(),
            env: vec![("DRAW_TEST_WRITE_DELAY_MS".into(), "700".into())],
            reruns: 2,
            summary: |out, _| out.trim().to_string(),
            failure: |_, err| err.trim().to_string(),
        })
        .unwrap();
        let start = Instant::now();
        let mut early = Vec::new();
        while !marker.exists() && !jobs.is_empty() && start.elapsed() < Duration::from_secs(30) {
            early.extend(jobs.poll());
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(marker.exists(), "the CLI never reached its write: {early:?}");
        std::fs::remove_file(&marker).unwrap();
        let mut scene = Scene::open(&dir).unwrap();
        let brushes = BrushSet::for_bundle(&scene.bundle).unwrap();
        scene
            .apply(
                &parse_lines(r#"{"op":"stroke","layer":"l2","path":"M10 10 L60 60","frame":3}"#).unwrap(),
                &brushes,
            )
            .unwrap();
        let events = until_done(&mut jobs, 60);
        let outcomes: Vec<&Outcome> = events.iter().map(|e| &e.outcome).collect();
        assert_eq!(outcomes.first(), Some(&&Outcome::Rerun), "{outcomes:?}");
        assert!(matches!(outcomes.last(), Some(Outcome::Done(_))), "{outcomes:?}");
        let after = Bundle::open(&dir).unwrap();
        let notes = after.doc.layer("l2").unwrap();
        let painted = notes
            .cels()
            .iter()
            .find(|c| c.frame == 3)
            .expect("the editor's new drawing at frame 3");
        assert_eq!(after.read_marks("l2", &painted.id).unwrap().len(), 1);
        let tweens = after
            .doc
            .layer("l1")
            .unwrap()
            .cels()
            .iter()
            .filter(|c| c.tween.is_some())
            .count();
        assert_eq!(tweens, 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
