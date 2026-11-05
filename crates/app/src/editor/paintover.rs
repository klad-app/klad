use super::Editor;
use crate::jobs::{Spec, Tag};
use crate::theme::Hint;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Form {
    pub sizes: String,
    pub threshold: f32,
    pub length: [f32; 2],
    pub curve: f32,
    pub brush: String,
    pub vary: f32,
    pub within: String,
    pub focus: String,
    pub seed: u32,
    pub until: String,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            sizes: "32,16,8".into(),
            threshold: 6.0,
            length: [1.0, 8.0],
            curve: 0.5,
            brush: "oil".into(),
            vary: 0.025,
            within: String::new(),
            focus: String::new(),
            seed: 1,
            until: String::new(),
        }
    }
}

pub(super) fn args(bundle: &Path, layer: &str, frame: u32, form: &Form, like: Option<(&str, String)>) -> Vec<String> {
    let mut args = vec![
        "paintover".to_string(),
        bundle.to_string_lossy().to_string(),
        layer.to_string(),
        "--frame".into(),
        frame.to_string(),
        "--sizes".into(),
        form.sizes.trim().replace(' ', ""),
        "--threshold".into(),
        form.threshold.to_string(),
        "--length".into(),
        format!("{},{}", form.length[0], form.length[1]),
        "--curve".into(),
        form.curve.to_string(),
        "--brush".into(),
        form.brush.clone(),
        "--vary".into(),
        form.vary.to_string(),
        "--seed".into(),
        form.seed.to_string(),
    ];
    [("--in", &form.within), ("--focus", &form.focus)]
        .iter()
        .filter(|(_, v)| !v.trim().is_empty())
        .for_each(|(k, v)| args.extend([k.to_string(), v.trim().to_string()]));
    if let Some((cel, frames)) = like {
        args.extend(["--like".into(), cel.to_string(), "--frames".into(), frames]);
    }
    args
}

impl Editor {
    fn paint_over(&mut self, carry: bool) -> Result<()> {
        let Some(layer) = self
            .scene
            .active_layer()
            .filter(|l| l.kind() == klad_core::doc::Kind::Paint)
        else {
            bail!("pick the paint layer to paint on first; paint over paints what is below it");
        };
        let form = self.paintover.clone();
        if form
            .sizes
            .split(',')
            .any(|s| s.trim().parse::<f32>().map_or(true, |v| v <= 0.0))
        {
            bail!("brush sizes are numbers above 0, biggest first: 32,16,8");
        }
        let frame = self.scene.frame;
        let like = match carry {
            false => None,
            true => {
                let Some(cel) = layer.content_at(frame) else {
                    bail!(
                        "paint over this drawing first: there is no drawing on {} here to carry",
                        layer.name
                    );
                };
                let last = match form.until.trim().parse::<u32>() {
                    Ok(f) => f,
                    Err(_) => self
                        .current_clip()
                        .map_or(self.scene.bundle.doc.timeline.frames.saturating_sub(1), |c| c.to),
                };
                Some((cel.id.clone(), format!("{frame}-{last}")))
            }
        };
        let name = match &like {
            Some((_, frames)) => format!("carry the paint-over on {} over frames {frames}", layer.name),
            None => format!("paint over on {} at frame {frame}", layer.name),
        };
        let args = args(
            &self.scene.bundle.root,
            &layer.id,
            frame,
            &form,
            like.as_ref().map(|(c, f)| (c.as_str(), f.clone())),
        );
        self.start_job(Spec {
            tag: Tag::Edit,
            name,
            exe: PathBuf::new(),
            args,
            env: Vec::new(),
            reruns: 3,
            summary: |_, err| err.lines().last().unwrap_or_default().to_string(),
            failure: |_, err| super::tweens::failure(err),
        });
        Ok(())
    }

    pub(super) fn paintover_window(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let mut run = None;
        let form = &mut self.paintover;
        ui.label(
            "Repaints what is below the active layer with brush strokes that follow its forms, big brushes first.",
        );
        ui.horizontal(|ui| {
            let label = ui.label("brush sizes");
            ui.add(egui::TextEdit::singleline(&mut form.sizes).desired_width(90.0))
                .labelled_by(label.id)
                .hint("px, biggest first; smaller ones only go where detail is left");
            let label = ui.label("brush");
            ui.add(egui::TextEdit::singleline(&mut form.brush).desired_width(90.0))
                .labelled_by(label.id)
                .hint("any brush id of this drawing or the library");
        });
        ui.add(egui::Slider::new(&mut form.threshold, 1.0..=25.0).text("how close"))
            .hint("lower paints more and smaller strokes, until the painting is this close to the source");
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut form.length[0])
                    .range(0.0..=20.0)
                    .speed(0.1)
                    .prefix("length "),
            )
            .hint("the shortest stroke, in brush widths");
            ui.add(
                egui::DragValue::new(&mut form.length[1])
                    .range(1.0..=40.0)
                    .speed(0.1)
                    .prefix("to "),
            )
            .hint("the longest stroke, in brush widths");
        });
        ui.add(egui::Slider::new(&mut form.curve, 0.0..=1.0).text("follow the forms"))
            .hint("0 straight strokes, 1 strokes that turn with every bend");
        ui.add(egui::Slider::new(&mut form.vary, 0.0..=0.1).text("break the colour"))
            .hint("each stroke a little warmer or cooler, lighter or darker");
        ui.horizontal(|ui| {
            let label = ui.label("only in");
            ui.add(
                egui::TextEdit::singleline(&mut form.within)
                    .desired_width(110.0)
                    .hint_text("all"),
            )
            .labelled_by(label.id)
            .hint("ids of closed shapes to paint inside, with commas");
            let label = ui.label("detail near");
            ui.add(
                egui::TextEdit::singleline(&mut form.focus)
                    .desired_width(80.0)
                    .hint_text("anywhere"),
            )
            .labelled_by(label.id)
            .hint("a closed shape: the smallest brush only works in and near it");
        });
        ui.add(egui::DragValue::new(&mut form.seed).prefix("seed "))
            .hint("another seed paints other strokes the same way");
        ui.horizontal(|ui| {
            if ui.button("paint over this drawing").hint("klad paintover, one undo step").clicked() {
                run = Some(false);
            }
            if ui
                .button("carry it to the next drawings")
                .hint("each later drawing up to the end of the clip takes this one's strokes, moved with the drawing under them, so animation does not boil")
                .clicked()
            {
                run = Some(true);
            }
            let label = ui.label("until frame");
            ui.add(egui::TextEdit::singleline(&mut form.until).desired_width(40.0).hint_text("clip end"))
                .labelled_by(label.id)
                .hint("the last frame to carry it to; empty: the end of the clip, or of the timeline");
        });
        if let Some(carry) = run {
            self.paint_over(carry)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_builds_the_cli_paintover() {
        let bundle = Path::new("/w/art.draw");
        let form = Form {
            within: "rock".into(),
            ..Form::default()
        };
        assert_eq!(
            args(bundle, "l4", 3, &form, None).join(" "),
            "paintover /w/art.draw l4 --frame 3 --sizes 32,16,8 --threshold 6 --length 1,8 --curve 0.5 --brush oil --vary 0.025 --seed 1 --in rock"
        );
        let carried = args(bundle, "l4", 3, &Form::default(), Some(("c9", "3-12".into()))).join(" ");
        assert!(carried.ends_with("--like c9 --frames 3-12"), "{carried}");
    }
}
