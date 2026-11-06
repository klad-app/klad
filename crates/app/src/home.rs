use crate::commands::{Bindings, Cmd};
use crate::editor::{Editor, Want};
use crate::gpu::CanvasGpu;
use crate::menus::{self, Picked};
use crate::messages::Tone;
use crate::picker;
use crate::settings::{self, Settings};
use crate::start::{self, Section, Start};
use crate::theme;
use crate::updates;
use anyhow::{Result, anyhow};
use egui::RichText;
use egui_wgpu::wgpu;
use std::path::{Path, PathBuf};

const WIDTH: f32 = 760.0;

pub fn run(path: Option<PathBuf>) -> Result<()> {
    crate::opened::listen();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(start::title(path.as_deref()))
            .with_inner_size([1600.0, 1000.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            surface: egui_wgpu::SurfaceConfig {
                present_mode: wgpu::PresentMode::AutoVsync,
                desired_maximum_frame_latency: Some(1),
            },
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "klad",
        options,
        Box::new(move |cc| Ok(Box::new(Home::new(cc, path.as_deref())?))),
    )
    .map_err(|e| anyhow!("{e}"))
}

pub(crate) struct Home {
    pub(crate) editor: Option<Box<Editor>>,
    pub(crate) start: Option<Start>,
    pub(crate) settings: Settings,
    pub(crate) settings_path: Option<PathBuf>,
    gpu: Option<egui_wgpu::RenderState>,
    themed: bool,
    pub(crate) update: Option<crate::updates::Release>,
    checking: Option<std::sync::mpsc::Receiver<updates::Checked>>,
    native_menus: bool,
    menus_built: Option<u64>,
    menus_shown: Option<std::time::Instant>,
}

impl Home {
    fn new(cc: &eframe::CreationContext, path: Option<&Path>) -> Result<Self> {
        let gpu = cc
            .wgpu_render_state
            .clone()
            .ok_or_else(|| anyhow!("the wgpu renderer is not available"))?;
        let settings_path = settings::default_path();
        let settings = settings_path.as_deref().map(Settings::load).unwrap_or_default();
        let mut home = Self::with(settings, settings_path, Some(gpu));
        home.native_menus = true;
        if updates::due(
            home.settings.check_updates,
            home.settings.updates_checked,
            updates::now(),
        ) {
            home.checking = Some(updates::spawn(updates::feed()));
        }
        home.begin(&cc.egui_ctx, path);
        Ok(home)
    }

    pub(crate) fn with(
        settings: Settings,
        settings_path: Option<PathBuf>,
        gpu: Option<egui_wgpu::RenderState>,
    ) -> Self {
        Self {
            editor: None,
            start: None,
            settings,
            settings_path,
            gpu,
            themed: false,
            update: None,
            checking: None,
            native_menus: false,
            menus_built: None,
            menus_shown: None,
        }
    }

    pub(crate) fn begin(&mut self, ctx: &egui::Context, path: Option<&Path>) {
        let Some(path) = path else {
            self.start = Some(Start::new(&self.settings, Section::New));
            return;
        };
        if let Err(e) = self.open(ctx, path) {
            let mut start = Start::new(&self.settings, Section::Open);
            start.path = path.display().to_string();
            start.error = Some(format!("opening {}: {e:#}", path.display()));
            self.start = Some(start);
        }
    }

    pub(crate) fn open(&mut self, ctx: &egui::Context, path: &Path) -> Result<()> {
        let mut settings = self
            .editor
            .as_ref()
            .map_or_else(|| self.settings.clone(), |e| e.settings.clone());
        let mut editor = Editor::open(ctx.clone(), path)?;
        settings.opened(path);
        editor.settings = settings.clone();
        editor.settings_path = self.settings_path.clone();
        editor.settings_dirty = true;
        if let Some(gpu) = &self.gpu {
            gpu.renderer.write().callback_resources.insert(CanvasGpu::new(
                &gpu.device,
                gpu.target_format,
                editor.scene.size(),
            ));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(start::title(Some(path))));
        self.settings = settings;
        self.editor = Some(Box::new(editor));
        self.start = None;
        Ok(())
    }

    fn checked(&mut self) {
        let Some(result) = self.checking.as_ref().and_then(|c| c.try_recv().ok()) else {
            return;
        };
        self.checking = None;
        let now = updates::now();
        self.settings.updates_checked = now;
        match self.editor.as_mut() {
            Some(editor) => {
                editor.settings.updates_checked = now;
                editor.settings_dirty = true;
            }
            None => {
                if let Some(path) = &self.settings_path {
                    let _ = self.settings.save(path);
                }
            }
        }
        if let Ok(Some(release)) = result {
            if let Some(editor) = self.editor.as_mut() {
                editor.say(Tone::Info, updates::summary(&release));
            }
            self.update = Some(release);
        }
    }

    fn tend_menus(&mut self, ctx: &egui::Context) {
        if !self.native_menus {
            return;
        }
        let settings = self.editor.as_ref().map_or(&self.settings, |e| &e.settings);
        let built = menus::signature(&settings.keys, &settings.documents);
        if self.menus_built != Some(built) {
            menus::install(ctx, &Bindings::from_settings(&settings.keys), &settings.documents);
            self.menus_built = Some(built);
        }
        if self
            .menus_shown
            .is_none_or(|t| t.elapsed() > std::time::Duration::from_millis(400))
        {
            menus::show(
                self.editor
                    .as_ref()
                    .map_or_else(menus::outside_a_drawing, |e| e.menu_state()),
            );
            self.menus_shown = Some(std::time::Instant::now());
        }
    }

    fn picked(&mut self, ctx: &egui::Context) -> Option<PathBuf> {
        menus::take()
            .into_iter()
            .fold(None, |path, picked| match (picked, self.editor.as_mut()) {
                (Picked::Open(p), _) => Some(p),
                (Picked::Run(cmd), Some(editor)) => {
                    editor.run_later(cmd);
                    path
                }
                (Picked::Run(Cmd::NewDocument), None) => {
                    self.start = Some(Start::new(&self.settings, Section::New));
                    path
                }
                (Picked::Run(Cmd::OpenRecent), None) => {
                    self.start = Some(Start::new(&self.settings, Section::Recent));
                    path
                }
                (Picked::Run(Cmd::OpenDocument), None) if picker::NATIVE => {
                    let folder = self.settings.drawings_folder();
                    picker::choose_folder(Some(&folder), "Open").or(path)
                }
                (Picked::Run(Cmd::OpenDocument), None) => {
                    self.start = Some(Start::new(&self.settings, Section::Open));
                    path
                }
                (Picked::Run(Cmd::Quit), None) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    path
                }
                (Picked::Run(_), None) => path,
            })
    }

    fn close(&mut self, ctx: &egui::Context) {
        let Some(editor) = self.editor.take() else {
            return;
        };
        self.settings = editor.settings.clone();
        if let Some(path) = &self.settings_path
            && let Err(e) = self.settings.save(path)
        {
            let mut start = Start::new(&self.settings, Section::Recent);
            start.error = Some(format!("saving your settings: {e:#}"));
            self.start = Some(start);
        } else {
            self.start = Some(Start::new(&self.settings, Section::Recent));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(start::title(None)));
    }

    pub(crate) fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.tend_menus(&ctx);
        let from_menu = self.picked(&ctx);
        self.checked();
        if self.checking.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if let Some(editor) = self.editor.as_mut()
            && editor.update.is_none()
        {
            editor.update.clone_from(&self.update);
        }
        let mut chosen = None;
        let mut closing = false;
        match self.editor.as_mut() {
            Some(editor) => {
                editor.frame(ui);
                match editor.wants.take() {
                    Some(Want::Close) => closing = true,
                    Some(Want::Path(path)) => chosen = Some(path),
                    Some(Want::New) => self.start = Some(Start::new(&editor.settings, Section::New)),
                    Some(Want::Recent) => self.start = Some(Start::new(&editor.settings, Section::Recent)),
                    Some(Want::Open) if picker::NATIVE => {
                        let folder = editor.settings.drawings_folder();
                        chosen = picker::choose_folder(Some(&folder), "Open").and_then(|path| {
                            start::openable(&path)
                                .map_err(|e| editor.error(format!("{e:#}")))
                                .ok()
                                .map(|_| path)
                        });
                    }
                    Some(Want::Open) => self.start = Some(Start::new(&editor.settings, Section::Open)),
                    None => {}
                }
                if let Some(start) = self.start.as_mut() {
                    let height = (ctx.content_rect().height() - 120.0).clamp(320.0, 760.0);
                    let modal = egui::Modal::new(egui::Id::unique("start window")).show(&ctx, |ui| {
                        ui.set_width(WIDTH);
                        ui.set_height(height);
                        let back = ui
                            .horizontal(|ui| {
                                ui.heading("Drawings");
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.button("Back to this drawing").clicked()
                                })
                                .inner
                            })
                            .inner;
                        ui.add_space(8.0);
                        chosen = start.ui(ui, &mut editor.settings);
                        back
                    });
                    editor.settings_dirty |= std::mem::take(&mut start.changed);
                    if modal.inner || modal.should_close() {
                        self.start = None;
                    }
                }
            }
            None => {
                if !self.themed {
                    theme::install(&ctx);
                    self.themed = true;
                }
                if (ctx.zoom_factor() - self.settings.ui_scale).abs() > 1e-3 {
                    ctx.set_zoom_factor(self.settings.ui_scale);
                }
                let start = self
                    .start
                    .get_or_insert_with(|| Start::new(&self.settings, Section::New));
                egui::CentralPanel::default().show(ui, |ui| {
                    let margin = ((ui.available_width() - WIDTH) / 2.0).max(16.0);
                    ui.add_space(32.0);
                    ui.horizontal_top(|ui| {
                        ui.add_space(margin);
                        ui.vertical(|ui| {
                            ui.set_max_width(WIDTH);
                            ui.horizontal(|ui| {
                                ui.heading(RichText::new("klad").size(26.0));
                                ui.label(
                                    RichText::new(format!("version {}", crate::VERSION)).color(crate::theme::WEAK),
                                );
                            });
                            if let Some(release) = &self.update {
                                ui.horizontal(|ui| {
                                    ui.label(updates::summary(release));
                                    ui.hyperlink_to("what changed", &release.url);
                                });
                            }
                            ui.add_space(12.0);
                            chosen = start.ui(ui, &mut self.settings);
                        });
                    });
                });
                if std::mem::take(&mut start.changed)
                    && let Some(path) = &self.settings_path
                    && let Err(e) = self.settings.save(path)
                {
                    start.error = Some(format!("saving your settings: {e:#}"));
                }
            }
        }
        if closing {
            self.close(&ctx);
        }
        let tour = self.start.as_ref().is_some_and(|s| s.tour);
        if let Some(path) = chosen.or(from_menu).or_else(crate::opened::take)
            && let Err(e) = self.open(&ctx, &path).inspect(|_| {
                if let Some(editor) = self.editor.as_mut().filter(|_| tour) {
                    editor.start_first_steps();
                }
            })
        {
            let message = format!("opening {}: {e:#}", path.display());
            match (self.start.as_mut(), self.editor.as_mut()) {
                (Some(start), _) => start.error = Some(message),
                (None, Some(editor)) => editor.error(message),
                (None, None) => {}
            }
        }
    }
}

