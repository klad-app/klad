pub mod claude;
pub mod secret;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

pub const DEFAULT_MODEL: &str = "claude-opus-5-5";

const NO_BUNDLE: [&str; 3] = ["fonts", "licences", "skill"];
const REFUSED: [&str; 10] = [
    "open", "install", "watch", "bench", "demo", "stress", "scribble", "new", "apply", "help",
];
const READ_ONLY: [&str; 16] = [
    "ls",
    "show",
    "frames",
    "bones",
    "keys",
    "brushes",
    "notes",
    "selection",
    "strokes",
    "hash",
    "fonts",
    "arcs",
    "compare",
    "log",
    "skill",
    "palette",
];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Usage {
    pub fn add(&mut self, other: Usage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
    }

    pub fn cost(&self, model: &str) -> f64 {
        let (input, output) = price(model);
        (self.input as f64 * input
            + self.output as f64 * output
            + self.cache_read as f64 * input * 0.05
            + self.cache_write as f64 * input * 1.25)
            / 1_000_000.0
    }
}

pub fn price(model: &str) -> (f64, f64) {
    match model {
        m if m.starts_with("claude-fable") || m.starts_with("claude-mythos") => (10.0, 50.0),
        m if m.starts_with("claude-opus-5-5") => (4.0, 20.0),
        m if m.starts_with("claude-opus") => (5.0, 25.0),
        m if m.starts_with("claude-sonnet-4") => (3.0, 15.0),
        m if m.starts_with("claude-sonnet") => (2.0, 10.0),
        m if m.starts_with("claude-haiku-5") => (0.1, 0.5),
        m if m.starts_with("claude-haiku") => (1.0, 5.0),
        _ => (5.0, 25.0),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Delta {
    Text(String),
    Thinking(String),
}

pub struct Request {
    pub model: String,
    pub system: String,
    pub tools: Vec<Value>,
    pub messages: Vec<Value>,
}

pub struct Reply {
    pub content: Vec<Value>,
    pub stop: String,
    pub usage: Usage,
    pub invalid: Vec<String>,
}

pub trait Provider: Send {
    fn send(&mut self, request: &Request, delta: &mut dyn FnMut(Delta)) -> Result<Reply>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Text(String),
    Thinking(String),
    Tool {
        name: String,
        input: Value,
    },
    Done {
        name: String,
        text: String,
        image: Option<Vec<u8>>,
        failed: bool,
        edited: bool,
    },
    Usage(Usage),
    Finished {
        stop: String,
    },
    Failed(String),
}

pub struct Drawing {
    pub bundle: PathBuf,
    pub binary: PathBuf,
    pub editable: bool,
}

impl Drawing {
    fn folder(&self) -> PathBuf {
        self.bundle
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    }

    fn exports(&self) -> PathBuf {
        self.folder().join("exports")
    }
}

fn normal(path: &Path) -> PathBuf {
    path.components().fold(PathBuf::new(), |mut out, c| {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
        out
    })
}

fn looks_like_path(arg: &str) -> bool {
    arg.contains('/')
        || arg.contains('\\')
        || arg.starts_with('~')
        || [
            ".png", ".jpg", ".jpeg", ".json", ".tres", ".tscn", ".ttf", ".otf", ".klad", ".svg",
        ]
        .iter()
        .any(|e| arg.to_ascii_lowercase().ends_with(e))
}

pub fn allowed(drawing: &Drawing, arg: &str) -> Result<()> {
    if !looks_like_path(arg) || arg.trim_start().starts_with(['M', 'm']) && arg.contains(' ') {
        return Ok(());
    }
    if arg.starts_with('~') {
        bail!("{arg}: paths stay inside the drawing or its exports folder");
    }
    let resolved = normal(&drawing.folder().join(arg));
    let roots = [normal(&drawing.bundle), normal(&drawing.exports())];
    match roots.iter().any(|r| resolved.starts_with(r)) {
        true => Ok(()),
        false => bail!(
            "{arg}: paths stay inside the drawing ({}) or the exports folder next to it",
            drawing.bundle.display()
        ),
    }
}

pub fn tools(editable: bool) -> Vec<Value> {
    let mut list = vec![
        json!({
            "name": "look",
            "description": "Render the open drawing and see it. Use this after every change you make, and zoom in on details with region and scale.",
            "eager_input_streaming": true,
            "input_schema": {"type": "object", "properties": {
                "frame": {"type": "integer", "description": "Frame to render (default 0)"},
                "region": {"type": "string", "description": "x,y,w,h in canvas px to render only that part"},
                "scale": {"type": "number", "description": "Render scale; default fits the longest side in 1024 px"},
                "overlay": {"type": "string", "description": "ids, bbox, grid, parts, bones or weights:<bone>"}
            }}
        }),
        json!({
            "name": "read",
            "description": "Read what the user selected or pinned, the layer tree, or a layer's strokes, as text.",
            "eager_input_streaming": true,
            "input_schema": {"type": "object", "properties": {
                "what": {"type": "string", "enum": ["selection", "notes", "ls", "strokes", "frames", "bones", "keys"]},
                "layer": {"type": "string", "description": "For strokes: the layer id or name"}
            }, "required": ["what"]}
        }),
    ];
    if editable {
        list.push(json!({
            "name": "klad",
            "description": "Run a klad command on the open drawing, as from a terminal. args starts with the subcommand; the drawing's path is put after it for you. Example: [\"stroke\", \"l4\", \"--brush\", \"ink\", \"--size\", \"8\", \"--path\", \"M100 100 L 200 140\"]. Files you write go into the drawing or the exports folder next to it.",
            "eager_input_streaming": true,
            "input_schema": {"type": "object", "properties": {
                "args": {"type": "array", "items": {"type": "string"}}
            }, "required": ["args"]}
        }));
        list.push(json!({
            "name": "apply",
            "description": "Apply many edits at once: JSON op lines, as klad apply takes them. One transaction.",
            "eager_input_streaming": true,
            "input_schema": {"type": "object", "properties": {
                "ops": {"type": "string", "description": "One JSON op per line"}
            }, "required": ["ops"]}
        }));
    }
    list
}

pub fn system(drawing: &Drawing) -> String {
    format!(
        "You are working inside the klad editor, on the drawing the user has open: {}. The user watches every change land on their canvas and can undo your whole turn in one step. Use look to see the drawing before and after you change it, read for what the user selected or pinned, klad and apply to change it. Keep to what the user asked; say briefly what you did.\n\n{}",
        drawing.bundle.display(),
        include_str!("../../../../docs/skill/SKILL.md")
    )
}

struct Ran {
    text: String,
    image: Option<Vec<u8>>,
    failed: bool,
    edited: bool,
}

fn run(drawing: &Drawing, args: &[String], stdin: Option<&str>) -> Result<(bool, String)> {
    let mut child = Command::new(&drawing.binary)
        .args(args)
        .current_dir(drawing.folder())
        .env("KLAD_AUTHOR", "agent")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting {}", drawing.binary.display()))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    let mut text = text.trim().to_string();
    if text.len() > 12_000 {
        text.truncate(12_000);
        text.push_str("\n... (cut; ask for less, for example one layer or a region)");
    }
    Ok((out.status.success(), text))
}

fn bundle_arg(drawing: &Drawing) -> String {
    drawing.bundle.to_string_lossy().to_string()
}

fn execute(drawing: &Drawing, name: &str, input: &Value) -> Result<Ran> {
    let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_string);
    match name {
        "look" => {
            let out = std::env::temp_dir().join(format!("klad-agent-look-{}.png", std::process::id()));
            let mut args = vec![
                "render".to_string(),
                bundle_arg(drawing),
                "--out".into(),
                out.to_string_lossy().to_string(),
            ];
            if let Some(f) = input.get("frame").and_then(Value::as_u64) {
                args.extend(["--frame".into(), f.to_string()]);
            }
            if let Some(r) = s("region") {
                args.extend(["--region".into(), r]);
            }
            let scale = input.get("scale").and_then(Value::as_f64).or_else(|| {
                let doc = std::fs::read_to_string(drawing.bundle.join("doc.json")).ok()?;
                let v: Value = serde_json::from_str(&doc).ok()?;
                let longest = v["canvas"]["width"].as_f64()?.max(v["canvas"]["height"].as_f64()?);
                (s("region").is_none()).then(|| (1024.0 / longest).min(1.0))
            });
            if let Some(sc) = scale {
                args.extend(["--scale".into(), format!("{sc:.3}")]);
            }
            if let Some(o) = s("overlay") {
                args.extend(["--overlay".into(), o]);
            }
            let (ok, text) = run(drawing, &args, None)?;
            let image = ok.then(|| std::fs::read(&out).ok()).flatten();
            let _ = std::fs::remove_file(&out);
            Ok(Ran {
                text,
                image,
                failed: !ok,
                edited: false,
            })
        }
        "read" => {
            let what = s("what").unwrap_or_default();
            let mut args = match what.as_str() {
                "selection" | "notes" | "ls" | "frames" | "bones" | "keys" => vec![what.clone(), bundle_arg(drawing)],
                "strokes" => vec![
                    "strokes".into(),
                    bundle_arg(drawing),
                    s("layer").context("strokes needs a layer")?,
                ],
                other => bail!("read: no {other:?}; selection, notes, ls, strokes, frames, bones or keys"),
            };
            if what == "strokes" {
                args.push("--json".into());
            }
            let (ok, text) = run(drawing, &args, None)?;
            Ok(Ran {
                text,
                image: None,
                failed: !ok,
                edited: false,
            })
        }
        "klad" if drawing.editable => {
            let args: Vec<String> = input
                .get("args")
                .and_then(Value::as_array)
                .context("klad needs args: a list of strings")?
                .iter()
                .map(|a| {
                    a.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| anyhow!("every arg is a string"))
                })
                .collect::<Result<_>>()?;
            let sub = args.first().cloned().context("klad needs a subcommand")?;
            if REFUSED.contains(&sub.as_str()) {
                bail!("klad {sub} is not available here; for many edits at once use the apply tool");
            }
            args.iter().try_for_each(|a| allowed(drawing, a))?;
            let mut full = vec![sub.clone()];
            if !NO_BUNDLE.contains(&sub.as_str()) {
                full.push(bundle_arg(drawing));
            }
            full.extend(args[1..].iter().cloned());
            let (ok, text) = run(drawing, &full, None)?;
            Ok(Ran {
                text,
                image: None,
                failed: !ok,
                edited: ok && !READ_ONLY.contains(&sub.as_str()),
            })
        }
        "apply" if drawing.editable => {
            let ops = s("ops").context("apply needs ops: JSON lines")?;
            let (ok, text) = run(drawing, &["apply".into(), bundle_arg(drawing)], Some(&ops))?;
            Ok(Ran {
                text,
                image: None,
                failed: !ok,
                edited: ok,
            })
        }
        "klad" | "apply" => bail!("the user has not let the agent edit this drawing; it can look and suggest only"),
        other => bail!("no tool {other}"),
    }
}

pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    bytes
        .chunks(3)
        .flat_map(|c| {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            let out = [18, 12, 6, 0].map(|s| TABLE[(n >> s & 63) as usize] as char);
            let keep = c.len() + 1;
            (0..4).map(move |i| if i < keep { out[i] } else { '=' })
        })
        .collect()
}

pub struct Limits {
    pub spend: f64,
    pub spent: f64,
}

pub fn turn(
    provider: &mut dyn Provider,
    drawing: &Drawing,
    model: &str,
    history: &mut Vec<Value>,
    limits: Limits,
    stop: &Arc<AtomicBool>,
    events: &Sender<Event>,
) -> Result<usize> {
    let request_tools = tools(drawing.editable);
    let system = system(drawing);
    let mut spent = limits.spent;
    let mut edits = 0;
    loop {
        if limits.spend > 0.0 && spent >= limits.spend {
            let _ = events.send(Event::Finished {
                stop: format!("the spend limit of ${:.2} for this session is reached", limits.spend),
            });
            return Ok(edits);
        }
        let request = Request {
            model: model.to_string(),
            system: system.clone(),
            tools: request_tools.clone(),
            messages: history.clone(),
        };
        let reply = provider.send(&request, &mut |d| {
            let _ = events.send(match d {
                Delta::Text(t) => Event::Text(t),
                Delta::Thinking(t) => Event::Thinking(t),
            });
        })?;
        spent += reply.usage.cost(model);
        let _ = events.send(Event::Usage(reply.usage));
        history.push(json!({"role": "assistant", "content": reply.content}));
        if reply.stop != "tool_use" {
            let _ = events.send(Event::Finished { stop: reply.stop });
            return Ok(edits);
        }
        let calls: Vec<(String, String, Value)> = reply
            .content
            .iter()
            .filter(|b| b["type"] == "tool_use")
            .map(|b| {
                (
                    b["id"].as_str().unwrap_or_default().to_string(),
                    b["name"].as_str().unwrap_or_default().to_string(),
                    b["input"].clone(),
                )
            })
            .collect();
        let mut results = Vec::new();
        for (id, name, input) in calls {
            if stop.load(Ordering::Relaxed) {
                results.push(json!({"type": "tool_result", "tool_use_id": id, "content": "stopped by the user", "is_error": true}));
                continue;
            }
            let _ = events.send(Event::Tool {
                name: name.clone(),
                input: input.clone(),
            });
            let ran = match reply.invalid.contains(&id) {
                true => Err(anyhow!("the tool input was not valid JSON (cut off?); send it again")),
                false => execute(drawing, &name, &input),
            };
            let (text, image, failed, edited) = match ran {
                Ok(r) => (r.text, r.image, r.failed, r.edited),
                Err(e) => (format!("{e:#}"), None, true, false),
            };
            edits += edited as usize;
            let _ = events.send(Event::Done {
                name: name.clone(),
                text: text.clone(),
                image: image.clone(),
                failed,
                edited,
            });
            let mut content =
                vec![json!({"type": "text", "text": if text.is_empty() { "done".to_string() } else { text }})];
            if let Some(png) = &image {
                content.insert(0, json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": base64(png)}}));
            }
            results.push(json!({"type": "tool_result", "tool_use_id": id, "content": content, "is_error": failed}));
        }
        history.push(json!({"role": "user", "content": results}));
        if stop.load(Ordering::Relaxed) {
            let _ = events.send(Event::Finished { stop: "stopped".into() });
            return Ok(edits);
        }
    }
}

pub struct Scripted {
    pub replies: Vec<Vec<Value>>,
}

impl Provider for Scripted {
    fn send(&mut self, _request: &Request, delta: &mut dyn FnMut(Delta)) -> Result<Reply> {
        if self.replies.is_empty() {
            bail!("the script has no more replies");
        }
        let content = self.replies.remove(0);
        content
            .iter()
            .filter_map(|b| b["text"].as_str())
            .for_each(|t| delta(Delta::Text(t.to_string())));
        let stop = match content.iter().any(|b| b["type"] == "tool_use") {
            true => "tool_use",
            false => "end_turn",
        };
        Ok(Reply {
            content,
            stop: stop.into(),
            usage: Usage {
                input: 1000,
                output: 100,
                ..Usage::default()
            },
            invalid: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::bundle::Bundle;
    use klad_core::doc::{Document, Layer};
    use std::sync::mpsc::channel;

    fn binary() -> PathBuf {
        let exe = std::env::current_exe().unwrap();
        exe.parent().and_then(Path::parent).unwrap().join("klad")
    }

    fn drawing(name: &str, editable: bool) -> Drawing {
        let dir = std::env::temp_dir().join(format!("klad-agent-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut doc = Document::new(320, 200);
        doc.canvas.background = Some("#ffffff".into());
        doc.layers = vec![Layer::paint("l1", "paint", "c1")];
        Bundle::create(dir.join("art.klad"), doc).unwrap();
        Drawing {
            bundle: dir.join("art.klad"),
            binary: binary(),
            editable,
        }
    }

    fn call(id: &str, name: &str, input: Value) -> Value {
        json!({"type": "tool_use", "id": id, "name": name, "input": input})
    }

    #[test]
    fn a_scripted_turn_draws_looks_and_undoes_in_its_steps() {
        let d = drawing("turn", true);
        let mut script = Scripted {
            replies: vec![
                vec![
                    json!({"type": "text", "text": "Drawing a line."}),
                    call(
                        "t1",
                        "apply",
                        json!({"ops": r##"{"op":"stroke","layer":"l1","brush":"ink","size":10,"color":"#202020","path":"M40 100 L 280 100"}"##}),
                    ),
                ],
                vec![
                    call("t2", "look", json!({})),
                    call("t3", "klad", json!({"args": ["export", "-o", "/etc/stolen.png"]})),
                ],
                vec![json!({"type": "text", "text": "Done: one line across the middle."})],
            ],
        };
        let (tx, rx) = channel();
        let mut history = vec![json!({"role": "user", "content": "draw a line"})];
        let stop = Arc::new(AtomicBool::new(false));
        let edits = turn(
            &mut script,
            &d,
            DEFAULT_MODEL,
            &mut history,
            Limits { spend: 0.0, spent: 0.0 },
            &stop,
            &tx,
        )
        .unwrap();
        assert_eq!(edits, 1);
        let events: Vec<Event> = rx.try_iter().collect();
        let looked = events.iter().find_map(|e| match e {
            Event::Done {
                name, image: Some(png), ..
            } if name == "look" => Some(png.clone()),
            _ => None,
        });
        assert!(
            looked.is_some_and(|p| p.starts_with(b"\x89PNG")),
            "look returns the render as a picture"
        );
        let refused = events.iter().find_map(|e| match e {
            Event::Done {
                name,
                text,
                failed: true,
                ..
            } if name == "klad" => Some(text.clone()),
            _ => None,
        });
        assert!(
            refused.is_some_and(|t| t.contains("paths stay inside the drawing")),
            "{events:?}"
        );
        assert!(matches!(events.last(), Some(Event::Finished { stop }) if stop == "end_turn"));
        assert_eq!(
            history.len(),
            6,
            "user, assistant, results, assistant, results, assistant"
        );
        let bundle = Bundle::open(&d.bundle).unwrap();
        assert_eq!(bundle.read_marks("l1", "c1").unwrap().len(), 1);
        let (ok, _) = run(&d, &["undo".into(), bundle_arg(&d)], None).unwrap();
        assert!(ok);
        assert!(
            Bundle::open(&d.bundle)
                .unwrap()
                .read_marks("l1", "c1")
                .unwrap()
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(d.bundle.parent().unwrap());
    }

    #[test]
    fn a_drawing_the_agent_may_not_edit_offers_only_looking_and_the_spend_limit_stops_it() {
        let d = drawing("locked", false);
        assert_eq!(
            tools(false)
                .iter()
                .map(|t| t["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["look", "read"]
        );
        let mut script = Scripted {
            replies: vec![
                vec![call("t1", "apply", json!({"ops": "{}"}))],
                vec![json!({"type": "text", "text": "ok"})],
            ],
        };
        let (tx, rx) = channel();
        let mut history = vec![json!({"role": "user", "content": "go"})];
        let stop = Arc::new(AtomicBool::new(false));
        turn(
            &mut script,
            &d,
            DEFAULT_MODEL,
            &mut history,
            Limits { spend: 0.0, spent: 0.0 },
            &stop,
            &tx,
        )
        .unwrap();
        let events: Vec<Event> = rx.try_iter().collect();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Done { failed: true, text, .. } if text.contains("look and suggest only")))
        );
        let mut more = Scripted {
            replies: vec![vec![json!({"type": "text", "text": "never"})]],
        };
        let (tx, rx) = channel();
        turn(
            &mut more,
            &d,
            DEFAULT_MODEL,
            &mut history,
            Limits { spend: 1.0, spent: 1.5 },
            &stop,
            &tx,
        )
        .unwrap();
        assert!(
            rx.try_iter()
                .any(|e| matches!(e, Event::Finished { stop } if stop.contains("spend limit")))
        );
        assert_eq!(more.replies.len(), 1, "no request is made past the limit");
        let _ = std::fs::remove_dir_all(d.bundle.parent().unwrap());
    }

    #[test]
    fn paths_stay_in_the_drawing_and_base64_is_standard() {
        let d = Drawing {
            bundle: PathBuf::from("/w/art.klad"),
            binary: PathBuf::new(),
            editable: true,
        };
        assert!(allowed(&d, "art.klad/brushes/x.json").is_ok());
        assert!(allowed(&d, "exports/hero.png").is_ok());
        assert!(allowed(&d, "../other.klad/doc.json").is_err());
        assert!(allowed(&d, "/etc/passwd").is_err());
        assert!(allowed(&d, "exports/../../x.png").is_err());
        assert!(allowed(&d, "M10 20 L 30 40").is_ok());
        assert_eq!(base64(b"klad"), "a2xhZA==");
        assert_eq!(base64(b"kla"), "a2xh");
        let u = Usage {
            input: 1_000_000,
            output: 100_000,
            cache_read: 0,
            cache_write: 0,
        };
        assert!((u.cost("claude-opus-5-5") - 6.0).abs() < 1e-9);
    }
}
