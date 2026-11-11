use super::{Editor, LassoAction, MarkTransform};
use crate::jobs::{Spec, Tag};
use crate::messages::Tone;
use crate::theme::{self, Hint};
use anyhow::{Result, anyhow, bail};
use egui::{Color32, Pos2, Rect, Stroke, Vec2, vec2};
use klad_core::markedit::bounds_of;
use klad_core::ops::Op;
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Checks {
    pub gaps: Option<Gaps>,
    pub pixels: Option<PixelCheck>,
    pub arcs: Option<Arcs>,
    pub parts: bool,
    pub part: String,
    pub font: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PixelCheck {
    pub layer: String,
    pub frame: u32,
    pub rev: u64,
    pub found: Vec<klad_core::pixelcheck::Finding>,
}

pub(super) fn tally(found: &[klad_core::pixelcheck::Finding]) -> String {
    use klad_core::pixelcheck::Kind;
    let parts: Vec<String> = Kind::ALL
        .iter()
        .filter_map(|kind| {
            let count = found.iter().filter(|f| f.kind == *kind).count();
            (count > 0).then(|| format!("{count} {}", kind.name()))
        })
        .collect();
    match parts.is_empty() {
        true => "nothing to look at on this layer".to_string(),
        false => format!("to look at, marked on the canvas: {}", parts.join(", ")),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Gaps {
    pub layer: String,
    pub frames: Vec<(u32, Vec<[f32; 4]>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Arcs {
    pub track: String,
    pub lines: Vec<Vec<(u32, [f32; 2])>>,
}

pub(super) fn json_of(output: &str) -> Option<Value> {
    let start = output.find('{')?;
    serde_json::Deserializer::from_str(&output[start..])
        .into_iter::<Value>()
        .next()?
        .ok()
}

fn numbers(value: &Value) -> Vec<f32> {
    value
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_f64().map(|v| v as f32)).collect())
        .unwrap_or_default()
}

pub(super) fn gaps_from(output: &str, layer: &str) -> Option<Gaps> {
    let found = json_of(output)?;
    let frames = found["frames"]
        .as_array()?
        .iter()
        .map(|f| {
            let pockets = f["pockets"]
                .as_array()
                .map(|all| {
                    all.iter()
                        .map(|p| numbers(&p["bounds"]))
                        .filter(|b| b.len() == 4)
                        .map(|b| [b[0], b[1], b[2], b[3]])
                        .collect()
                })
                .unwrap_or_default();
            (f["frame"].as_u64().unwrap_or(0) as u32, pockets)
        })
        .collect();
    Some(Gaps {
        layer: layer.to_string(),
        frames,
    })
}

pub(super) fn arcs_from(output: &str, track: &str) -> Option<Arcs> {
    let found = json_of(output)?;
    let lines = found["tracks"]
        .as_array()?
        .iter()
        .map(|t| {
            t["frames"]
                .as_array()
                .map(|all| {
                    all.iter()
                        .filter_map(|f| {
                            let at = numbers(&f["at"]);
                            (at.len() == 2).then(|| (f["frame"].as_u64().unwrap_or(0) as u32, [at[0], at[1]]))
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    Some(Arcs {
        track: track.to_string(),
        lines,
    })
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

pub(super) fn refill_args(bundle: &str, layer: &str, frame: u32) -> Vec<String> {
    strings(&["refill", bundle, layer, "--frame", &frame.to_string()])
}

pub(super) fn fill_like_args(bundle: &str, layer: &str, cel: &str, (from, to): (u32, u32)) -> Vec<String> {
    strings(&[
        "fill",
        bundle,
        layer,
        "--like",
        cel,
        "--frames",
        &format!("{from}-{to}"),
    ])
}

pub(super) fn gaps_args(bundle: &str, layer: &str, frames: (u32, u32)) -> Vec<String> {
    let mut args = strings(&["gaps", bundle, layer, "--json"]);
    match frames {
        (from, to) if from == to => args.extend(strings(&["--frame", &from.to_string()])),
        (from, to) => args.extend(strings(&["--sheet", &format!("{from}-{to}")])),
    }
    args
}

pub(super) fn arcs_args(bundle: &str, track: &str, (from, to): (u32, u32)) -> Vec<String> {
    strings(&[
        "arcs",
        bundle,
        "--track",
        track,
        "--from",
        &from.to_string(),
        "--to",
        &to.to_string(),
        "--json",
    ])
}

pub(super) fn font_args(bundle: &str, file: &str) -> Vec<String> {
    strings(&["font", bundle, file])
}

fn last_line(out: &str, err: &str) -> String {
    out.lines()
        .chain(err.lines())
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn quiet(_: &str, _: &str) -> String {
    String::new()
}

impl Editor {
    fn bundle_arg(&self) -> String {
        self.scene.bundle.root.to_string_lossy().to_string()
    }

    fn run_job(&mut self, tag: Tag, name: String, args: Vec<String>, summary: fn(&str, &str) -> String) {
        self.start_job(Spec {
            tag,
            name,
            exe: PathBuf::new(),
            args,
            env: Vec::new(),
            reruns: 3,
            summary,
            failure: |_, err| super::tweens::failure(err),
        });
    }

    fn active_paint_id(&self) -> Result<String> {
        match self.scene.active_layer() {
            Some(l) if self.active_paint() => Ok(l.id.clone()),
            Some(l) => bail!("{} is not a paint layer", l.name),
            None => bail!("pick a paint layer first"),
        }
    }

    pub(super) fn refill(&mut self) -> Result<()> {
        let layer = self.active_paint_id()?;
        let frame = self.scene.frame;
        let args = refill_args(&self.bundle_arg(), &layer, frame);
        self.run_job(Tag::Edit, format!("refill {layer} at frame {frame}"), args, last_line);
        Ok(())
    }

    pub(super) fn fill_like(&mut self) -> Result<()> {
        let span = self
            .timeline
            .span
            .clone()
            .ok_or_else(|| anyhow!("select frames first: shift click or shift drag over cells"))?;
        let layer = span
            .rows
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("select frames on a paint layer"))?;
        let cel = self
            .scene
            .bundle
            .doc
            .layer(&layer)
            .and_then(|l| l.cel_at(span.from))
            .map(|c| c.id.clone())
            .ok_or_else(|| anyhow!("no drawing at frame {} to take the fills from", span.from))?;
        let args = fill_like_args(&self.bundle_arg(), &layer, &cel, (span.from, span.to));
        self.run_job(
            Tag::Edit,
            format!("fill frames {} to {} like {cel}", span.from, span.to),
            args,
            last_line,
        );
        Ok(())
    }

    pub(super) fn show_gaps(&mut self) -> Result<()> {
        if self.checks.gaps.take().is_some() {
            return Ok(());
        }
        let layer = self.active_paint_id()?;
        let frames = self
            .timeline
            .span
            .as_ref()
            .filter(|s| s.rows.contains(&layer))
            .map_or((self.scene.frame, self.scene.frame), |s| (s.from, s.to));
        let args = gaps_args(&self.bundle_arg(), &layer, frames);
        self.run_job(Tag::Gaps, format!("gaps on {layer}"), args, quiet);
        Ok(())
    }

    pub(super) fn track(&self) -> Result<String> {
        let (_, chosen) = self
            .chosen_marks()
            .ok_or_else(|| anyhow!("select a mark or the marks of a part with S first"))?;
        let doc = &self.scene.bundle.doc;
        let parts: Vec<Option<(String, String)>> = chosen
            .iter()
            .map(|(cel, m)| {
                let layer = doc
                    .all_layers()
                    .into_iter()
                    .find(|l| l.cels().iter().any(|c| &c.id == cel))?;
                let cel = layer.cels().iter().find(|c| &c.id == cel)?;
                Some((layer.name.clone(), cel.part_of(m.id())?.to_string()))
            })
            .collect();
        match (parts.first(), chosen.first()) {
            (Some(Some(first)), _) if parts.iter().all(|p| p.as_ref() == Some(first)) => {
                Ok(format!("{}:{}", first.0, first.1))
            }
            (_, Some((cel, m))) if chosen.len() == 1 => Ok(klad_core::markedit::address(cel, m.id())),
            _ => bail!("select one mark, or marks that are all one part, to track"),
        }
    }

    pub(super) fn show_arcs(&mut self) -> Result<()> {
        if self.checks.arcs.take().is_some() {
            return Ok(());
        }
        let track = self.track()?;
        let frames = match self.current_clip() {
            Some(c) => (c.from, c.to),
            None => (0, self.scene.bundle.doc.timeline.frames.saturating_sub(1)),
        };
        let args = arcs_args(&self.bundle_arg(), &track, frames);
        self.run_job(Tag::Arcs, format!("arcs of {track}"), args, quiet);
        Ok(())
    }

    pub(super) fn add_font(&mut self) -> Result<()> {
        let file = self.checks.font.trim().to_string();
        if file.is_empty() {
            bail!("type the path of a font file (.ttf or .otf) first");
        }
        let args = font_args(&self.bundle_arg(), &file);
        self.run_job(Tag::Edit, format!("add the font {file}"), args, last_line);
        Ok(())
    }

    pub(super) fn choose_font(&mut self) {
        if let Some(path) = crate::picker::choose_file(None, "Add") {
            self.checks.font = path.display().to_string();
        }
    }

    pub(super) fn font_window(&mut self, ui: &mut egui::Ui) -> Result<()> {
        ui.label("copies a font file into the drawing, so its text can use it");
        ui.horizontal(|ui| {
            let label = ui.label("file");
            ui.add(
                egui::TextEdit::singleline(&mut self.checks.font)
                    .desired_width(240.0)
                    .hint_text("/path/to/Font.ttf"),
            )
            .labelled_by(label.id)
            .hint("a .ttf or .otf file");
        });
        let (mut choose, mut add) = (false, false);
        ui.horizontal(|ui| {
            if crate::picker::NATIVE {
                choose = ui.button("choose...").hint("pick the font file").clicked();
            }
            add = ui
                .button("add the font")
                .hint("copy it into the drawing, as klad font does")
                .clicked();
        });
        let fonts = self.fonts();
        ui.small(match fonts.is_empty() {
            true => "the drawing has no fonts yet".to_string(),
            false => format!("in the drawing: {}", fonts.join(", ")),
        });
        if choose {
            self.choose_font();
        }
        if add {
            self.add_font()?;
        }
        Ok(())
    }

    pub(super) fn name_part(&mut self) -> Result<()> {
        let Some(picked) = self.picked.clone() else {
            bail!("select strokes with S first");
        };
        let (mut ops, after) = match &picked.cut {
            Some(_) => self
                .picked_ops(LassoAction::Transform(MarkTransform::default()))
                .ok_or_else(|| anyhow!("the selection changed underneath: select the strokes again"))?,
            None => (Vec::new(), Some(picked.clone())),
        };
        let ids = after.as_ref().map_or(Vec::new(), |p| p.addresses());
        let name = self.checks.part.trim().to_string();
        ops.push(Op::Part {
            ids,
            name: name.clone(),
        });
        self.apply(ops)?;
        self.picked = after;
        self.forget_missing_selection();
        self.say(
            Tone::Done,
            match name.is_empty() {
                true => "the marks are in no part now".to_string(),
                false => format!("the marks are part {name}"),
            },
        );
        Ok(())
    }

    pub(super) fn picked_part_names(&self) -> Vec<String> {
        let Some((_, chosen)) = self.chosen_marks() else {
            return Vec::new();
        };
        let doc = &self.scene.bundle.doc;
        let mut names: Vec<String> = chosen
            .iter()
            .map(|(cel, m)| {
                doc.all_layers()
                    .into_iter()
                    .flat_map(|l| l.cels().iter())
                    .find(|c| &c.id == cel)
                    .and_then(|c| c.part_of(m.id()))
                    .unwrap_or("no part")
                    .to_string()
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    pub(super) fn job_found(&mut self, tag: Tag, name: &str, output: &str) {
        match tag {
            Tag::Gaps => {
                let layer = name.trim_start_matches("gaps on ");
                self.checks.gaps = gaps_from(output, layer);
                let count: usize = self
                    .checks
                    .gaps
                    .as_ref()
                    .map_or(0, |g| g.frames.iter().map(|(_, p)| p.len()).sum());
                self.say(
                    match count {
                        0 => Tone::Done,
                        _ => Tone::Refused,
                    },
                    match count {
                        0 => "no gaps: every closed region is filled".to_string(),
                        n => format!("{n} closed regions without a fill, outlined on the canvas"),
                    },
                );
            }
            Tag::Arcs => {
                let track = name.trim_start_matches("arcs of ");
                self.checks.arcs = arcs_from(output, track);
            }
            Tag::Compare => self.compared(output),
            _ => {}
        }
    }

    fn pixel_findings(&self) -> Option<PixelCheck> {
        let layer = self.scene.active_layer().filter(|l| l.pixel())?;
        let doc = &self.scene.bundle.doc;
        let grid = klad_core::pixel::grid_of(self.scene.active_marks(), doc.canvas.width, doc.canvas.height);
        let colour = |key: &str| match doc.entry(key) {
            Some(swatch) => klad_core::color::parse_hex(&swatch.color).ok(),
            None => klad_core::color::parse_hex(key).ok(),
        };
        Some(PixelCheck {
            layer: layer.id.clone(),
            frame: self.scene.frame,
            rev: doc.rev,
            found: klad_core::pixelcheck::check(&grid, doc.canvas.wrap, &colour),
        })
    }

    pub(super) fn check_pixels(&mut self) {
        if self.checks.pixels.take().is_some() {
            return;
        }
        self.checks.pixels = self.pixel_findings();
        if let Some(check) = &self.checks.pixels {
            let words = tally(&check.found);
            self.say(
                match check.found.is_empty() {
                    true => Tone::Done,
                    false => Tone::Refused,
                },
                words,
            );
        }
    }

    pub(super) fn follow_pixel_check(&mut self) {
        let stale = self.checks.pixels.as_ref().is_some_and(|check| {
            check.rev != self.scene.bundle.doc.rev
                || check.frame != self.scene.frame
                || self.scene.active_id().as_deref() != Some(check.layer.as_str())
        });
        if stale {
            self.checks.pixels = self.pixel_findings();
        }
    }

    pub(super) fn draw_checks(&self, painter: &egui::Painter, place: impl Fn(Vec2) -> Pos2) {
        let frame = self.scene.frame;
        let dark = Stroke::new(3.5, Color32::from_black_alpha(150));
        if let Some(check) = &self.checks.pixels {
            use klad_core::pixelcheck::Kind;
            check.found.iter().for_each(|f| {
                let tint = match f.kind {
                    Kind::Stray => Color32::from_rgb(255, 196, 0),
                    Kind::Corner => Color32::from_rgb(255, 80, 120),
                    Kind::Jagged => Color32::from_rgb(255, 120, 40),
                    Kind::Banding => Color32::from_rgb(90, 200, 255),
                    Kind::OffPalette => Color32::from_rgb(200, 120, 255),
                };
                let r = Rect::from_two_pos(
                    place(vec2(f.at[0] as f32, f.at[1] as f32)),
                    place(vec2(f.to[0] as f32 + 1.0, f.to[1] as f32 + 1.0)),
                )
                .expand(2.0);
                painter.rect_stroke(r, 1.0, dark, egui::StrokeKind::Middle);
                painter.rect_stroke(r, 1.0, Stroke::new(1.5, tint), egui::StrokeKind::Middle);
            });
        }
        if let Some(gaps) = &self.checks.gaps {
            let warn = Color32::from_rgb(255, 80, 120);
            gaps.frames
                .iter()
                .filter(|(f, _)| *f == frame)
                .flat_map(|(_, pockets)| pockets.iter())
                .for_each(|b| {
                    let r =
                        Rect::from_two_pos(place(vec2(b[0], b[1])), place(vec2(b[0] + b[2], b[1] + b[3]))).expand(3.0);
                    painter.rect_stroke(r, 2.0, dark, egui::StrokeKind::Middle);
                    painter.rect_stroke(r, 2.0, Stroke::new(1.5, warn), egui::StrokeKind::Middle);
                });
        }
        if let Some(arcs) = &self.checks.arcs {
            arcs.lines.iter().for_each(|line| {
                let points: Vec<Pos2> = line.iter().map(|(_, p)| place(vec2(p[0], p[1]))).collect();
                painter.add(egui::Shape::line(points.clone(), dark));
                painter.add(egui::Shape::line(points, Stroke::new(1.5, theme::ACCENT)));
                line.iter().for_each(|(f, p)| {
                    let at = place(vec2(p[0], p[1]));
                    let here = *f == frame;
                    painter.circle_filled(at, if here { 5.0 } else { 3.0 }, Color32::WHITE);
                    painter.circle_stroke(at, if here { 5.0 } else { 3.0 }, Stroke::new(1.0, theme::ACCENT));
                });
            });
        }
        if self.checks.parts
            && let Some(cel) = self.scene.active_cel()
            && let Some(found) = self
                .scene
                .bundle
                .doc
                .all_layers()
                .into_iter()
                .flat_map(|l| l.cels().iter())
                .find(|c| c.id == cel)
        {
            let marks = self.scene.active_marks();
            found.parts.iter().for_each(|(name, ids)| {
                if let Some(b) = bounds_of(marks, ids) {
                    let r = Rect::from_two_pos(place(vec2(b[0], b[1])), place(vec2(b[2], b[3]))).expand(2.0);
                    painter.rect_stroke(r, 2.0, dark, egui::StrokeKind::Middle);
                    painter.rect_stroke(r, 2.0, Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Middle);
                    painter.text(
                        r.left_top() + vec2(0.0, -2.0),
                        egui::Align2::LEFT_BOTTOM,
                        name,
                        egui::FontId::proportional(12.0),
                        Color32::WHITE,
                    );
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_job_arguments_are_the_cli_commands() {
        assert_eq!(refill_args("a.klad", "l2", 4).join(" "), "refill a.klad l2 --frame 4");
        assert_eq!(
            fill_like_args("a.klad", "l2", "c7", (3, 9)).join(" "),
            "fill a.klad l2 --like c7 --frames 3-9"
        );
        assert_eq!(
            gaps_args("a.klad", "l2", (5, 5)).join(" "),
            "gaps a.klad l2 --json --frame 5"
        );
        assert_eq!(
            gaps_args("a.klad", "l2", (0, 11)).join(" "),
            "gaps a.klad l2 --json --sheet 0-11"
        );
        assert_eq!(
            arcs_args("a.klad", "lines:near leg", (12, 27)),
            [
                "arcs",
                "a.klad",
                "--track",
                "lines:near leg",
                "--from",
                "12",
                "--to",
                "27",
                "--json"
            ]
        );
        assert_eq!(font_args("a.klad", "/f/Sign.ttf").join(" "), "font a.klad /f/Sign.ttf");
    }

    #[test]
    fn gaps_and_arcs_read_the_cli_json_even_with_lines_after_it() {
        let gaps = gaps_from(
            r#"{"frames":[{"cel":"c3","frame":2,"pockets":[{"bounds":[10,20,30,40],"pixels":900}],"render":null}],"gap":4.0,"layer":"colour","ref":"lines"}
gaps rendered in 12 ms"#,
            "l2",
        )
        .unwrap();
        assert_eq!(gaps.frames, vec![(2, vec![[10.0, 20.0, 30.0, 40.0]])]);
        let arcs = arcs_from(
            r#"{"from":0,"to":1,"tracks":[{"frames":[{"at":[1.5,2.0],"frame":0},{"at":[3.0,4.0],"frame":1}]}]}"#,
            "leaf",
        )
        .unwrap();
        assert_eq!(arcs.lines, vec![vec![(0, [1.5, 2.0]), (1, [3.0, 4.0])]]);
        assert!(gaps_from("error: no layer", "l2").is_none());
    }
}
