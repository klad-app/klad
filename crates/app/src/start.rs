use crate::picker;
use crate::settings::Settings;
use crate::theme::{self, icon};
use anyhow::{Result, anyhow, bail};
use egui::{RichText, TextureHandle, vec2};
use klad_core::bundle::{Blank, Bundle};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, SystemTime};

pub const PRESETS: [(&str, u32, u32); 5] = [
    ("square", 2048, 2048),
    ("screen", 1920, 1080),
    ("4K screen", 3840, 2160),
    ("sprite", 1024, 1024),
    ("large", 4096, 4096),
];

const THUMB: f32 = 168.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    New,
    Open,
    Recent,
}

enum Card {
    Open(PathBuf),
    Forget,
    Idle,
}

enum Thumb {
    Waiting(Receiver<Option<egui::ColorImage>>),
    Ready(TextureHandle),
    Missing,
}

pub struct Start {
    pub name: String,
    pub folder: String,
    pub preset: Option<usize>,
    pub width: u32,
    pub height: u32,
    pub paper: bool,
    pub colour: [u8; 3],
    pub fps: u32,
    pub sketch: bool,
    pub path: String,
    pub error: Option<String>,
    pub section: Section,
    pub changed: bool,
    focused: bool,
    thumbs: BTreeMap<String, Thumb>,
}

impl Start {
    pub fn new(settings: &Settings, section: Section) -> Self {
        let blank = Blank::default();
        let folder = settings.drawings_folder();
        Self {
            name: free_name(&folder),
            folder: folder.display().to_string(),
            preset: Some(0),
            width: blank.width,
            height: blank.height,
            paper: blank.background.is_some(),
            colour: [244, 239, 230],
            fps: blank.fps,
            sketch: true,
            path: String::new(),
            error: None,
            section,
            changed: false,
            focused: false,
            thumbs: BTreeMap::new(),
        }
    }

    pub fn blank(&self) -> Blank {
        let (width, height) = self
            .preset
            .map_or((self.width, self.height), |i| (PRESETS[i].1, PRESETS[i].2));
        let [r, g, b] = self.colour;
        Blank {
            width,
            height,
            background: self.paper.then(|| format!("#{r:02x}{g:02x}{b:02x}")),
            fps: self.fps,
            sketch: self.sketch,
            ..Blank::default()
        }
    }

    pub fn target(&self) -> PathBuf {
        let name = self.name.trim();
        let file = match name.ends_with(".klad") {
            true => name.to_string(),
            false => format!("{name}.klad"),
        };
        expand(&self.folder).join(file)
    }

