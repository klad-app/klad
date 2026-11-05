use super::Editor;
use super::options;
use crate::messages::Tone;
use crate::theme::{self, Hint};
use anyhow::{Result, anyhow, bail};
use egui::{Color32, RichText, TextureHandle, TextureOptions};
use klad_core::brushes::{AngleDef, BrushDef, ColorDef, Curve, DualDef, GrainDef, PaintDef, Paper, TIP_IMAGES, TipDef};
use std::path::Path;

pub(super) const PREVIEW: (u32, u32) = (320, 84);

#[derive(Default)]
pub(super) struct BrushWindow {
    pub name: String,
    pub picture_path: String,
    preview: Option<(u64, TextureHandle)>,
}

fn slider(ui: &mut egui::Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, text: &str, hint: &str) -> bool {
    ui.add(egui::Slider::new(value, range).text(text)).hint(hint).changed()
}

fn curve_of(c: Option<Curve>) -> Curve {
    c.unwrap_or(Curve {
        min: 1.0,
        max: 1.0,
        gamma: 1.0,
    })
}

fn plain(c: Curve) -> Option<Curve> {
    (c.min != 1.0 || c.max != 1.0 || c.gamma != 1.0).then_some(c)
}

pub(super) fn slug(name: &str) -> String {
    let words: Vec<String> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    words.join("-")
}

fn tip_name(tip: &TipDef) -> String {
    match tip {
        TipDef::Round { .. } => "round".into(),
        TipDef::Chalk { .. } => "chalk".into(),
        TipDef::Image { image, more } if more.is_empty() => image.clone(),
        TipDef::Image { image, more } => format!("{image} +{}", more.len()),
    }
}

fn tip_choice(ui: &mut egui::Ui, salt: &str, tip: &mut TipDef, pictures: &[String], hint: &str) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(salt)
        .selected_text(tip_name(tip))
        .show_ui(ui, |ui| {
            let mut pick = |ui: &mut egui::Ui, label: &str, next: TipDef| {
                if ui.selectable_label(tip_name(tip) == label, label).clicked() && tip_name(tip) != label {
                    *tip = next;
                    changed = true;
                }
            };
            pick(ui, "round", TipDef::Round { hardness: 0.8 });
            pick(ui, "chalk", TipDef::Chalk { seed: 7 });
            pictures.iter().for_each(|name| {
                pick(
                    ui,
                    name,
                    TipDef::Image {
                        image: name.clone(),
                        more: Vec::new(),
                    },
                );
            });
        })
        .response
        .hint(hint);
    match tip {
        TipDef::Round { hardness } => changed |= slider(ui, hardness, 0.0..=1.0, "hardness", "soft to hard edge"),
        TipDef::Chalk { seed } => {
            changed |= ui
                .add(egui::DragValue::new(seed).range(0..=9999).prefix("seed "))
                .hint("which chalk grain")
                .changed()
        }
        TipDef::Image { .. } => {}
    }
    changed
}

impl Editor {
    #[cfg(test)]
    pub(super) fn brush_window_preview_key(&self) -> Option<u64> {
        self.brush_window.preview.as_ref().map(|(k, _)| *k)
    }

    pub(super) fn draft(&self) -> Result<BrushDef> {
        match &self.custom {
            Some(def) => Ok(def.clone()),
            None => Ok(self.shaped()?.1),
        }
    }

    fn picture_names(&self) -> Vec<String> {
        TIP_IMAGES
            .iter()
            .map(|n| n.to_string())
            .chain(self.brushes.pictures().keys().cloned())
            .collect()
    }

    pub(super) fn add_brush_picture(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        klad_core::brushes::valid_image(name)?;
        let picture = klad_brush::images::decode(bytes)?;
        let path = self
            .scene
            .bundle
            .root
            .join("brushes")
            .join("images")
            .join(format!("{name}.png"));
        if path.exists() && std::fs::read(&path)? != bytes {
            bail!(
                "a picture {name} is in this drawing already with other pixels; brushes using it would change, so give it another name"
            );
        }
        std::fs::create_dir_all(path.parent().ok_or_else(|| anyhow!("no folder"))?)?;
        klad_core::bundle::write_atomic(&path, bytes)?;
        self.brushes.add_picture(name, picture);
        self.say(
            Tone::Done,
            format!("picture {name} added: pick it as a tip or a texture"),
        );
        Ok(())
    }

