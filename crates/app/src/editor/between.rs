use super::Editor;
use super::tweens::{self, Span};
use crate::jobs::{Spec, Tag};
use crate::messages::Tone;
use crate::theme::{self, Hint};
use anyhow::Result;
use egui::{Color32, RichText, Sense, Stroke, pos2, vec2};
use klad_core::doc::{Document, Kind, Loc};
use klad_core::stroke::{Author, Mark};
use klad_core::tween::{Spacing, made_hash};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Request {
    pub layers: Vec<String>,
    pub from: u32,
    pub to: u32,
    pub count: Option<u32>,
    pub spacing: String,
    pub matching: String,
    pub unpaired: String,
    pub pairs: Vec<[u32; 2]>,
}

pub(super) fn args(r: &Request) -> Vec<String> {
    let mut out = vec![
        "--layers".to_string(),
        r.layers.join(","),
        "--from".into(),
        r.from.to_string(),
        "--to".into(),
        r.to.to_string(),
        "--spacing".into(),
        r.spacing.clone(),
        "--match".into(),
        r.matching.clone(),
    ];
    if let Some(n) = r.count {
        out.extend(["--count".to_string(), n.to_string()]);
    }
    if !r.unpaired.is_empty() && r.unpaired != "leave" {
        out.extend(["--unpaired".to_string(), r.unpaired.clone()]);
    }
    if !r.pairs.is_empty() {
        let pairs: Vec<String> = r.pairs.iter().map(|[a, b]| format!("s{a}=s{b}")).collect();
        out.extend(["--pair".to_string(), pairs.join(",")]);
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Line {
    Layer {
        name: String,
        a: String,
        b: String,
        paired: u32,
        touching: u32,
        parts: Vec<String>,
    },
    Unpaired {
        cel: String,
        id: u32,
        first: bool,
    },
    Clash(String),
    Left {
        what: String,
        frame: Option<u32>,
    },
    Made {
        cel: String,
        frame: u32,
    },
    Kept {
        cel: String,
        count: u32,
    },
    Note(String),
}

fn layer_line(text: &str) -> Option<Line> {
    let (name, rest) = text.split_once(": ")?;
    let (a, rest) = rest.split_once(" -> ")?;
    let (b, rest) = rest.split_once(", ")?;
    let (paired, rest) = rest.split_once(" marks paired (")?;
    let (_, rest) = rest.split_once(" flood fills)")?;
    let (rest, touching) = match rest.rsplit_once(", ") {
        Some((before, last)) if last.ends_with(" line ends kept touching") => {
            (before, last.trim_end_matches(" line ends kept touching").parse().ok()?)
        }
        _ => (rest, 0),
    };
    let parts = rest
        .strip_prefix(", by part first: ")
        .map(|p| p.split(", ").map(String::from).collect())
        .unwrap_or_default();
    Some(Line::Layer {
        name: name.to_string(),
        a: a.to_string(),
        b: b.to_string(),
        paired: paired.parse().ok()?,
        touching,
        parts,
    })
}

fn unpaired_items(list: &str) -> Vec<Line> {
    list.split("), ")
        .map(|item| item.trim_end_matches(')'))
        .filter_map(|item| {
            let (mark, why) = item.split_once(" (")?;
            match why {
                "only in the first key" | "only in the second key" => {
                    let (cel, id) = klad_core::markedit::parse_address(mark)?;
                    Some(Line::Unpaired {
                        cel: cel.to_string(),
                        id,
                        first: why.contains("first"),
                    })
                }
                _ => Some(Line::Clash(format!("{mark} ({why})"))),
            }
        })
        .collect()
}

fn frame_in(text: &str) -> Option<u32> {
    let (_, rest) = text.split_once("at frame ")?;
    rest.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

pub(super) fn report(text: &str) -> Vec<Line> {
    text.lines()
        .flat_map(|raw| {
            let line = raw.trim();
            if let Some(rest) = line.strip_prefix("unpaired, ") {
                return rest
                    .split_once(": ")
                    .map(|(_, list)| unpaired_items(list))
                    .unwrap_or_default();
            }
            if let Some(list) = line.strip_prefix("fills left out: ") {
                return list
                    .split("; ")
                    .map(|what| Line::Left {
                        what: what.to_string(),
                        frame: frame_in(what),
                    })
                    .collect();
            }
            if let Some((cel, rest)) = line.split_once(" at frame ")
                && rest.contains(" (t ")
                && !cel.contains(' ')
            {
                return frame_in(line)
                    .map(|frame| Line::Made {
                        cel: cel.to_string(),
                        frame,
                    })
                    .into_iter()
                    .collect();
            }
            if let Some((cel, rest)) = line.split_once(": ")
                && rest.ends_with("drawn by hand kept on top")
            {
                let count = rest.split(' ').next().and_then(|n| n.parse().ok()).unwrap_or(0);
                return vec![Line::Kept {
                    cel: cel.to_string(),
                    count,
                }];
            }
            if let Some(layer) = layer_line(line) {
                return vec![layer];
            }
            let skip = line.is_empty()
                || ["rev ", "added ", "changed ", "removed "]
                    .iter()
                    .any(|p| line.starts_with(p));
            match skip {
                true => Vec::new(),
                false => vec![Line::Note(line.to_string())],
            }
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Pick {
    pub cel: String,
    pub id: u32,
    pub first: bool,
    pub partner_cel: String,
    pub partner_frame: u32,
}

pub(super) fn pair_from(pick: &Pick, partner: u32) -> [u32; 2] {
    match pick.first {
        true => [pick.id, partner],
        false => [partner, pick.id],
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Remake,
    Drawn(usize),
    Edited,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct RedoRow {
    pub cel: String,
    pub layer: String,
    pub frame: u32,
    pub status: Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Keep,
    Force,
    Leave,
}

pub(super) fn redo_rows(
    doc: &Document,
    span: Option<(u32, u32)>,
    layers: &[String],
    read: impl Fn(&str, &str) -> Result<Vec<Mark>>,
) -> Result<Vec<RedoRow>> {
    doc.all_layers()
        .into_iter()
        .filter(|l| l.kind() == Kind::Paint && (layers.is_empty() || layers.contains(&l.id)))
        .flat_map(|l| l.cels().iter().map(move |c| (l, c)))
        .filter(|(_, c)| c.tween.is_some() && span.is_none_or(|(a, b)| c.frame >= a && c.frame <= b))
        .map(|(l, c)| {
            let marks = read(&l.id, &c.id)?;
            let made = c.tween.as_ref().map(|t| t.made.clone()).unwrap_or_default();
            let hand = marks.iter().filter(|m| m.author() == Author::Human).count();
            let status = match (made_hash(&marks) == made, hand) {
                (true, _) => Status::Remake,
                (false, 0) => Status::Edited,
                (false, n) => Status::Drawn(n),
            };
            Ok(RedoRow {
                cel: c.id.clone(),
                layer: l.id.clone(),
                frame: c.frame,
                status,
            })
        })
        .collect()
}

pub(super) fn default_choice(status: Status) -> Choice {
    match status {
        Status::Remake => Choice::Force,
        Status::Drawn(_) => Choice::Keep,
        Status::Edited => Choice::Leave,
    }
}

pub(super) fn redo_args(rows: &[RedoRow], choices: &BTreeMap<String, Choice>) -> Option<Vec<String>> {
    let choice = |r: &RedoRow| choices.get(&r.cel).copied().unwrap_or(default_choice(r.status));
    let only: Vec<&str> = rows
        .iter()
        .filter(|r| r.status == Status::Remake || choice(r) != Choice::Leave)
        .map(|r| r.cel.as_str())
        .collect();
    if only.is_empty() {
        return None;
    }
    let listed = |want: Choice| -> Vec<&str> {
        rows.iter()
            .filter(|r| r.status != Status::Remake && choice(r) == want)
            .map(|r| r.cel.as_str())
            .collect()
    };
    let mut out = vec!["--redo".to_string(), "--only".into(), only.join(",")];
    [("--keep-cels", Choice::Keep), ("--force-cels", Choice::Force)]
        .into_iter()
        .for_each(|(flag, want)| {
            let cels = listed(want);
            if !cels.is_empty() {
                out.extend([flag.to_string(), cels.join(",")]);
            }
        });
    Some(out)
}

#[derive(Default)]
pub(super) struct Between {
    pub last: Option<Request>,
    pub report: Vec<Line>,
    pub pick: Option<Pick>,
    pub redo: Option<Vec<RedoRow>>,
    pub choices: BTreeMap<String, Choice>,
}

impl Editor {
    fn request_from(&self, span: &Span) -> Option<Request> {
        let (from, to) = tweens::keys(&self.scene.bundle.doc, span)?;
        let ask = &self.timeline.ask;
        Some(Request {
            layers: span.rows.clone(),
            from,
            to,
            count: (!ask.every_frame).then_some(ask.count),
            spacing: ask.spacing.clone(),
            matching: ask.matching.clone(),
            unpaired: ask.unpaired.clone(),
            pairs: self
                .between
                .last
                .as_ref()
                .filter(|l| (l.from, l.to) == (from, to) && l.layers == span.rows)
                .map(|l| l.pairs.clone())
                .unwrap_or_default(),
        })
    }

    pub(super) fn make_inbetweens(&mut self, request: Request) {
        let what = format!("in-betweens of frames {} and {}", request.from, request.to);
        self.between.last = Some(request.clone());
        self.between.report.clear();
        self.between.redo = None;
        self.run_inbetween_args(args(&request), what);
    }

    pub(super) fn check_redo(&mut self) -> Result<()> {
        let (span, layers) = match &self.timeline.span {
            Some(s) => (Some((s.from, s.to)), s.rows.clone()),
            None => (None, Vec::new()),
        };
        let bundle = &self.scene.bundle;
        let rows = redo_rows(&bundle.doc, span, &layers, |l, c| bundle.read_marks(l, c))?;
        self.between.choices.clear();
        self.between.redo = Some(rows);
        self.windows.between = true;
        Ok(())
    }

    pub(super) fn run_inbetween_args(&mut self, extra: Vec<String>, what: String) {
        if self.jobs.busy(Tag::Inbetween) {
            self.say(Tone::Refused, "in-betweens are already being made; wait for them");
            return;
        }
        let mut full = vec![
            "inbetween".to_string(),
            self.scene.bundle.root.to_string_lossy().to_string(),
        ];
        full.extend(extra);
        self.start_job(Spec {
            tag: Tag::Inbetween,
            name: what,
            exe: PathBuf::new(),
            args: full,
            env: Vec::new(),
            reruns: 3,
            summary: |out, _| tweens::summary(out),
            failure: |_, err| tweens::failure(err),
        });
    }

    pub(super) fn tend_pick(&mut self) -> Result<()> {
        let Some(pick) = self.between.pick.clone() else {
            return Ok(());
        };
        let Some((cel, id)) = self
            .picked
            .as_ref()
            .filter(|p| p.cel == pick.partner_cel && p.ids.len() == 1)
            .map(|p| (p.cel.clone(), p.ids[0]))
        else {
            return Ok(());
        };
        self.between.pick = None;
        self.picked = None;
        let Some(mut request) = self.between.last.clone() else {
            return Ok(());
        };
        let pair = pair_from(&pick, id);
        request.pairs.retain(|p| p[0] != pair[0] && p[1] != pair[1]);
        request.pairs.push(pair);
        self.say(
            Tone::Done,
            format!(
                "{}:s{} pairs with {cel}:s{id}; making the in-betweens again",
                pick.cel, pick.id
            ),
        );
        self.make_inbetweens(request);
        Ok(())
    }

    fn spacing_chart(ui: &mut egui::Ui, spacing: &str, count: usize, (a, b): (u32, u32)) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(320.0), 44.0), Sense::hover());
        let painter = ui.painter_at(rect);
        let (left, right, y) = (rect.left() + 14.0, rect.right() - 14.0, rect.center().y);
        let ink = Stroke::new(1.5, theme::TEXT);
        painter.line_segment([pos2(left, y), pos2(right, y)], ink);
        [(left, a), (right, b)].into_iter().for_each(|(x, f)| {
            painter.line_segment([pos2(x, y - 12.0), pos2(x, y + 12.0)], Stroke::new(2.5, theme::TEXT));
            painter.text(
                pos2(x, rect.bottom() - 2.0),
                egui::Align2::CENTER_BOTTOM,
                f.to_string(),
                egui::FontId::proportional(11.0),
                theme::WEAK,
            );
        });
        let ts = Spacing::parse(spacing).map(|s| s.ts(count)).unwrap_or_default();
        ts.iter().for_each(|t| {
            let x = left + (right - left) * t;
            painter.line_segment([pos2(x, y - 7.0), pos2(x, y + 7.0)], Stroke::new(1.5, theme::ACCENT));
        });
    }

    pub(super) fn between_window(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let span = self.timeline.span.clone();
        let request = span.as_ref().and_then(|s| self.request_from(s));
        match (&span, &request) {
            (Some(span), Some(r)) => {
                let first = span.rows.first().cloned().unwrap_or_default();
                let key_cels: Vec<Option<String>> = [r.from, r.to]
                    .iter()
                    .map(|f| {
                        self.scene
                            .bundle
                            .doc
                            .layer(&first)
                            .and_then(|l| l.content_at(*f))
                            .map(|c| c.link.clone().unwrap_or_else(|| c.id.clone()))
                    })
                    .collect();
                ui.horizontal(|ui| {
                    [r.from, r.to].iter().zip(&key_cels).for_each(|(f, cel)| {
                        ui.vertical(|ui| {
                            match cel.as_ref().and_then(|c| self.thumb(ui.ctx(), c)) {
                                Some(t) => {
                                    ui.add(egui::Image::new(&t).fit_to_exact_size(vec2(96.0, 68.0)));
                                }
                                None => {
                                    let (rect, _) = ui.allocate_exact_size(vec2(96.0, 68.0), Sense::hover());
                                    ui.painter().rect_stroke(
                                        rect,
                                        2.0,
                                        Stroke::new(1.0, theme::WEAK),
                                        egui::StrokeKind::Inside,
                                    );
                                    ui.painter().text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        format!("go to {f}\nto see it"),
                                        egui::FontId::proportional(11.0),
                                        theme::WEAK,
                                    );
                                }
                            }
                            ui.label(format!("key at {f}: {}", cel.clone().unwrap_or_default()));
                        });
                    });
                });
                let room = r.to - r.from - 1;
                let ask = &mut self.timeline.ask;
                ask.count = ask.count.clamp(1, room.max(1));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut ask.every_frame, "one on every frame")
                        .hint("an in-between on each frame between the keys");
                    if !ask.every_frame {
                        let label = ui.label("count");
                        ui.add(egui::DragValue::new(&mut ask.count).range(1..=room.max(1)))
                            .labelled_by(label.id)
                            .hint("how many in-betweens, spread over the frames between the keys");
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("spacing");
                    ["even", "in", "out", "in-out"].iter().for_each(|s| {
                        ui.selectable_value(&mut ask.spacing, s.to_string(), *s)
                            .hint("in: bunched near the first key; out: near the second");
                    });
                });
                let count = if ask.every_frame { room } else { ask.count } as usize;
                let spacing = ask.spacing.clone();
                Self::spacing_chart(ui, &spacing, count, (r.from, r.to));
                let ask = &mut self.timeline.ask;
                ui.horizontal(|ui| {
                    ui.label("pair by");
                    [("id", "id"), ("order", "order"), ("near", "nearest shape")]
                        .iter()
                        .for_each(|(key, label)| {
                            ui.selectable_value(&mut ask.matching, key.to_string(), *label)
                                .hint("how marks without a shared part find their partner");
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("one key only");
                    ["leave", "pop", "fade", "draw-on"].iter().for_each(|m| {
                        ui.selectable_value(&mut ask.unpaired, m.to_string(), *m)
                            .hint("marks in one key only: left out, popping in or out halfway, fading, or drawn on");
                    });
                });
                let parts = self.shared_parts(&first, &key_cels);
                if !parts.is_empty() {
                    ui.label(RichText::new(format!("parts pair first: {}", parts.join(", "))).color(theme::WEAK));
                }
                if !r.pairs.is_empty() {
                    let listed: Vec<String> = r.pairs.iter().map(|[a, b]| format!("s{a}=s{b}")).collect();
                    ui.label(RichText::new(format!("pairs you chose: {}", listed.join(", "))).color(theme::WEAK));
                }
                if ui
                    .add_enabled(
                        !self.jobs.busy(Tag::Inbetween),
                        egui::Button::new("make the in-betweens"),
                    )
                    .hint("runs klad inbetween with these settings; you can keep drawing meanwhile")
                    .clicked()
                {
                    self.make_inbetweens(r.clone());
                }
            }
            _ => {
                ui.label(
                    "select from one drawing to the frame where another starts, on a paint layer: drag over the cells",
                );
            }
        }
        self.report_rows(ui)?;
        ui.separator();
        self.redo_rows_ui(ui)
    }

    fn shared_parts(&self, layer: &str, cels: &[Option<String>]) -> Vec<String> {
        let parts = |cel: &Option<String>| -> BTreeMap<String, Vec<u32>> {
            match cel.as_ref().and_then(|c| self.scene.bundle.doc.locate(c)) {
                Some(Loc::Cel(path, c)) => self.scene.bundle.doc.layer_at(&path).cels()[c].parts.clone(),
                _ => BTreeMap::new(),
            }
        };
        let _ = layer;
        match cels {
            [a, b] => {
                let (pa, pb) = (parts(a), parts(b));
                pa.keys().filter(|k| pb.contains_key(*k)).cloned().collect()
            }
            _ => Vec::new(),
        }
    }

    fn report_rows(&mut self, ui: &mut egui::Ui) -> Result<()> {
        if self.between.report.is_empty() {
            return Ok(());
        }
        ui.separator();
        ui.label(RichText::new("the last run").strong());
        let mut go = None;
        let mut pick = None;
        let made: Vec<String> = self
            .between
            .report
            .iter()
            .filter_map(|l| match l {
                Line::Made { cel, frame } => Some(format!("{cel} at {frame}")),
                _ => None,
            })
            .collect();
        let last = self.between.last.clone();
        self.between.report.clone().into_iter().for_each(|line| match line {
            Line::Layer {
                name,
                a,
                b,
                paired,
                touching,
                parts,
            } => {
                let by = if parts.is_empty() {
                    String::new()
                } else {
                    format!(", by part: {}", parts.join(", "))
                };
                ui.label(format!(
                    "{name}: {a} to {b}, {paired} pairs{by}, {touching} line ends kept touching"
                ));
            }
            Line::Unpaired { cel, id, first } => {
                let side = if first { "first" } else { "second" };
                if ui
                    .button(format!("{cel}:s{id}, only in the {side} key: pick its partner"))
                    .hint(
                        "then click the stroke it should become with S; the in-betweens are made again with that pair",
                    )
                    .clicked()
                    && let Some(r) = &last
                {
                    let partner_frame = if first { r.to } else { r.from };
                    let layer = r.layers.first().cloned().unwrap_or_default();
                    let partner_cel = self
                        .scene
                        .bundle
                        .doc
                        .layer(&layer)
                        .and_then(|l| l.content_at(partner_frame))
                        .map(|c| c.link.clone().unwrap_or_else(|| c.id.clone()))
                        .unwrap_or_default();
                    pick = Some(Pick {
                        cel,
                        id,
                        first,
                        partner_cel,
                        partner_frame,
                    });
                }
            }
            Line::Clash(text) => {
                ui.label(RichText::new(format!("not paired: {text}")).color(theme::WEAK));
            }
            Line::Left { what, frame } => {
                if ui
                    .button(format!("fill left out: {what}"))
                    .hint("go to that frame")
                    .clicked()
                {
                    go = frame;
                }
            }
            Line::Kept { cel, count } => {
                ui.label(format!("{cel}: {count} hand-drawn marks kept on top"));
            }
            Line::Made { .. } => {}
            Line::Note(text) => {
                ui.label(RichText::new(text).color(theme::WEAK));
            }
        });
        if !made.is_empty() {
            ui.label(RichText::new(format!("made: {}", made.join(", "))).color(theme::WEAK));
        }
        if let Some(p) = pick {
            self.say(
                Tone::Info,
                format!(
                    "now click the partner of {}:s{} on frame {} (the stroke tool is on)",
                    p.cel, p.id, p.partner_frame
                ),
            );
            go = Some(p.partner_frame);
            self.tool = super::Tool::Strokes;
            self.picked = None;
            self.between.pick = Some(p);
        }
        if let Some(p) = self.between.pick.clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("picking the partner of {}:s{}", p.cel, p.id)).color(theme::ACCENT));
                if ui.button("stop").hint("stop picking a partner").clicked() {
                    self.between.pick = None;
                }
            });
        }
        if let Some(frame) = go {
            self.go_to(frame)?;
        }
        Ok(())
    }

    fn redo_rows_ui(&mut self, ui: &mut egui::Ui) -> Result<()> {
        if ui
            .button("check what a redo would remake (after a key changed)")
            .hint("lists the in-betweens in the selection (or all) and those drawn on since they were made")
            .clicked()
        {
            self.check_redo()?;
        }
        let Some(rows) = self.between.redo.clone() else {
            return Ok(());
        };
        if rows.is_empty() {
            ui.label("no in-betweens here");
            return Ok(());
        }
        rows.iter().for_each(|r| {
            ui.horizontal(|ui| {
                ui.label(format!("{} at {} on {}", r.cel, r.frame, r.layer));
                match r.status {
                    Status::Remake => {
                        ui.label(RichText::new("remade").color(theme::WEAK));
                    }
                    status => {
                        let what = match status {
                            Status::Drawn(n) => format!("{n} marks drawn by hand"),
                            _ => "edited since it was made".to_string(),
                        };
                        ui.label(RichText::new(what).color(Color32::from_rgb(240, 190, 90)));
                        let choice = self
                            .between
                            .choices
                            .entry(r.cel.clone())
                            .or_insert(default_choice(status));
                        if matches!(status, Status::Drawn(_)) {
                            ui.selectable_value(choice, Choice::Keep, "keep them on top").hint(
                                "remake the in-between and keep the hand-drawn marks over it, moved with their line",
                            );
                        }
                        ui.selectable_value(choice, Choice::Force, "in-between them too")
                            .hint("remake it from the keys; marks drawn on it are replaced");
                        ui.selectable_value(choice, Choice::Leave, "leave it")
                            .hint("this in-between stays as it is");
                    }
                }
            });
        });
        let args = redo_args(&rows, &self.between.choices);
        if ui
            .add_enabled(
                args.is_some() && !self.jobs.busy(Tag::Inbetween),
                egui::Button::new("redo them"),
            )
            .hint("one history step for all of them")
            .clicked()
            && let Some(args) = args
        {
            self.between.redo = None;
            self.between.report.clear();
            self.run_inbetween_args(args, "redoing in-betweens".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUN: &str = "lines: lines0 -> lines2, 15 marks paired (0 flood fills), by part first: body, near leg, 16 line ends kept touching
colour: col0 -> col6, 2 marks paired (2 flood fills)
  unpaired, left out: b6:s3 (only in the second key), a0:s5 (only in the first key), a0:s2 and b6:s4 (one is a stroke or lasso fill, the other not)
  fills left out: col0:s2 at frame 4: its seed lands on a line; col0:s1 at frame 5: it leaked (40 px where the keys have 10 and 12)
  c11 at frame 1 (t 0.17) on lines
  c12 at frame 2 (t 0.33) on lines
  c14: 1 mark drawn by hand kept on top
2 in-betweens on 2 frames in 120 ms (60 ms per frame)
rev 5";

    #[test]
    fn the_panel_builds_the_cli_arguments() {
        let r = Request {
            layers: vec!["lines".into(), "colour".into()],
            from: 0,
            to: 6,
            count: Some(2),
            spacing: "in-out".into(),
            matching: "near".into(),
            unpaired: "fade".into(),
            pairs: vec![[3, 7], [5, 9]],
        };
        assert_eq!(
            args(&r).join(" "),
            "--layers lines,colour --from 0 --to 6 --spacing in-out --match near --count 2 --unpaired fade --pair s3=s7,s5=s9"
        );
        let plain = Request {
            count: None,
            unpaired: "leave".into(),
            pairs: Vec::new(),
            ..r
        };
        assert!(
            !args(&plain)
                .iter()
                .any(|a| a == "--count" || a == "--unpaired" || a == "--pair")
        );
    }

    #[test]
    fn the_cli_report_becomes_rows_and_two_clicks_make_a_pair() {
        let rows = report(RUN);
        assert_eq!(
            rows[0],
            Line::Layer {
                name: "lines".into(),
                a: "lines0".into(),
                b: "lines2".into(),
                paired: 15,
                touching: 16,
                parts: vec!["body".into(), "near leg".into()],
            }
        );
        assert!(matches!(&rows[1], Line::Layer { touching: 0, parts, .. } if parts.is_empty()));
        assert_eq!(
            rows[2],
            Line::Unpaired {
                cel: "b6".into(),
                id: 3,
                first: false
            }
        );
        assert_eq!(
            rows[3],
            Line::Unpaired {
                cel: "a0".into(),
                id: 5,
                first: true
            }
        );
        assert!(matches!(&rows[4], Line::Clash(t) if t.contains("a0:s2 and b6:s4")));
        assert!(matches!(&rows[5], Line::Left { frame: Some(4), .. }));
        assert!(matches!(&rows[6], Line::Left { frame: Some(5), what } if what.contains("leaked")));
        assert_eq!(
            rows[7],
            Line::Made {
                cel: "c11".into(),
                frame: 1
            }
        );
        assert_eq!(
            rows[9],
            Line::Kept {
                cel: "c14".into(),
                count: 1
            }
        );
        assert!(matches!(&rows[10], Line::Note(t) if t.starts_with("2 in-betweens")));
        assert_eq!(rows.len(), 11);
        let second = Pick {
            cel: "b6".into(),
            id: 3,
            first: false,
            partner_cel: "a0".into(),
            partner_frame: 0,
        };
        assert_eq!(pair_from(&second, 8), [8, 3]);
        let first = Pick { first: true, ..second };
        assert_eq!(pair_from(&first, 8), [3, 8]);
    }

    #[test]
    fn a_redo_lists_what_it_remakes_and_takes_a_choice_per_drawn_in_between() {
        let row = |cel: &str, status| RedoRow {
            cel: cel.into(),
            layer: "lines".into(),
            frame: 1,
            status,
        };
        let rows = vec![
            row("c11", Status::Remake),
            row("c12", Status::Drawn(2)),
            row("c13", Status::Edited),
            row("c14", Status::Drawn(1)),
        ];
        let mut choices = BTreeMap::new();
        assert_eq!(
            redo_args(&rows, &choices).unwrap().join(" "),
            "--redo --only c11,c12,c14 --keep-cels c12,c14"
        );
        choices.insert("c13".to_string(), Choice::Force);
        choices.insert("c14".to_string(), Choice::Leave);
        assert_eq!(
            redo_args(&rows, &choices).unwrap().join(" "),
            "--redo --only c11,c12,c13 --keep-cels c12 --force-cels c13"
        );
        let left = vec![row("c13", Status::Edited)];
        assert_eq!(redo_args(&left, &BTreeMap::new()), None);
    }
}