    pub fn create(&self, settings: &mut Settings) -> Result<PathBuf> {
        let name = self.name.trim();
        if name.is_empty() {
            bail!("give the drawing a name");
        }
        if name.contains('/') {
            bail!("a name cannot hold a /: pick the folder in the field below it");
        }
        let path = self.target();
        if path.exists() {
            bail!("{} already exists: pick another name", path.display());
        }
        std::fs::create_dir_all(expand(&self.folder))?;
        Bundle::blank(&path, &self.blank(), "editor")?;
        if expand(&self.folder) != settings.drawings_folder() {
            settings.drawings = Some(expand(&self.folder).display().to_string());
        }
        Ok(path)
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, settings: &mut Settings) -> Option<PathBuf> {
        let mut chosen = None;
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(420.0);
                        chosen = chosen.take().or_else(|| self.new_section(ui, settings));
                    });
                });
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(290.0);
                        chosen = chosen.take().or_else(|| self.open_section(ui, settings));
                    });
                });
            });
            if let Some(error) = &self.error {
                ui.add_space(4.0);
                ui.colored_label(theme::ERROR, error);
            }
            ui.add_space(10.0);
            chosen = chosen.take().or_else(|| self.recent_section(ui, settings));
        });
        self.focused = true;
        chosen
    }

    fn new_section(&mut self, ui: &mut egui::Ui, settings: &mut Settings) -> Option<PathBuf> {
        ui.label(
            RichText::new(format!("{} New drawing", icon::FILE_PLUS))
                .strong()
                .size(16.0),
        );
        ui.add_space(6.0);
        let mut entered = false;
        egui::Grid::new("start new")
            .num_columns(2)
            .spacing(vec2(10.0, 8.0))
            .show(ui, |ui| {
                let label = ui.label("Name");
                let name = ui
                    .add(egui::TextEdit::singleline(&mut self.name).desired_width(260.0))
                    .labelled_by(label.id);
                if !self.focused && self.section == Section::New {
                    name.request_focus();
                }
                entered = name.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.end_row();
                let label = ui.label("Folder");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.folder).desired_width(200.0))
                        .labelled_by(label.id);
                    if picker::NATIVE
                        && ui.button("Choose").clicked()
                        && let Some(dir) = picker::choose_folder(Some(&expand(&self.folder)), "Choose")
                    {
                        self.folder = dir.display().to_string();
                    }
                });
                ui.end_row();
                let label = ui.label("Size");
                let shown = self.preset.map_or("custom".to_string(), preset_name);
                egui::ComboBox::from_id_salt("start size")
                    .selected_text(shown)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        (0..PRESETS.len()).for_each(|i| {
                            ui.selectable_value(&mut self.preset, Some(i), preset_name(i));
                        });
                        ui.selectable_value(&mut self.preset, None, "custom");
                    })
                    .response
                    .labelled_by(label.id);
                ui.end_row();
                if self.preset.is_none() {
                    ui.label("Width and height");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut self.width).range(1..=16384).suffix(" px"));
                        ui.label("by");
                        ui.add(egui::DragValue::new(&mut self.height).range(1..=16384).suffix(" px"));
                    });
                    ui.end_row();
                }
                ui.label("Background");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.paper, "Colour");
                    match self.paper {
                        true => {
                            egui::color_picker::color_edit_button_srgb(ui, &mut self.colour);
                        }
                        false => {
                            ui.label(RichText::new("transparent").color(theme::WEAK));
                        }
                    }
                });
                ui.end_row();
                let label = ui.label("Frames per second");
                ui.add(egui::DragValue::new(&mut self.fps).range(1..=120))
                    .labelled_by(label.id);
                ui.end_row();
            });
        ui.checkbox(&mut self.sketch, "A sketch layer for blocking in shapes");
        ui.add_space(6.0);
        if !(ui.add(egui::Button::new("Create").min_size(vec2(96.0, 28.0))).clicked() || entered) {
            return None;
        }
        match self.create(settings) {
            Ok(path) => {
                self.changed = true;
                Some(path)
            }
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                None
            }
        }
    }

    fn open_section(&mut self, ui: &mut egui::Ui, settings: &Settings) -> Option<PathBuf> {
        ui.label(
            RichText::new(format!("{} Open a drawing", icon::FOLDER_OPEN))
                .strong()
                .size(16.0),
        );
        ui.add_space(6.0);
        let mut picked = None;
        if picker::NATIVE
            && ui
                .add(egui::Button::new("Choose a .klad folder...").min_size(vec2(200.0, 28.0)))
                .clicked()
        {
            picked = picker::choose_folder(Some(&settings.drawings_folder()), "Open");
        }
        ui.add_space(6.0);
        let label = ui.label(RichText::new("Or paste its path").color(theme::WEAK));
        ui.horizontal(|ui| {
            let field = ui
                .add(egui::TextEdit::singleline(&mut self.path).desired_width(220.0))
                .labelled_by(label.id);
            if !self.focused && self.section == Section::Open {
                field.request_focus();
            }
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Open").clicked() || entered {
                picked = Some(expand(self.path.trim()));
            }
        });
        let path = picked?;
        match openable(&path) {
            Ok(()) => Some(path),
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                None
            }
        }
    }

    fn recent_section(&mut self, ui: &mut egui::Ui, settings: &mut Settings) -> Option<PathBuf> {
        ui.label(RichText::new(format!("{} Recent", icon::HISTORY)).strong().size(16.0));
        ui.add_space(6.0);
        if settings.documents.is_empty() {
            ui.label(RichText::new("Drawings you open show up here.").color(theme::WEAK));
            return None;
        }
        let mut chosen = None;
        let documents = settings.documents.clone();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
            documents.iter().for_each(|doc| {
                ui.vertical(|ui| {
                    ui.set_width(THUMB + 8.0);
                    match self.card(ui, doc) {
                        Card::Open(path) => chosen = Some(path),
                        Card::Forget => {
                            settings.forget(doc);
                            self.changed = true;
                        }
                        Card::Idle => {}
                    }
                });
            });
        });
        chosen
    }

    fn card(&mut self, ui: &mut egui::Ui, doc: &str) -> Card {
        let path = PathBuf::from(doc);
        let there = path.join("doc.json").is_file();
        let name = path
            .file_stem()
            .map_or_else(|| doc.to_string(), |s| s.to_string_lossy().to_string());
        let (rect, response) = ui.allocate_exact_size(vec2(THUMB + 8.0, THUMB + 32.0), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, there, &name));
        let look = ui.style().interact(&response);
        let fill = match there {
            true => look.weak_bg_fill,
            false => ui.visuals().faint_bg_color,
        };
        ui.painter()
            .rect(rect, 6.0, fill, look.bg_stroke, egui::StrokeKind::Inside);
        let picture = egui::Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(THUMB, THUMB));
        if let Some(texture) = there.then(|| self.thumb(ui.ctx(), doc)).flatten() {
            egui::Image::new(&texture).paint_at(
                ui,
                egui::Rect::from_center_size(picture.center(), fit(texture.size_vec2())),
            );
        }
        let colour = match there {
            true => look.text_color(),
            false => theme::WEAK,
        };
        let galley = egui::WidgetText::from(name.as_str()).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            THUMB - 8.0,
            egui::TextStyle::Body,
        );
        ui.painter()
            .galley(egui::pos2(rect.min.x + 8.0, rect.max.y - 24.0), galley, colour);
        let folder = path
            .parent()
            .and_then(Path::file_name)
            .map_or_else(String::new, |s| s.to_string_lossy().to_string());
        let clicked = there && response.on_hover_text(doc).clicked();
        ui.add(egui::Label::new(RichText::new(format!("in {folder}")).color(theme::WEAK).size(11.0)).truncate());
        if clicked {
            return Card::Open(path);
        }
        if !there {
            ui.label(RichText::new("moved or deleted").color(theme::REFUSED));
            if ui.button(format!("Remove {name}")).clicked() {
                return Card::Forget;
            }
        }
        Card::Idle
    }

    fn thumb(&mut self, ctx: &egui::Context, doc: &str) -> Option<TextureHandle> {
        let state = self.thumbs.entry(doc.to_string()).or_insert_with(|| {
            let (tx, rx) = channel();
            let path = PathBuf::from(doc);
            let repaint = ctx.clone();
            std::thread::spawn(move || {
                let image = thumbnail(&path);
                let _ = tx.send(image);
                repaint.request_repaint();
            });
            Thumb::Waiting(rx)
        });
        if let Thumb::Waiting(rx) = state {
            *state = match rx.try_recv() {
                Ok(Some(image)) => {
                    Thumb::Ready(ctx.load_texture(format!("thumb {doc}"), image, egui::TextureOptions::LINEAR))
                }
                Ok(None) | Err(TryRecvError::Disconnected) => Thumb::Missing,
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(200));
                    return None;
                }
            };
        }
        match state {
            Thumb::Ready(texture) => Some(texture.clone()),
            _ => None,
        }
    }
}

