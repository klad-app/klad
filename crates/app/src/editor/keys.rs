use super::Editor;
use crate::theme::{self, Hint};
use anyhow::Result;
use egui::RichText;
use klad_core::doc::Item;
use klad_core::keys::{Ease, Keys, PROPS, value};
use klad_core::ops::{KeySpec, Op};
use std::collections::BTreeMap;

const EASES: [(Ease, &str); 5] = [
    (Ease::Linear, "linear"),
    (Ease::In, "in"),
    (Ease::Out, "out"),
    (Ease::InOut, "in-out"),
    (Ease::Step, "step"),
];

fn resting(doc: &klad_core::doc::Document, id: &str) -> Option<(Keys, [f32; 6])> {
    match doc.item(id)? {
        Item::Layer(l) => Some((l.keys.clone(), [0.0, 0.0, 0.0, 1.0, 1.0, l.opacity])),
        Item::Node(_, _, n) => Some((
            n.keys.clone(),
            [
                n.transform.x,
                n.transform.y,
                n.transform.rotate,
                n.transform.sx,
                n.transform.sy,
                n.opacity,
            ],
        )),
        _ => None,
    }
}

impl Editor {
    pub(super) fn key_here(&mut self, id: &str, values: BTreeMap<String, f32>, rm: bool) -> Result<()> {
        self.apply(vec![Op::Key(KeySpec {
            id: id.to_string(),
            frame: self.scene.frame,
            values,
            ease: self.key_ease,
            rm,
        })])
    }

    pub(super) fn animation_section(&mut self, ui: &mut egui::Ui, id: &str) -> Result<()> {
        let Some((keys, rest)) = resting(&self.scene.bundle.doc, id) else {
            return Ok(());
        };
        let frame = self.scene.frame;
        let mut run: Option<(BTreeMap<String, f32>, bool)> = None;
        ui.collapsing(RichText::new("keys").strong(), |ui| {
            ui.label(
                RichText::new("animate without a drawing per frame: set a value here and key it at this frame")
                    .color(theme::WEAK),
            );
            PROPS.iter().zip(rest).for_each(|(prop, rest)| {
                let now = value(&keys, prop, frame).unwrap_or(rest);
                ui.horizontal(|ui| {
                    let slot = format!("{id}.key.{prop}");
                    let mut v = self.edits.get(&slot).copied().unwrap_or(now);
                    let r = ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .speed(if *prop == "opacity" { 0.01 } else { 0.5 })
                                .prefix(format!("{prop} ")),
                        )
                        .hint(format!("{prop} at this frame; key it to keep it"));
                    if r.changed() {
                        self.edits.insert(slot.clone(), v);
                    }
                    if ui.button("key").hint(format!("key {prop} at frame {frame}")).clicked() {
                        self.edits.remove(&slot);
                        run = Some((BTreeMap::from([(prop.to_string(), v)]), false));
                    }
                    let list = keys.get(*prop).cloned().unwrap_or_default();
                    let shown: Vec<String> = list.iter().map(|k| format!("{}:{:.1}", k.frame, k.value)).collect();
                    if !shown.is_empty() {
                        ui.label(RichText::new(shown.join(" ")).small().color(theme::WEAK));
                    }
                });
            });
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(format!("{id}.ease"))
                    .selected_text(
                        EASES
                            .iter()
                            .find(|(e, _)| *e == self.key_ease)
                            .map_or("curve", |(_, n)| *n),
                    )
                    .show_ui(ui, |ui| {
                        EASES.iter().for_each(|(e, n)| {
                            ui.selectable_value(&mut self.key_ease, *e, *n);
                        });
                    })
                    .response
                    .hint("how the next keys ease towards the key after them");
                if ui
                    .button("remove keys here")
                    .hint(format!("every key at frame {frame}"))
                    .clicked()
                {
                    run = Some((BTreeMap::new(), true));
                }
            });
        });
        if let Some((values, rm)) = run {
            self.key_here(id, values, rm)?;
        }
        Ok(())
    }
}
