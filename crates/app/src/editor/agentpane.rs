use super::Editor;
use crate::agent::{self, Drawing, Event, Limits, Provider, Usage};
use crate::messages::Tone;
use crate::theme::{self, Hint};
use anyhow::Result;
use egui::RichText;
use klad_core::history::Stack;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

pub(super) enum Entry {
    User(String),
    Text(String),
    Thinking(String),
    Tool {
        name: String,
        input: Value,
        text: String,
        image: Option<egui::TextureHandle>,
        failed: bool,
        done: bool,
    },
    Note(String),
}

struct Running {
    events: Receiver<Event>,
    done: Receiver<(Vec<Value>, Result<usize, String>)>,
    stop: Arc<AtomicBool>,
}

#[derive(Default)]
pub(super) struct AgentPane {
    pub input: String,
    pub entries: Vec<Entry>,
    running: Option<Running>,
    history: Vec<Value>,
    pub session: Usage,
    pub turn: Usage,
    pub last_edits: usize,
    pub key: String,
    pub models: Vec<String>,
    pub script: Option<Vec<Vec<Value>>>,
}

fn summary(name: &str, input: &Value) -> String {
    match name {
        "klad" => input["args"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "))
            .unwrap_or_default(),
        "apply" => format!(
            "{} ops",
            input["ops"]
                .as_str()
                .map_or(0, |o| o.lines().filter(|l| !l.trim().is_empty()).count())
        ),
        "look" => ["frame", "region", "scale", "overlay"]
            .iter()
            .filter_map(|k| {
                input
                    .get(*k)
                    .map(|v| format!("{k} {}", v.to_string().trim_matches('"')))
            })
            .collect::<Vec<_>>()
            .join(", "),
        "read" => input["what"].as_str().unwrap_or_default().to_string(),
        _ => String::new(),
    }
}

fn tokens(n: u64) -> String {
    match n {
        n if n >= 10_000 => format!("{:.0}k", n as f64 / 1000.0),
        n if n >= 1000 => format!("{:.1}k", n as f64 / 1000.0),
        n => n.to_string(),
    }
}

impl Editor {
    pub(super) fn agent_running(&self) -> bool {
        self.agent.running.is_some()
    }

    fn editable(&self) -> bool {
        let path = self.scene.bundle.root.display().to_string();
        !self.settings.agent.locked.contains(&path)
    }

    pub(super) fn ask_agent(&mut self, text: String) -> Result<()> {
        if self.agent_running() || text.trim().is_empty() {
            return Ok(());
        }
        let provider: Box<dyn Provider> = match self.agent.script.take() {
            Some(replies) => Box::new(agent::Scripted { replies }),
            None => {
                let key = agent::secret::load()
                    .ok_or_else(|| anyhow::anyhow!("add a Claude API key first: the settings window, under agent"))?;
                Box::new(agent::claude::Claude { key })
            }
        };
        let binary =
            crate::start::draw_binary().ok_or_else(|| anyhow::anyhow!("the klad command is not next to the editor"))?;
        let drawing = Drawing {
            bundle: self.scene.bundle.root.clone(),
            binary,
            editable: self.editable(),
        };
        let model = self.settings.agent.model.clone();
        let limits = Limits {
            spend: self.settings.agent.limit as f64,
            spent: self.agent.session.cost(&model),
        };
        let mut history = std::mem::take(&mut self.agent.history);
        self.agent.entries.push(Entry::User(text.clone()));
        self.agent.turn = Usage::default();
        let (tx, events) = channel();
        let (finished, done) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let halt = stop.clone();
        let ctx = self.ctx.clone();
        std::thread::Builder::new().name("klad agent".into()).spawn(move || {
            let mut provider = provider;
            let read = |what: &str| {
                std::process::Command::new(&drawing.binary)
                    .args([what, &drawing.bundle.to_string_lossy()])
                    .output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default()
            };
            let (selection, notes) = (read("selection"), read("notes"));
            let mut message = text;
            [
                ("What the user has selected", selection),
                ("Notes pinned on the canvas", notes),
            ]
            .into_iter()
            .filter(|(_, v)| !v.is_empty() && !v.starts_with("nothing") && !v.starts_with("no "))
            .for_each(|(k, v)| message.push_str(&format!("\n\n{k}:\n{v}")));
            history.push(json!({"role": "user", "content": message}));
            let result = agent::turn(provider.as_mut(), &drawing, &model, &mut history, limits, &halt, &tx)
                .map_err(|e| format!("{e:#}"));
            if let Err(e) = &result {
                let _ = tx.send(Event::Failed(e.clone()));
                if history.last().is_some_and(|m| m["role"] == "user") {
                    history.pop();
                }
            }
            let _ = finished.send((history, result));
            ctx.request_repaint();
        })?;
        self.agent.running = Some(Running { events, done, stop });
        Ok(())
    }

    pub(super) fn poll_agent(&mut self) {
        let Some(running) = self.agent.running.as_ref() else {
            return;
        };
        let events: Vec<Event> = running.events.try_iter().collect();
        let finished = running.done.try_recv().ok();
        events.into_iter().for_each(|e| match e {
            Event::Text(t) => match self.agent.entries.last_mut() {
                Some(Entry::Text(s)) => s.push_str(&t),
                _ => self.agent.entries.push(Entry::Text(t)),
            },
            Event::Thinking(t) => match self.agent.entries.last_mut() {
                Some(Entry::Thinking(s)) => s.push_str(&t),
                _ => self.agent.entries.push(Entry::Thinking(t)),
            },
            Event::Tool { name, input } => self.agent.entries.push(Entry::Tool {
                name,
                input,
                text: String::new(),
                image: None,
                failed: false,
                done: false,
            }),
            Event::Done {
                text: t,
                image: png,
                failed: f,
                ..
            } => {
                let texture = png.as_deref().and_then(|bytes| {
                    let rgba = klad_render::assets::decode(bytes).ok()?;
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [rgba.width as usize, rgba.height as usize],
                        &rgba.pixels,
                    );
                    Some(self.ctx.load_texture("agent look", image, egui::TextureOptions::LINEAR))
                });
                if let Some(Entry::Tool {
                    text,
                    image,
                    failed,
                    done,
                    ..
                }) = self
                    .agent
                    .entries
                    .iter_mut()
                    .rev()
                    .find(|e| matches!(e, Entry::Tool { done: false, .. }))
                {
                    *text = t;
                    *image = texture;
                    *failed = f;
                    *done = true;
                }
            }
            Event::Usage(u) => {
                self.agent.turn.add(u);
                self.agent.session.add(u);
            }
            Event::Finished { stop } if stop != "end_turn" && stop != "tool_use" => {
                self.agent.entries.push(Entry::Note(match stop.as_str() {
                    "max_tokens" => "the reply hit its length limit".into(),
                    "refusal" => "the model declined this request".into(),
                    other => other.to_string(),
                }))
            }
            Event::Finished { .. } => {}
            Event::Failed(e) => self.agent.entries.push(Entry::Note(format!("stopped: {e}"))),
        });
        if let Some((history, result)) = finished {
            self.agent.history = history;
            self.agent.last_edits = result.as_ref().map_or(0, |n| *n);
            self.agent.running = None;
            if let Err(e) = result {
                self.say(Tone::Error, format!("the agent: {e}"));
            }
            self.check(|ed| ed.reload());
        }
    }

    pub(super) fn undo_agent_turn(&mut self) -> Result<()> {
        let n = std::mem::take(&mut self.agent.last_edits);
        (0..n).try_for_each(|_| self.step(Stack::Undo))?;
        self.agent
            .entries
            .push(Entry::Note(format!("undid the agent's last turn ({n} edits)")));
        Ok(())
    }

    pub(super) fn agent_pane(&mut self, ui: &mut egui::Ui) -> Result<()> {
        self.poll_agent();
        if !self.settings.agent.notice_seen {
            ui.group(|ui| {
                ui.label(
                    "The agent sends your messages, what you selected, your notes for it, renders of this drawing and the commands it runs to the model's provider (Anthropic, with your own API key). It changes this drawing only while it may edit it, and you can undo a whole turn.",
                );
                if ui.button("understood").hint("shown once").clicked() {
                    self.settings.agent.notice_seen = true;
                    self.settings_dirty = true;
                }
            });
            return Ok(());
        }
        let mut send = false;
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 110.0)
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.agent.entries.is_empty() {
                    ui.label(
                        RichText::new("Ask for a change in plain words: \"paint the rock as a round form lit from the left\", \"ink everything on the sketch layer\". Select strokes or pin notes first to point at things.")
                            .color(theme::WEAK),
                    );
                }
                self.agent.entries.iter().for_each(|e| match e {
                    Entry::User(t) => {
                        ui.label(RichText::new(t).strong());
                    }
                    Entry::Text(t) => {
                        ui.label(t);
                    }
                    Entry::Thinking(t) if !t.trim().is_empty() => {
                        ui.label(RichText::new(t).italics().small().color(theme::WEAK));
                    }
                    Entry::Thinking(_) => {}
                    Entry::Tool { name, input, text, image, failed, done } => {
                        let mark = match (done, failed) {
                            (false, _) => "...",
                            (true, true) => "failed",
                            (true, false) => "",
                        };
                        ui.collapsing(RichText::new(format!("{name} {} {mark}", summary(name, input))).small(), |ui| {
                            if let Some(t) = image {
                                let size = t.size_vec2();
                                let k = (ui.available_width() / size.x.max(1.0)).min(1.0);
                                ui.image((t.id(), size * k));
                            }
                            ui.label(RichText::new(text).small().monospace());
                        });
                    }
                    Entry::Note(t) => {
                        ui.label(RichText::new(t).small().color(theme::REFUSED));
                    }
                });
            });
        ui.separator();
        let r = ui.add(
            egui::TextEdit::multiline(&mut self.agent.input)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
                .hint_text("what should the agent do?"),
        );
        if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command) {
            send = true;
        }
        ui.horizontal(|ui| {
            match self.agent_running() {
                true => {
                    if ui.button("stop").hint("ends the turn at the next tool call").clicked()
                        && let Some(r) = &self.agent.running
                    {
                        r.stop.store(true, Ordering::Relaxed);
                    }
                }
                false => {
                    if ui
                        .button("send")
                        .hint("cmd Return; your selection and notes go with it")
                        .clicked()
                    {
                        send = true;
                    }
                }
            }
            let undo = self.agent.last_edits > 0 && !self.agent_running();
            if ui
                .add_enabled(undo, egui::Button::new("undo this turn"))
                .hint("every change the agent's last turn made, in one go")
                .clicked()
            {
                self.check(|e| e.undo_agent_turn());
            }
            let mut editable = self.editable();
            if ui
                .checkbox(&mut editable, "may edit")
                .hint("off: the agent can look and suggest, not change this drawing")
                .changed()
            {
                let path = self.scene.bundle.root.display().to_string();
                self.settings.agent.locked.retain(|p| p != &path);
                if !editable {
                    self.settings.agent.locked.push(path);
                }
                self.settings_dirty = true;
            }
        });
        let model = self.settings.agent.model.clone();
        ui.label(
            RichText::new(format!(
                "{model}: this turn {} in, {} out, ${:.2}; session ${:.2} of ${:.2}",
                tokens(self.agent.turn.input + self.agent.turn.cache_read + self.agent.turn.cache_write),
                tokens(self.agent.turn.output),
                self.agent.turn.cost(&model),
                self.agent.session.cost(&model),
                self.settings.agent.limit
            ))
            .small()
            .color(theme::WEAK),
        );
        if send {
            let text = std::mem::take(&mut self.agent.input);
            self.ask_agent(text)?;
        }
        if self.agent_running() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        Ok(())
    }

    pub(super) fn agent_settings(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("agent").strong());
        let has_key = agent::secret::load().is_some();
        ui.horizontal(|ui| {
            let label = ui.label(if has_key {
                "Claude API key (saved)"
            } else {
                "Claude API key"
            });
            ui.add(
                egui::TextEdit::singleline(&mut self.agent.key)
                    .password(true)
                    .desired_width(180.0),
            )
            .labelled_by(label.id)
            .hint("a key from the Claude Console; kept in the Keychain on a Mac, never in settings or the drawing");
            if ui.button("save").hint("keep the key and check it").clicked() {
                let key = std::mem::take(&mut self.agent.key);
                match agent::secret::store(&key).and_then(|_| agent::claude::models(&key)) {
                    Ok(models) => {
                        self.say(Tone::Done, format!("key saved; {} models", models.len()));
                        self.agent.models = models;
                    }
                    Err(e) => self.say(Tone::Error, format!("the key: {e:#}")),
                }
            }
            if has_key && ui.button("forget").hint("remove the key").clicked() {
                let _ = agent::secret::forget();
                self.say(Tone::Done, "the key is removed");
            }
        });
        ui.horizontal(|ui| {
            let models = match self.agent.models.is_empty() {
                true => vec![
                    agent::DEFAULT_MODEL.to_string(),
                    "claude-sonnet-5-5".into(),
                    "claude-haiku-5-5".into(),
                ],
                false => self.agent.models.clone(),
            };
            egui::ComboBox::from_id_salt("agent model")
                .selected_text(&self.settings.agent.model)
                .show_ui(ui, |ui| {
                    models.iter().for_each(|m| {
                        if ui.selectable_label(self.settings.agent.model == *m, m).clicked() {
                            self.settings.agent.model = m.clone();
                            self.settings_dirty = true;
                        }
                    });
                })
                .response
                .hint("the model the agent uses");
            let mut limit = self.settings.agent.limit;
            if ui
                .add(
                    egui::DragValue::new(&mut limit)
                        .range(0.0..=100.0)
                        .speed(0.1)
                        .prefix("spend limit $"),
                )
                .hint("the agent stops before the next call once a session has cost this much; 0 has no limit")
                .changed()
            {
                self.settings.agent.limit = limit;
                self.settings_dirty = true;
            }
        });
    }
}