fn preset_name(i: usize) -> String {
    let (name, w, h) = PRESETS[i];
    format!("{w} x {h}  {name}")
}

fn fit(size: egui::Vec2) -> egui::Vec2 {
    size * (THUMB / size.x.max(size.y).max(1.0))
}

pub fn expand(text: &str) -> PathBuf {
    match (text.strip_prefix("~/"), crate::places::home()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(text),
    }
}

pub fn openable(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        bail!("paste the path of a .klad folder");
    }
    if !path.is_dir() {
        bail!("{} is not a folder", path.display());
    }
    if !path.join("doc.json").is_file() {
        bail!("{} is not a drawing: it has no doc.json", path.display());
    }
    Ok(())
}

pub fn free_name(folder: &Path) -> String {
    (1..)
        .map(|i| match i {
            1 => "untitled".to_string(),
            n => format!("untitled {n}"),
        })
        .find(|name| !folder.join(format!("{name}.klad")).exists())
        .unwrap_or_default()
}

pub(crate) fn draw_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if exe.file_stem().is_some_and(|s| s == "klad") {
        return Some(exe);
    }
    let name = format!("klad{}", std::env::consts::EXE_SUFFIX);
    [exe.parent()?.join(&name), exe.parent()?.parent()?.join(&name)]
        .into_iter()
        .find(|p| p.is_file())
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn thumbnail(bundle: &Path) -> Option<egui::ColorImage> {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let png = bundle.join(".cache").join("thumb.png");
    let fresh = matches!((modified(&png), modified(&bundle.join("doc.json"))), (Some(t), Some(d)) if t >= d);
    if !fresh {
        let _turn = ONE_AT_A_TIME.lock().ok()?;
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(bundle.join("doc.json")).ok()?).ok()?;
        let side = ["width", "height"]
            .iter()
            .filter_map(|k| doc["canvas"][k].as_f64())
            .fold(1.0, f64::max);
        let done = std::process::Command::new(draw_binary()?)
            .env("RAYON_NUM_THREADS", "2")
            .arg("render")
            .arg(bundle)
            .arg("--scale")
            .arg(format!("{:.4}", (2.0 * THUMB as f64 / side).min(1.0)))
            .arg("-o")
            .arg(&png)
            .output()
            .ok()?;
        if !done.status.success() {
            return None;
        }
    }
    decode(&std::fs::read(&png).ok()?).ok()
}

