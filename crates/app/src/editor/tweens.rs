use super::Editor;
use crate::messages::Tone;
use anyhow::Result;
use klad_core::doc::{Document, Kind};
use klad_core::ops::Op;
use klad_core::retime::{Retime, RetimeSpec};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Span {
    pub from: u32,
    pub to: u32,
    pub rows: Vec<String>,
    pub anchor: (u32, usize),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Ask {
    pub every_frame: bool,
    pub count: u32,
    pub spacing: String,
    pub matching: String,
    pub every_layer: bool,
    pub fit: u32,
    pub unpaired: String,
}

impl Default for Ask {
    fn default() -> Self {
        Self {
            every_frame: true,
            count: 1,
            spacing: "even".into(),
            matching: "id".into(),
            every_layer: false,
            fit: 8,
            unpaired: "leave".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum SpanAction {
    Retime(Retime),
    Inbetween,
    Redo,
    Clear,
}

pub(super) fn keys(doc: &Document, span: &Span) -> Option<(u32, u32)> {
    let layer = doc.layer(span.rows.first()?)?;
    if layer.kind() != Kind::Paint {
        return None;
    }
    layer.content_at(span.from)?;
    let to = layer
        .cels()
        .iter()
        .filter(|c| c.frame > span.from && c.frame <= span.to)
        .map(|c| c.frame)
        .max()?;
    (to > span.from + 1).then_some((span.from, to))
}

pub(super) fn span_menu(ui: &mut egui::Ui, span: &Span, keys: Option<(u32, u32)>, ask: &mut Ask) -> Option<SpanAction> {
    let mut chosen = None;
    ui.separator();
    ui.label(format!(
        "selected: frames {} to {} on {}",
        span.from,
        span.to,
        span.rows.join(", ")
    ));
    ui.menu_button("retime the selected frames", |ui| {
        ui.checkbox(&mut ask.every_layer, "on every layer (later frames and clips move too)");
        [(1, "on ones"), (2, "on twos"), (3, "on threes")]
            .iter()
            .for_each(|(n, label)| {
                if ui.button(*label).clicked() {
                    chosen = Some(SpanAction::Retime(Retime::On(*n)));
                    ui.close();
                }
            });
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut ask.fit).range(1..=10_000));
            if ui.button("fit into this many frames").clicked() {
                chosen = Some(SpanAction::Retime(Retime::Fit(ask.fit)));
                ui.close();
            }
        });
        if ui.button("reverse").clicked() {
            chosen = Some(SpanAction::Retime(Retime::Reverse));
            ui.close();
        }
        if ui
            .button("ping-pong (back again as links, ends not repeated)")
            .clicked()
        {
            chosen = Some(SpanAction::Retime(Retime::Pingpong));
            ui.close();
        }
    });
    let can = keys.is_some();
    if ui
        .add_enabled(can, egui::Button::new("in-between..."))
        .on_disabled_hover_text("select from one drawing to the frame where another starts, on a paint layer")
        .clicked()
    {
        chosen = Some(SpanAction::Inbetween);
        ui.close();
    }
    if ui
        .button("redo the in-betweens in the selection (after a key changed)...")
        .clicked()
    {
        chosen = Some(SpanAction::Redo);
        ui.close();
    }
    if ui.button("clear the selection (Esc)").clicked() {
        chosen = Some(SpanAction::Clear);
        ui.close();
    }
    chosen
}

impl Editor {
    pub(super) fn span_action(&mut self, action: SpanAction) -> Result<()> {
        let Some(span) = self.timeline.span.clone() else {
            return Ok(());
        };
        match action {
            SpanAction::Clear => self.timeline.span = None,
            SpanAction::Retime(how) => {
                let spec = RetimeSpec {
                    from: span.from,
                    to: span.to,
                    action: how,
                    layers: (!self.timeline.ask.every_layer).then(|| span.rows.clone()),
                };
                if let Some(applied) = self.timeline_apply(vec![Op::Retime(spec)])? {
                    if let Some(note) = applied.notes.first() {
                        self.say(Tone::Done, note.clone());
                    }
                    self.timeline.span = None;
                }
            }
            SpanAction::Inbetween => {
                self.windows.between = true;
                self.between.redo = None;
            }
            SpanAction::Redo => self.check_redo()?,
        }
        Ok(())
    }
}

pub(super) fn summary(stdout: &str) -> String {
    let keep: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains(" in-between") || l.contains("left out") || l.contains("unpaired"))
        .map(str::trim)
        .collect();
    keep.join("; ")
}

pub(super) fn failure(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(3)..].join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::{Cel, Layer};

    #[test]
    fn the_keys_of_a_span_are_its_first_drawing_and_the_last_one_starting_in_it() {
        let mut doc = Document::new(64, 64);
        doc.timeline.frames = 12;
        let mut layer = Layer::paint("l1", "lines", "c1");
        layer.cels_mut().unwrap().extend([Cel::at("c2", 4), Cel::at("c3", 8)]);
        doc.layers.push(layer);
        let span = |from: u32, to: u32| Span {
            from,
            to,
            rows: vec!["l1".into()],
            anchor: (from, 0),
        };
        assert_eq!(keys(&doc, &span(0, 4)), Some((0, 4)));
        assert_eq!(keys(&doc, &span(0, 9)), Some((0, 8)));
        assert_eq!(keys(&doc, &span(1, 6)), Some((1, 4)));
        assert_eq!(keys(&doc, &span(0, 3)), None);
        assert_eq!(keys(&doc, &span(3, 4)), None);
    }
}