    fn add_picture_from(&mut self, path: &Path) -> Result<()> {
        let name = slug(path.file_stem().and_then(|s| s.to_str()).unwrap_or_default());
        if name.is_empty() {
            bail!("{} has no usable name", path.display());
        }
        let bytes = std::fs::read(path)?;
        self.add_brush_picture(&name, &bytes)
    }

    pub(super) fn save_brush(&mut self) -> Result<()> {
        let mut def = self.draft()?;
        let id = slug(&self.brush_window.name);
        if id.is_empty() {
            bail!("give the brush a name first");
        }
        def.name = self.brush_window.name.trim().to_string();
        match self.scene.bundle.brush(&id)? {
            Some(known) if known == def => {}
            Some(_) => bail!(
                "a brush {id} is in this drawing already with other settings; strokes keep their brush, so pick another name"
            ),
            None => {
                if klad_core::brushes::library(&id).is_some() {
                    bail!("{id} is a library brush; pick another name");
                }
                self.scene.bundle.embed_brush(&id, &def)?;
            }
        }
        self.embedded.insert(id.clone(), def.clone());
        self.brushes.insert(&id, def);
        self.pick(&id);
        self.say(Tone::Done, format!("brush {id} saved in this drawing"));
        Ok(())
    }

    fn preview(&mut self, ctx: &egui::Context, def: &BrushDef) -> Option<TextureHandle> {
        let key = def.fingerprint() ^ self.brushes.fingerprint();
        if let Some((k, texture)) = &self.brush_window.preview
            && *k == key
        {
            return Some(texture.clone());
        }
        let mut set = self.brushes.clone();
        set.insert("draft", def.clone());
        let raster = options::sample_sized(&set, "draft", PREVIEW, 28.0).ok()?;
        let image = options::image_of(&raster, PREVIEW, Color32::from_gray(38));
        let texture = ctx.load_texture("brush preview", image, TextureOptions::LINEAR);
        self.brush_window.preview = Some((key, texture.clone()));
        Some(texture)
    }