fn decode(bytes: &[u8]) -> Result<egui::ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or_else(|| anyhow!("a png too large"))?];
    let info = reader.next_frame(&mut buffer)?;
    let pixels = &buffer[..info.buffer_size()];
    let size = [info.width as usize, info.height as usize];
    Ok(match info.color_type {
        png::ColorType::Rgba => egui::ColorImage::from_rgba_unmultiplied(size, pixels),
        png::ColorType::Rgb => egui::ColorImage::from_rgb(size, pixels),
        other => bail!("thumbnails are rgb or rgba pngs, not {other:?}"),
    })
}

pub fn title(path: Option<&Path>) -> String {
    match path {
        Some(path) => format!(
            "{}  klad",
            path.file_stem()
                .map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().to_string())
        ),
        None => "klad".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thumbnail_is_rendered_once_and_again_when_the_drawing_changes() {
        assert!(draw_binary().is_some(), "build the klad binary first (just test does)");
        let dir = std::env::temp_dir().join(format!("draw-thumb-{}.klad", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let blank = Blank {
            width: 400,
            height: 200,
            background: Some("#336699".into()),
            ..Blank::default()
        };
        Bundle::blank(&dir, &blank, "cli").unwrap();
        let image = thumbnail(&dir).unwrap();
        assert_eq!(image.size, [336, 168]);
        let middle = image.pixels[84 * 336 + 168];
        assert!(
            middle.r().abs_diff(0x33) <= 2 && middle.b().abs_diff(0x99) <= 2,
            "{middle:?}"
        );
        let png = dir.join(".cache").join("thumb.png");
        let first = modified(&png).unwrap();
        thumbnail(&dir).unwrap();
        assert_eq!(modified(&png), Some(first));
        std::thread::sleep(Duration::from_millis(20));
        Bundle::open(&dir).unwrap().save().unwrap();
        thumbnail(&dir).unwrap();
        assert!(modified(&png).unwrap() > first);
        assert_eq!(free_name(dir.parent().unwrap()), "untitled");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn names_paths_and_sizes_come_out_as_typed() {
        let settings = Settings {
            drawings: Some("/art".into()),
            ..Settings::default()
        };
        let mut start = Start::new(&settings, Section::New);
        start.name = "  cave  ".into();
        assert_eq!(start.target(), PathBuf::from("/art/cave.klad"));
        start.name = "cave.klad".into();
        assert_eq!(start.target(), PathBuf::from("/art/cave.klad"));
        assert_eq!(
            (start.blank().width, start.blank().height, start.blank().background),
            (2048, 2048, None)
        );
        start.preset = None;
        (start.width, start.height, start.paper) = (640, 360, true);
        assert_eq!(
            (start.blank().width, start.blank().height, start.blank().background),
            (640, 360, Some("#f4efe6".to_string()))
        );
        start.name = "a/b".into();
        assert!(format!("{:#}", start.create(&mut Settings::default()).unwrap_err()).contains("cannot hold a /"));
        assert!(format!("{:#}", openable(Path::new("/nowhere")).unwrap_err()).contains("is not a folder"));
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(expand("~/art"), PathBuf::from(home).join("art"));
        }
    }
}
