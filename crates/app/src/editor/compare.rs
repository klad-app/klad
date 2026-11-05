use super::Editor;
use crate::jobs::{Spec, Tag};
use crate::messages::Tone;
use crate::theme::Hint;
use anyhow::{Context, Result};
use klad_core::doc::{Kind, Shape};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) struct Compared {
    pub report: Value,
    pub heat: Option<egui::TextureHandle>,
}

pub(super) fn reference_image(root: &Path, doc: &klad_core::doc::Document) -> Option<PathBuf> {
    doc.all_layers()
        .into_iter()
        .filter(|l| l.reference && l.kind() == Kind::Vector)
        .flat_map(|l| l.cels().iter())
        .flat_map(|c| c.nodes.iter())
        .find_map(|n| match &n.shape {
            Shape::Image(p) => Some(root.join(klad_render::assets::IMAGES).join(&p.src)),
            _ => None,
        })
}

pub(super) fn args(bundle: &Path, reference: &Path, frame: u32, heat: &Path) -> Vec<String> {
    vec![
        "compare".into(),
        bundle.to_string_lossy().to_string(),
        reference.to_string_lossy().to_string(),
        "--frame".into(),
        frame.to_string(),
        "--json".into(),
        "--heat".into(),
        heat.to_string_lossy().to_string(),
    ]
}

fn heat_texture(ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
    let bytes = std::fs::read(path).ok()?;
    let rgba = klad_render::assets::decode(&bytes).ok()?;
    let image = egui::ColorImage::from_rgba_unmultiplied([rgba.width as usize, rgba.height as usize], &rgba.pixels);
    Some(ctx.load_texture("compare heat", image, egui::TextureOptions::LINEAR))
}

impl Editor {
    pub(super) fn compare_reference(&mut self) -> Result<()> {
        let root = self.scene.bundle.root.clone();
        let reference = reference_image(&root, &self.scene.bundle.doc)
            .context("add a reference picture first: a picture layer marked as a reference")?;
        let heat = self.scene.bundle.cache_dir().join("compare-heat.png");
        self.start_job(Spec {
            tag: Tag::Compare,
            name: "compare with the reference".into(),
            exe: PathBuf::new(),
            args: args(&root, &reference, self.scene.frame, &heat),
            env: Vec::new(),
            reruns: 0,
            summary: |out, _| {
                serde_json::from_str::<Value>(out)
                    .ok()
                    .and_then(|v| {
                        v["words"]
                            .as_array()
                            .map(|w| w.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
                    })
                    .unwrap_or_default()
            },
            failure: |_, err| err.lines().last().unwrap_or_default().to_string(),
        });
        Ok(())
    }

    pub(super) fn compared(&mut self, output: &str) {
        match serde_json::from_str::<Value>(output) {
            Ok(report) => {
                let heat = report["heat"].as_str().and_then(|p| heat_texture(&self.ctx, p));
                self.compare = Some(Compared { report, heat });
                self.windows.compare = true;
            }
            Err(e) => self.say(Tone::Error, format!("the comparison could not be read: {e}")),
        }
    }

    pub(super) fn compare_window(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let Some(compared) = &self.compare else {
            ui.label("Compare runs against the drawing's reference picture.");
            return Ok(());
        };
        let r = &compared.report;
        let number = |v: &Value| v.as_f64().map_or("-".to_string(), |n| format!("{n:.1}"));
        let range = |v: &Value| {
            v.as_array()
                .map(|a| a.iter().map(number).collect::<Vec<_>>().join(" / "))
                .unwrap_or_default()
        };
        egui::Grid::new("compare numbers").num_columns(3).show(ui, |ui| {
            ui.label("");
            ui.strong("yours");
            ui.strong("reference");
            ui.end_row();
            ui.label("value L")
                .hint("lightness at the 5th, 50th and 95th percentile, 0 to 100");
            ui.label(range(&r["yours"]["value"]));
            ui.label(range(&r["reference"]["value"]));
            ui.end_row();
            [
                ("chroma", "how saturated, on average"),
                ("texture", "how busy the strokes are"),
                ("sharpness", "how hard the hardest edges are"),
            ]
            .iter()
            .for_each(|(key, hint)| {
                ui.label(*key).hint(*hint);
                ui.label(number(&r["yours"][key]));
                ui.label(number(&r["reference"][key]));
                ui.end_row();
            });
        });
        let words: Vec<&str> = r["words"]
            .as_array()
            .map_or_else(Vec::new, |w| w.iter().filter_map(Value::as_str).collect());
        ui.label(format!("difference {}: {}", number(&r["difference"]), words.join(", ")));
        if let Some(heat) = &compared.heat {
            let size = heat.size_vec2();
            let scale = (320.0 / size.x.max(1.0)).min(1.0);
            ui.image((heat.id(), size * scale))
                .hint("where they differ most: black is the same, red, yellow and white differ more");
        }
        if ui.button("compare again").hint("after painting more").clicked() {
            self.compare_reference()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_builds_the_cli_compare_and_finds_the_reference() {
        let joined = args(
            Path::new("/w/a.draw"),
            Path::new("/w/a.draw/images/f.png"),
            2,
            Path::new("/w/h.png"),
        )
        .join(" ");
        assert_eq!(
            joined,
            "compare /w/a.draw /w/a.draw/images/f.png --frame 2 --json --heat /w/h.png"
        );
        let mut doc = klad_core::doc::Document::new(100, 100);
        assert!(reference_image(Path::new("/w/a.draw"), &doc).is_none());
        let node: klad_core::doc::Node =
            serde_json::from_value(serde_json::json!({"id": "n1", "type": "image", "src": "f.png", "w": 10, "h": 10}))
                .unwrap();
        let mut layer = klad_core::doc::Layer::vector("l1", "photo", "c1", vec![node]);
        layer.reference = true;
        doc.layers = vec![layer];
        assert_eq!(
            reference_image(Path::new("/w/a.draw"), &doc),
            Some(PathBuf::from("/w/a.draw/images/f.png"))
        );
    }
}