impl eframe::App for Home {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.editor
            .as_ref()
            .map_or_else(|| theme::surround(self.settings.defaults.surround), |e| e.clear())
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(editor) = self.editor.as_mut() {
            editor.take_tabs(ctx, raw_input);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::Role;
    use egui::{Key, Modifiers, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use klad_core::bundle::{Blank, Bundle};

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("draw-home-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(&dir).unwrap()
    }

    fn home(dir: &Path, documents: &[&Path]) -> Home {
        let settings = Settings {
            drawings: Some(dir.display().to_string()),
            documents: documents.iter().map(|p| p.display().to_string()).collect(),
            ..Settings::default()
        };
        Home::with(settings, None, None)
    }

    fn harness(home: Home) -> Harness<'static, Home> {
        let mut h = Harness::builder()
            .with_size(vec2(1280.0, 860.0))
            .build_ui_state(|ui, home: &mut Home| home.frame(ui), home);
        h.run_steps(3);
        h
    }

    fn small() -> Blank {
        Blank {
            width: 320,
            height: 200,
            ..Blank::default()
        }
    }

    fn opened(h: &Harness<'static, Home>) -> PathBuf {
        h.state()
            .editor
            .as_ref()
            .expect("an open drawing")
            .scene
            .bundle
            .root
            .clone()
    }

    #[test]
    fn the_start_window_makes_a_drawing_like_draw_new_and_opens_it() {
        let dir = folder("new");
        let mut first = home(&dir, &[]);
        first.begin(&egui::Context::default(), None);
        let mut h = harness(first);
        assert!(h.state().editor.is_none());
        assert!(h.query_by_label("Drawings you open show up here.").is_some());
        {
            let start = h.state_mut().start.as_mut().unwrap();
            assert_eq!(start.name, "untitled");
            start.name.clear();
            start.preset = None;
            (start.width, start.height) = (320, 200);
            start.paper = true;
        }
        h.get_by_role_and_label(Role::TextInput, "Name").type_text("rock study");
        h.run_steps(2);
        h.get_by_label("Create").click();
        h.run_steps(3);
        let made = dir.join("rock study.klad");
        assert_eq!(opened(&h), made);
        let cli = Bundle::blank(
            dir.join("cli.klad"),
            &Blank {
                background: Some("#f4efe6".into()),
                sketch: true,
                ..small()
            },
            "cli",
        )
        .unwrap();
        assert_eq!(Bundle::open(&made).unwrap().doc, cli.doc);
        let kinds: Vec<(String, bool)> = cli.doc.layers.iter().map(|l| (l.name.clone(), l.visible)).collect();
        assert_eq!(kinds, [("paint 1".to_string(), true), ("sketch".to_string(), true)]);
        assert_eq!(h.state().settings.documents, [made.display().to_string()]);
        let mut again = home(&dir, &[]);
        again.begin(&egui::Context::default(), None);
        assert_eq!(again.start.as_ref().unwrap().name, "untitled");
        let mut taken = Start::new(&again.settings, Section::New);
        taken.name = "rock study".into();
        let refused = format!("{:#}", taken.create(&mut again.settings).unwrap_err());
        assert!(refused.contains("already exists"), "{refused}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_drawing_the_system_asks_to_open_opens_from_the_start_window() {
        let dir = folder("finder");
        let made = dir.join("from finder.klad");
        Bundle::blank(&made, &small(), "cli").unwrap();
        let mut first = home(&dir, &[]);
        first.begin(&egui::Context::default(), None);
        let mut h = harness(first);
        assert!(h.state().editor.is_none());
        crate::opened::push(made.clone());
        h.run_steps(2);
        assert_eq!(opened(&h), made);
        assert!(crate::opened::take().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn menu_picks_open_a_drawing_run_its_commands_and_close_it() {
        let dir = folder("menus");
        let made = dir.join("ledge.klad");
        Bundle::blank(&made, &small(), "cli").unwrap();
        let mut first = home(&dir, &[]);
        first.begin(&egui::Context::default(), None);
        let mut h = harness(first);
        menus::push(Picked::Run(Cmd::Undo));
        menus::push(Picked::Run(Cmd::OpenRecent));
        h.run_steps(2);
        assert!(
            h.state().editor.is_none(),
            "outside a drawing only the file commands run"
        );
        menus::push(Picked::Open(made.clone()));
        h.run_steps(3);
        assert_eq!(opened(&h), made);
        let state = h.state().editor.as_ref().unwrap().menu_state();
        assert!(state.off.contains(&Cmd::Unlink) && !state.off.contains(&Cmd::Export));
        assert!(state.on.contains(&Cmd::Brush), "the tool in use is ticked");
        menus::push(Picked::Run(Cmd::CloseDocument));
        h.run_steps(3);
        assert!(h.state().editor.is_none(), "close goes back to the start window");
        assert!(h.state().start.is_some());
        assert_eq!(h.state().settings.documents, [made.display().to_string()]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_first_start_welcomes_offers_first_steps_and_opens_samples_as_copies() {
        let dir = folder("welcome");
        let shipped = dir.join("shipped/rock/rock.klad");
        Bundle::blank(&shipped, &small(), "cli").unwrap();
        let mut first = home(&dir, &[]);
        first.begin(&egui::Context::default(), None);
        first.start.as_mut().unwrap().samples = vec![klad_core::samples::Sample {
            name: "rock".into(),
            about: "a rock on its own".into(),
            path: shipped.clone(),
        }];
        let mut h = harness(first);
        assert!(
            h.query_by_label("Welcome to klad").is_some(),
            "a first start says hello"
        );
        assert!(
            h.query_by_label("a rock on its own").is_some(),
            "the samples show with what they are"
        );
        h.get_by_label("rock").click();
        h.run_steps(3);
        assert_eq!(
            opened(&h),
            dir.join("rock.klad"),
            "a sample opens as a copy in the drawings folder"
        );
        assert!(shipped.join("doc.json").is_file());
        assert!(h.state().settings.welcomed, "opening anything counts as welcomed");
        let mut again = home(&dir, &[]);
        again.begin(&egui::Context::default(), None);
        let mut h = harness(again);
        h.get_by_label("First steps").click();
        h.run_steps(3);
        assert_eq!(opened(&h), dir.join("first steps.klad"));
        assert!(
            h.query_by_label_contains("Draw a closed shape with the brush")
                .is_some(),
            "first steps open on the new drawing"
        );
        let mut later = home(&dir, &[]);
        later.settings.welcomed = true;
        later.begin(&egui::Context::default(), None);
        let h = harness(later);
        assert!(h.query_by_label("Welcome to klad").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_new_version_shows_in_the_start_window_with_what_changed() {
        let dir = folder("update");
        let mut first = home(&dir, &[]);
        first.begin(&egui::Context::default(), None);
        first.update = updates::parse(
            r#"{"tag_name":"v9.0.0","html_url":"https://github.com/klad-app/klad/releases/tag/v9.0.0","body":"- Paint over"}"#,
        );
        let h = harness(first);
        assert!(h.query_by_label(&format!("version {}", crate::VERSION)).is_some());
        assert!(h.query_by_label("klad 9.0.0 is out: Paint over").is_some());
        assert!(h.query_by_label("what changed").is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn recent_reopens_a_drawing_and_greys_out_a_moved_one() {
        let dir = folder("recent");
        let (a, b) = (dir.join("ledge.klad"), dir.join("cave.klad"));
        [&a, &b].iter().for_each(|p| {
            Bundle::blank(p, &small(), "cli").unwrap();
        });
        std::fs::rename(&b, dir.join("cave moved.klad")).unwrap();
        let mut first = home(&dir, &[&a, &b]);
        first.begin(&egui::Context::default(), None);
        let mut h = harness(first);
        assert!(h.query_by_label("moved or deleted").is_some());
        h.get_by_label("Remove cave").click();
        h.run_steps(2);
        assert_eq!(h.state().settings.documents, [a.display().to_string()]);
        h.get_by_label("ledge").click();
        h.run_steps(3);
        assert_eq!(opened(&h), a);
        assert!(h.state().start.is_none());
        let mut missing = home(&dir, &[]);
        missing.begin(&egui::Context::default(), Some(&dir.join("nowhere.klad")));
        assert!(missing.editor.is_none());
        let error = missing.start.as_ref().unwrap().error.clone().unwrap();
        assert!(error.contains("nowhere.klad"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cmd_n_and_cmd_o_in_the_editor_switch_drawings_and_back_keeps_this_one() {
        let dir = folder("switch");
        let (a, b) = (dir.join("first.klad"), dir.join("second.klad"));
        [&a, &b].iter().for_each(|p| {
            Bundle::blank(p, &small(), "cli").unwrap();
        });
        let mut first = home(&dir, &[]);
        first.open(&egui::Context::default(), &a).unwrap();
        let mut h = harness(first);
        h.key_press_modifiers(Modifiers::COMMAND, Key::N);
        h.run_steps(3);
        assert!(h.state().start.is_some());
        h.get_by_label("Back to this drawing").click();
        h.run_steps(3);
        assert!(h.state().start.is_none());
        assert_eq!(opened(&h), a);
        h.key_press_modifiers(Modifiers::COMMAND, Key::N);
        h.run_steps(3);
        {
            let start = h.state_mut().start.as_mut().unwrap();
            start.name = "third".into();
            start.preset = None;
            (start.width, start.height) = (320, 200);
        }
        h.run_steps(2);
        h.get_by_label("Create").click();
        h.run_steps(3);
        assert_eq!(opened(&h), dir.join("third.klad"));
        h.key_press_modifiers(Modifiers::COMMAND, Key::O);
        h.run_steps(3);
        h.get_by_role_and_label(Role::TextInput, "Or paste its path")
            .type_text(&b.display().to_string());
        h.run_steps(2);
        h.get_by_label("Open").click();
        h.run_steps(3);
        assert_eq!(opened(&h), b);
        let order: Vec<String> = h.state().settings.documents.clone();
        assert_eq!(
            order,
            [&b, &dir.join("third.klad"), &a].map(|p| p.display().to_string())
        );
        h.state_mut().editor.as_mut().unwrap().wants = None;
        h.get_by_label(theme::icon::FILE).click();
        h.run_steps(2);
        h.get_by_label(&format!("{} copy path", theme::icon::COPY)).click();
        h.run_steps(1);
        let copied = h
            .output()
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(text) if *text == b.display().to_string()));
        assert!(copied);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[ignore]
    fn start_pictures() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let out = root.join("scratch/ui");
        std::fs::create_dir_all(&out).unwrap();
        let shown: Vec<PathBuf> = [
            "samples/solids/solids.klad",
            "samples/lantern/lantern-painted.klad",
            "samples/creature/creature.klad",
        ]
        .iter()
        .map(|p| std::fs::canonicalize(root.join(p)).expect("run just samples and build the painted lantern first"))
        .collect();
        let gone = PathBuf::from("/Users/you/Drawings/old sketch.klad");
        let settings = Settings {
            drawings: Some("~/Drawings".into()),
            documents: shown
                .iter()
                .chain(std::iter::once(&gone))
                .map(|p| p.display().to_string())
                .collect(),
            ..Settings::default()
        };
        let mut first = Home::with(settings, None, None);
        first.begin(&egui::Context::default(), None);
        let mut h = Harness::builder()
            .with_size(vec2(1440.0, 900.0))
            .wgpu()
            .build_ui_state(|ui, home: &mut Home| home.frame(ui), first);
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(60) {
            h.run_steps(2);
            std::thread::sleep(std::time::Duration::from_millis(50));
            if shown.iter().all(|p| p.join(".cache/thumb.png").exists()) && started.elapsed().as_secs() > 2 {
                break;
            }
        }
        h.run_steps(6);
        h.render().unwrap().save(out.join("start-1-window.png")).unwrap();
        h.state_mut().open(&egui::Context::default(), &shown[1]).unwrap();
        h.run_steps(6);
        h.key_press_modifiers(Modifiers::COMMAND, Key::N);
        h.run_steps(8);
        h.render()
            .unwrap()
            .save(out.join("start-2-over-the-editor.png"))
            .unwrap();
    }

    #[test]
    fn the_document_menu_shows_the_licence_and_the_notices() {
        let dir = folder("about");
        let path = dir.join("about.klad");
        Bundle::blank(&path, &small(), "cli").unwrap();
        let mut first = home(&dir, &[]);
        first.open(&egui::Context::default(), &path).unwrap();
        let mut h = harness(first);
        h.get_by_label(theme::icon::FILE).click();
        h.run_steps(2);
        assert!(
            h.query_by_label(&format!("{} install the klad command", theme::icon::DOWNLOAD))
                .is_some()
        );
        assert_eq!(crate::editor::install_job().args, ["install"]);
        h.get_by_label(&format!("{} about and licences", theme::icon::INFO))
            .click();
        h.run_steps(3);
        assert!(
            h.query_by_label_contains("Source-available licence, copyright Indicode BV")
                .is_some()
        );
        assert!(h.query_by_label("# Third-party notices").is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[ignore]
    fn editor_picture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let out = root.join("scratch/ui");
        std::fs::create_dir_all(&out).unwrap();
        let source = std::fs::canonicalize(root.join("samples/lantern/lantern-painted.klad")).expect(
            "build it first: PAINTED=1 FORCE=1 bash samples/lantern/build.sh samples/lantern/lantern-painted.klad",
        );
        let dir = folder("editor-picture");
        let path = dir.join("lantern.klad");
        let copied = std::process::Command::new("cp")
            .arg("-R")
            .arg(&source)
            .arg(&path)
            .status()
            .unwrap();
        assert!(copied.success());
        let mut bundle = Bundle::open(&path).unwrap();
        bundle.doc.canvas.background = Some("#ebe6dc".into());
        bundle.save().unwrap();
        let gpu = egui_kittest::wgpu::create_render_state(
            egui_kittest::wgpu::default_wgpu_setup(),
            egui_wgpu::RendererOptions::PREDICTABLE,
        );
        let mut settings = Settings::default();
        settings.layout.timeline_height = 190.0;
        let mut first = Home::with(settings, None, Some(gpu.clone()));
        first.open(&egui::Context::default(), &path).unwrap();
        let mut h = Harness::builder()
            .with_size(vec2(1600.0, 1000.0))
            .renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(gpu))
            .build_ui_state(|ui, home: &mut Home| home.frame(ui), first);
        let started = std::time::Instant::now();
        while !h.state().editor.as_ref().unwrap().canvas_current() && started.elapsed().as_secs() < 60 {
            h.step();
            h.render().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        h.state_mut().editor.as_mut().unwrap().redraw_canvas();
        (0..6).for_each(|_| {
            h.step();
            h.render().unwrap();
        });
        h.render().unwrap().save(out.join("editor-lantern.png")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
