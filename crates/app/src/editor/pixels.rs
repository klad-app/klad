use super::Editor;
use crate::jobs::{Spec, Tag};
use crate::theme::Hint;
use anyhow::{Result, bail};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Source {
    #[default]
    File,
    Layer,
    Whole,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Form {
    pub source: Source,
    pub path: String,
    pub size: u32,
    pub colors: u32,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            source: Source::File,
            path: String::new(),
            size: 64,
            colors: 0,
        }
    }
}

pub(super) fn args(bundle: &str, layer: &str, frame: u32, form: &Form) -> Vec<String> {
    let mut args = vec!["pixelize".to_string(), bundle.to_string()];
    let mut push = |flag: &str, value: String| {
        args.push(flag.to_string());
        args.push(value);
    };
    match form.source {
        Source::File => {
            push("--image", form.path.trim().to_string());
            push("--to", layer.to_string());
            push("--frame", frame.to_string());
            if form.size > 0 {
                push("--size", form.size.to_string());
            }
        }
        Source::Layer => {
            push("--layer", layer.to_string());
            push("--size", form.size.max(1).to_string());
        }
        Source::Whole => push("--size", form.size.max(1).to_string()),
    }
    if form.colors >= 2 {
        push("--colors", form.colors.to_string());
    }
    args
}

fn made(out: &str, err: &str) -> String {
    let drawing = out.lines().map(str::trim).rfind(|l| l.ends_with(".klad"));
    let notes: Vec<&str> = err.lines().filter_map(|l| l.trim().strip_prefix("note: ")).collect();
    match drawing {
        Some(path) => format!("made {path}: open it from File > Open"),
        None => notes.first().map_or(String::new(), |n| n.to_string()),
    }
}

impl Editor {
    pub(super) fn make_pixels(&mut self) -> Result<()> {
        let form = self.pixels.clone();
        let layer = self
            .scene
            .active_layer()
            .map(|l| (l.id.clone(), l.name.clone(), l.pixel()));
        let name = match (form.source, &layer) {
            (Source::File, _) if form.path.trim().is_empty() => {
                bail!("type the path of a PNG or JPEG file first")
            }
            (Source::File, Some((_, name, true))) => format!("pixels of the picture on {name}"),
            (Source::File, _) => {
                bail!("pick a pixel layer first: the picture's pixels go on it (Layer > New pixel layer)")
            }
            (Source::Layer, Some((_, name, _))) => format!("a pixel drawing of {name}"),
            (Source::Layer, None) => bail!("pick the layer to make pixels of first"),
            (Source::Whole, _) => "a pixel drawing of the picture".to_string(),
        };
        let layer_id = layer.map_or(String::new(), |(id, _, _)| id);
        let args = args(
            &self.scene.bundle.root.to_string_lossy(),
            &layer_id,
            self.scene.frame,
            &form,
        );
        self.start_job(Spec {
            tag: Tag::Edit,
            name,
            exe: PathBuf::new(),
            args,
            env: Vec::new(),
            reruns: 3,
            summary: made,
            failure: |_, err| super::tweens::failure(err),
        });
        Ok(())
    }

    pub(super) fn pixels_window(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let on_pixels = self.scene.active_layer().is_some_and(|l| l.pixel());
        let form = &mut self.pixels;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut form.source, Source::File, "a picture file")
                .hint("a PNG or JPEG becomes pixels on the pixel layer you are on, in the palette's colours");
            ui.selectable_value(&mut form.source, Source::Layer, "this layer")
                .hint("a new pixel drawing of the layer you are on, every frame of it; this drawing is not changed");
            ui.selectable_value(&mut form.source, Source::Whole, "the whole picture")
                .hint("a new pixel drawing of everything that shows, every frame of it; this drawing is not changed");
        });
        let mut choose = false;
        if form.source == Source::File {
            ui.horizontal(|ui| {
                let label = ui.label("file");
                ui.add(
                    egui::TextEdit::singleline(&mut form.path)
                        .desired_width(240.0)
                        .hint_text("/path/to/picture.png"),
                )
                .labelled_by(label.id)
                .hint("a PNG or JPEG file");
                if crate::picker::NATIVE {
                    choose = ui.button("choose...").hint("pick the picture").clicked();
                }
            });
            if !on_pixels {
                ui.weak("pick a pixel layer first: the pixels go on the layer you are on");
            }
        }
        let least = match form.source {
            Source::File => 0,
            _ => 1,
        };
        form.size = form.size.clamp(least, 512);
        ui.add(egui::Slider::new(&mut form.size, least..=512).text("longest side px"))
            .hint(match form.source {
                Source::File => "the picture's longest side once it is pixels; 0 keeps its own size (up to 512 px)",
                _ => "the longest side of the pixel drawing: 64 for a 64 px sprite",
            });
        ui.add(egui::Slider::new(&mut form.colors, 0..=64).text("colours"))
            .hint("0: every pixel takes the nearest colour of the palette (or the picture's own colours when the palette is empty); from 2: that many colours are made from the picture");
        if form.colors == 1 {
            form.colors = 2;
        }
        let run = ui.button("make pixels").hint("as klad pixelize does").clicked();
        if choose && let Some(path) = crate::picker::choose_file(None, "Choose") {
            self.pixels.path = path.display().to_string();
        }
        if run {
            self.make_pixels()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_builds_the_cli_call() {
        let file = Form {
            path: " /p/ref.png ".into(),
            size: 0,
            colors: 0,
            ..Form::default()
        };
        assert_eq!(
            args("/w/art.klad", "l3", 2, &file).join(" "),
            "pixelize /w/art.klad --image /p/ref.png --to l3 --frame 2"
        );
        let sized = Form {
            path: "/p/ref.png".into(),
            size: 48,
            colors: 12,
            ..Form::default()
        };
        assert_eq!(
            args("/w/art.klad", "l3", 0, &sized).join(" "),
            "pixelize /w/art.klad --image /p/ref.png --to l3 --frame 0 --size 48 --colors 12"
        );
        let layer = Form {
            source: Source::Layer,
            size: 64,
            colors: 1,
            ..Form::default()
        };
        assert_eq!(
            args("/w/art.klad", "l3", 5, &layer).join(" "),
            "pixelize /w/art.klad --layer l3 --size 64"
        );
        let whole = Form {
            source: Source::Whole,
            size: 32,
            colors: 8,
            ..Form::default()
        };
        assert_eq!(
            args("/w/art.klad", "l3", 5, &whole).join(" "),
            "pixelize /w/art.klad --size 32 --colors 8"
        );
        assert_eq!(
            made(
                "/w/art-pixel.klad\n",
                "64x64, 28 frames\nnote: every pixel took the nearest\n"
            ),
            "made /w/art-pixel.klad: open it from File > Open"
        );
        assert_eq!(
            made(
                "rev 9\n",
                "note: 6x5 pixels in 1 colour on pixel 1\nnote: every pixel\n"
            ),
            "6x5 pixels in 1 colour on pixel 1"
        );
    }
}