    pub(super) fn brush_window_ui(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
                .collect()
        });
        dropped.iter().try_for_each(|p| self.add_picture_from(p))?;
        let mut def = self.draft()?;
        let pictures = self.picture_names();
        if let Some(texture) = self.preview(ui.ctx(), &def) {
            ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::vec2(PREVIEW.0 as f32, PREVIEW.1 as f32)));
        }
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(380.0);
            let mut dual = def.dual.is_some();
            if ui
                .checkbox(&mut dual, RichText::new("dual tip").strong())
                .hint("a second tip, scattered, cuts the first into a broken, bristly mark")
                .changed()
            {
                def.dual = dual.then(|| DualDef {
                    tip: TipDef::Image {
                        image: "bristle-round".into(),
                        more: Vec::new(),
                    },
                    size: 0.5,
                    scatter: 1.0,
                    count: 3,
                });
                changed = true;
            }
            let mut textured = def.grain.is_some();
            if ui
                .checkbox(&mut textured, RichText::new("texture").strong())
                .hint("paper or a picture under the paint, fixed on the canvas")
                .changed()
            {
                def.grain = textured.then_some(GrainDef {
                    paper: Some(Paper { seed: 23, feature: 4 }),
                    image: None,
                    strength: 0.4,
                    scale: 1.0,
                    contrast: 1.0,
                    pressure: 0.0,
                });
                changed = true;
            }
            let mut colour = def.color.is_some();
            if ui
                .checkbox(&mut colour, RichText::new("colour dynamics").strong())
                .hint("colour that varies per dab, per stroke, with pressure or along the stroke")
                .changed()
            {
                def.color = colour.then(ColorDef::default);
                changed = true;
            }
        });
        ui.horizontal(|ui| {
            let kind = match def.paint {
                None => 0,
                Some(PaintDef::Wet { .. }) => 1,
                Some(PaintDef::Smudge { .. }) => 2,
                Some(PaintDef::Blend { .. }) => 3,
            };
            [
                (0, "dry", "lays its own colour"),
                (1, "wet", "picks up the colour under it and mixes it in"),
                (2, "smudge", "pulls the paint along"),
                (3, "blend", "softens what is under it"),
            ]
            .iter()
            .for_each(|(k, label, hint)| {
                if ui.selectable_label(kind == *k, *label).hint(*hint).clicked() && kind != *k {
                    def.paint = match k {
                        1 => Some(PaintDef::Wet {
                            wetness: 0.6,
                            pickup: 0.5,
                            dilution: 0.2,
                        }),
                        2 => Some(PaintDef::Smudge { strength: 0.7 }),
                        3 => Some(PaintDef::Blend { strength: 0.5 }),
                        _ => None,
                    };
                    changed = true;
                }
            });
        });
        changed |= slider(
            ui,
            &mut def.body,
            0.0..=1.0,
            "body",
            "how thick the paint lies: strokes leave ridges that a relief effect on the layer lights; 0 is flat paint",
        );
        egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
            ui.set_max_width(380.0);
            ui.label(RichText::new("tip").strong());
            ui.vertical(|ui| {
                changed |= tip_choice(
                    ui,
                    "brush tip",
                    &mut def.tip,
                    &pictures,
                    "the shape each dab stamps: round, chalk or a picture",
                );
            });
            ui.vertical(|ui| {
                changed |= slider(
                    ui,
                    &mut def.spacing,
                    0.01..=2.0,
                    "spacing",
                    "the gap between dabs, as a part of the size",
                );
                changed |= slider(ui, &mut def.roundness, 0.05..=1.0, "roundness", "squash the tip");
            });
            ui.vertical(|ui| {
                let mut follow = def.angle == AngleDef::Follow;
                let mut tilt = def.angle == AngleDef::Tilt;
                if ui
                    .checkbox(&mut follow, "turn with the stroke")
                    .hint("the tip turns along the stroke")
                    .changed()
                {
                    def.angle = if follow { AngleDef::Follow } else { AngleDef::Fixed(0.0) };
                    changed = true;
                }
                if ui
                    .checkbox(&mut tilt, "turn with the pen's tilt")
                    .hint("the tip turns with the pen's tilt")
                    .changed()
                {
                    def.angle = if tilt { AngleDef::Tilt } else { AngleDef::Fixed(0.0) };
                    changed = true;
                }
                if let AngleDef::Fixed(angle) = &mut def.angle {
                    changed |= slider(ui, angle, -3.15..=3.15, "angle", "the tip's fixed angle, in radians");
                }
            });
            ui.vertical(|ui| {
                changed |= slider(
                    ui,
                    &mut def.jitter.size,
                    0.0..=1.0,
                    "size jitter",
                    "each dab's size varies by up to this much",
                );
                changed |= slider(
                    ui,
                    &mut def.jitter.angle,
                    0.0..=6.3,
                    "angle jitter",
                    "each dab turns by up to this much",
                );
                changed |= slider(
                    ui,
                    &mut def.jitter.scatter,
                    0.0..=4.0,
                    "scatter",
                    "dabs stray across the stroke by up to this many sizes",
                );
            });
            ui.separator();
            ui.label(RichText::new("pressure and tilt").strong());
            ui.vertical(|ui| {
                let mut size = curve_of(def.dynamics.size.pressure);
                let mut flow = curve_of(def.dynamics.flow.pressure);
                let mut opacity = curve_of(def.dynamics.opacity.pressure);
                let a = slider(
                    ui,
                    &mut size.min,
                    0.0..=1.0,
                    "size at the lightest press",
                    "how big a light press draws",
                );
                let b = slider(
                    ui,
                    &mut size.gamma,
                    0.3..=3.0,
                    "size curve",
                    "below 1 light presses grow sooner, above 1 later",
                );
                let c = slider(
                    ui,
                    &mut flow.min,
                    0.0..=1.0,
                    "flow at the lightest press",
                    "how much paint a light press lays",
                );
                let d = slider(
                    ui,
                    &mut opacity.min,
                    0.0..=1.0,
                    "opacity at the lightest press",
                    "how dark a light press can get",
                );
                if a || b || c || d {
                    def.dynamics.size.pressure = plain(size);
                    def.dynamics.flow.pressure = plain(flow);
                    def.dynamics.opacity.pressure = plain(opacity);
                    changed = true;
                }
                let mut tilt_size = curve_of(def.dynamics.size.tilt);
                let mut tilt_opacity = curve_of(def.dynamics.opacity.tilt);
                let e = slider(
                    ui,
                    &mut tilt_size.max,
                    0.2..=4.0,
                    "size at full tilt",
                    "a tilted pen paints this much bigger",
                );
                let f = slider(
                    ui,
                    &mut tilt_opacity.max,
                    0.0..=1.0,
                    "opacity at full tilt",
                    "a tilted pen paints this light",
                );
                if e || f {
                    def.dynamics.size.tilt = plain(tilt_size);
                    def.dynamics.opacity.tilt = plain(tilt_opacity);
                    changed = true;
                }
            });
            ui.vertical(|ui| {
                changed |= slider(ui, &mut def.flow, 0.001..=1.0, "flow", "paint per dab");
                changed |= slider(
                    ui,
                    &mut def.opacity,
                    0.001..=1.0,
                    "opacity",
                    "the most one stroke can cover",
                );
                changed |= slider(
                    ui,
                    &mut def.taper[0],
                    0.0..=200.0,
                    "taper in px",
                    "the start comes to full size over this length",
                );
                changed |= slider(
                    ui,
                    &mut def.taper[1],
                    0.0..=200.0,
                    "taper out px",
                    "the end comes to a point over this length",
                );
                changed |= slider(
                    ui,
                    &mut def.stabilizer,
                    0.0..=200.0,
                    "stabiliser ms",
                    "the line follows the pen's average over this many ms",
                );
                changed |= slider(
                    ui,
                    &mut def.smoothing,
                    0.0..=100.0,
                    "pressure smoothing ms",
                    "evens out pressure over this many ms",
                );
            });
            ui.separator();
            if let Some(d) = def.dual.as_mut() {
                ui.vertical(|ui| {
                    changed |= tip_choice(ui, "dual tip", &mut d.tip, &pictures, "the second tip");
                    changed |= slider(ui, &mut d.size, 0.05..=2.0, "its size", "as a part of the main tip");
                    changed |= slider(ui, &mut d.scatter, 0.0..=4.0, "its scatter", "how far its stamps stray");
                    let mut count = d.count as f32;
                    if slider(
                        ui,
                        &mut count,
                        1.0..=8.0,
                        "stamps per dab",
                        "more stamps fill the mark in",
                    ) {
                        d.count = count.round() as u32;
                        changed = true;
                    }
                });
            }
            if let Some(g) = def.grain.as_mut() {
                ui.vertical(|ui| {
                    let current = g.image.clone().unwrap_or_else(|| "paper".into());
                    egui::ComboBox::from_id_salt("brush texture")
                        .selected_text(&current)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(g.image.is_none(), "paper").clicked() && g.image.is_some() {
                                g.image = None;
                                g.paper = Some(Paper { seed: 23, feature: 4 });
                                changed = true;
                            }
                            pictures.iter().for_each(|name| {
                                if ui.selectable_label(g.image.as_deref() == Some(name), name).clicked()
                                    && g.image.as_deref() != Some(name)
                                {
                                    g.image = Some(name.clone());
                                    g.paper = None;
                                    changed = true;
                                }
                            });
                        })
                        .response
                        .hint("the texture: paper noise or a picture");
                    if let Some(paper) = g.paper.as_mut() {
                        let mut feature = paper.feature as f32;
                        if slider(ui, &mut feature, 1.0..=64.0, "grain size", "how coarse the paper is") {
                            paper.feature = feature.round() as usize;
                            changed = true;
                        }
                    }
                    changed |= slider(
                        ui,
                        &mut g.strength,
                        0.0..=1.0,
                        "strength",
                        "how much the texture takes away",
                    );
                    changed |= slider(ui, &mut g.scale, 0.05..=20.0, "scale", "the texture's size");
                    changed |= slider(
                        ui,
                        &mut g.contrast,
                        0.0..=8.0,
                        "contrast",
                        "harder or softer texture edges",
                    );
                    changed |= slider(
                        ui,
                        &mut g.pressure,
                        0.0..=1.0,
                        "pressed in",
                        "a hard press fills the texture in by this much",
                    );
                });
            }
            if let Some(c) = def.color.as_mut() {
                ui.vertical(|ui| {
                    changed |= slider(
                        ui,
                        &mut c.hue,
                        0.0..=180.0,
                        "hue per dab",
                        "degrees each dab's hue can move",
                    );
                    changed |= slider(
                        ui,
                        &mut c.saturation,
                        0.0..=1.0,
                        "saturation per dab",
                        "how far each dab's saturation can move",
                    );
                    changed |= slider(
                        ui,
                        &mut c.value,
                        0.0..=1.0,
                        "value per dab",
                        "how far each dab's lightness can move",
                    );
                    changed |= slider(
                        ui,
                        &mut c.stroke_hue,
                        0.0..=180.0,
                        "hue per stroke",
                        "degrees each stroke's hue can move",
                    );
                    changed |= slider(
                        ui,
                        &mut c.stroke_saturation,
                        0.0..=1.0,
                        "saturation per stroke",
                        "per stroke",
                    );
                    changed |= slider(ui, &mut c.stroke_value, 0.0..=1.0, "value per stroke", "per stroke");
                    let mut by_pressure = c.pressure.is_some();
                    if ui
                        .checkbox(&mut by_pressure, "second colour at light pressure")
                        .hint("a light press paints the second colour, a full press the first")
                        .changed()
                    {
                        c.pressure = by_pressure.then_some(Curve {
                            min: 0.0,
                            max: 1.0,
                            gamma: 1.0,
                        });
                        changed = true;
                    }
                    changed |= slider(
                        ui,
                        &mut c.along,
                        0.0..=2000.0,
                        "to the second colour over px",
                        "the colour turns into the second over this length (0: off)",
                    );
                });
            }
            ui.vertical(|ui| match def.paint.as_mut() {
                Some(PaintDef::Wet {
                    wetness,
                    pickup,
                    dilution,
                }) => {
                    changed |= slider(
                        ui,
                        wetness,
                        0.0..=1.0,
                        "wetness",
                        "how much of the picked-up colour it lays",
                    );
                    changed |= slider(
                        ui,
                        pickup,
                        0.0..=1.0,
                        "pickup",
                        "how fast it takes on the colour under it",
                    );
                    changed |= slider(ui, dilution, 0.0..=1.0, "dilution", "thinner, lighter paint");
                }
                Some(PaintDef::Smudge { strength } | PaintDef::Blend { strength }) => {
                    changed |= slider(ui, strength, 0.0..=1.0, "strength", "how strongly it moves the paint");
                }
                None => {}
            });
            ui.separator();
            ui.vertical(|ui| {
                let label = ui.label("picture from a file");
                ui.add(egui::TextEdit::singleline(&mut self.brush_window.picture_path).desired_width(180.0))
                    .labelled_by(label.id)
                    .hint("a PNG for a tip or a texture: dark is paint, or its alpha; or drop the file on this window");
                if ui
                    .button("add the picture")
                    .hint("copy the PNG into this drawing's brushes/images")
                    .clicked()
                {
                    let path = std::path::PathBuf::from(self.brush_window.picture_path.trim());
                    self.check(|e| e.add_picture_from(&path));
                }
            });
        });
        if changed {
            def.validate()?;
            self.custom = Some(def);
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("name");
            ui.add(egui::TextEdit::singleline(&mut self.brush_window.name).desired_width(140.0))
                .labelled_by(label.id)
                .hint("the brush's name; saving keeps it in this drawing");
            if ui
                .button("save in this drawing")
                .hint("a brush of this drawing, in the brush tiles; its id comes from the name")
                .clicked()
            {
                self.check(|e| e.save_brush());
            }
            if ui
                .add_enabled(self.custom.is_some(), egui::Button::new("back to the brush"))
                .hint("drop the edits and draw with the brush as it was")
                .clicked()
            {
                self.custom = None;
                self.brush_window.preview = None;
            }
        });
        if self.custom.is_some() {
            ui.label(RichText::new("you draw with these settings now; save them to keep them").color(theme::WEAK));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_names_become_ids() {
        assert_eq!(slug("My Wet  Oil!"), "my-wet-oil");
        assert_eq!(slug("  "), "");
    }
}
