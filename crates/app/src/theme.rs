use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Id, Margin, Rect, Response, Shadow, Stroke,
    Style, TextStyle, WidgetText, vec2,
};
use std::sync::Arc;

pub const TARGET: f32 = 24.0;
pub const GRAB: f32 = 12.0;
pub const ROW: f32 = 28.0;
pub const SMALL: f32 = 11.0;
pub const BODY: f32 = 13.0;
pub const HEADING: f32 = 15.0;
pub const ICON: f32 = 17.0;

pub const ACCENT: Color32 = Color32::from_rgb(64, 196, 255);
pub const DONE: Color32 = Color32::from_rgb(120, 200, 140);
pub const REFUSED: Color32 = Color32::from_rgb(232, 190, 110);
pub const ERROR: Color32 = Color32::from_rgb(240, 120, 120);
pub const TEXT: Color32 = Color32::from_gray(214);
pub const WEAK: Color32 = Color32::from_gray(150);
pub const PANEL: Color32 = Color32::from_gray(34);
pub const SURROUND: [u8; 3] = [58, 58, 58];

pub mod icon {
    pub const BRUSH: &str = "\u{E6F0}";
    pub const SUN: &str = "\u{E472}";
    pub const ADJUST: &str = "\u{E434}";
    pub const SPARKLE: &str = "\u{E6A2}";
    pub const RECTANGLE: &str = "\u{E3F0}";
    pub const MOUNTAINS: &str = "\u{E7AE}";
    pub const REPEAT: &str = "\u{E3F6}";
    pub const SCATTER: &str = "\u{E1FC}";
    pub const IMAGE: &str = "\u{E2CA}";
    pub const ERASER: &str = "\u{E21E}";
    pub const FILL: &str = "\u{E392}";
    pub const SHAPES: &str = "\u{E1DC}";
    pub const STROKES: &str = "\u{E69A}";
    pub const POINTS: &str = "\u{EB00}";
    pub const PEN: &str = "\u{E3AC}";
    pub const PREPARE: &str = "\u{E2DE}";
    pub const NOTE: &str = "\u{E3E2}";
    pub const EYEDROPPER: &str = "\u{E568}";
    pub const MINUS: &str = "\u{E32A}";
    pub const PLUS: &str = "\u{E3D4}";
    pub const HAND: &str = "\u{E298}";
    pub const ZOOM: &str = "\u{E30C}";
    pub const SEARCH: &str = "\u{E30C}";
    pub const TEXT_T: &str = "\u{E48A}";
    pub const DOWNLOAD: &str = "\u{E20C}";
    pub const FIT: &str = "\u{E1D0}";
    pub const TURN: &str = "\u{E036}";
    pub const BONE: &str = "\u{E7F2}";
    pub const UNDO: &str = "\u{E08A}";
    pub const REDO: &str = "\u{E08C}";
    pub const TRASH: &str = "\u{E4A6}";
    pub const CLOSE: &str = "\u{E4F6}";
    pub const COPY: &str = "\u{E1CA}";
    pub const PASTE: &str = "\u{E198}";
    pub const SELECT_ALL: &str = "\u{E746}";
    pub const LASSO: &str = "\u{EDC6}";
    pub const PALETTE: &str = "\u{E6C8}";
    pub const FLIP_H: &str = "\u{ED6A}";
    pub const ALIGN_LEFT: &str = "\u{E50E}";
    pub const ALIGN_CENTRE_ACROSS: &str = "\u{E50A}";
    pub const ALIGN_RIGHT: &str = "\u{E510}";
    pub const ALIGN_TOP: &str = "\u{E512}";
    pub const ALIGN_CENTRE_DOWN: &str = "\u{E50C}";
    pub const ALIGN_BOTTOM: &str = "\u{E506}";
    pub const DISTRIBUTE_ACROSS: &str = "\u{E534}";
    pub const DISTRIBUTE_DOWN: &str = "\u{E536}";
    pub const FLIP_V: &str = "\u{ED6C}";
    pub const LAYERS: &str = "\u{E466}";
    pub const EYE: &str = "\u{E220}";
    pub const EYE_OFF: &str = "\u{E224}";
    pub const LOCK: &str = "\u{E2FA}";
    pub const CLIP: &str = "\u{E2D2}";
    pub const MASK: &str = "\u{E18C}";
    pub const ONION: &str = "\u{E62A}";
    pub const STEP_ON: &str = "\u{E13A}";
    pub const STEP_BACK: &str = "\u{E138}";
    pub const NEXT: &str = "\u{E5A6}";
    pub const PREVIOUS: &str = "\u{E5A4}";
    pub const NEW_DRAWING: &str = "\u{E236}";
    pub const LINK: &str = "\u{E2E2}";
    pub const UNLINK: &str = "\u{E2E4}";
    pub const INSERT: &str = "\u{E542}";
    pub const REMOVE: &str = "\u{ED4C}";
    pub const FLIP_FRAMES: &str = "\u{E0A0}";
    pub const PLAY: &str = "\u{E3D0}";
    pub const LOOP: &str = "\u{E3F6}";
    pub const SETTINGS: &str = "\u{E270}";
    pub const RETIME: &str = "\u{E492}";
    pub const INBETWEEN: &str = "\u{EA9A}";
    pub const CLIP_BAR: &str = "\u{E8C2}";
    pub const EXPORT: &str = "\u{EAF0}";
    pub const KEYBOARD: &str = "\u{E2D8}";
    pub const SIDEBAR: &str = "\u{EC24}";
    pub const INSPECTOR: &str = "\u{E434}";
    pub const HISTORY: &str = "\u{E1A0}";
    pub const TIMELINE: &str = "\u{E792}";
    pub const NUMBERS: &str = "\u{E628}";
    pub const LOG: &str = "\u{E2F0}";
    pub const UP: &str = "\u{E08E}";
    pub const DOWN: &str = "\u{E03E}";
    pub const LEFT: &str = "\u{E058}";
    pub const RIGHT: &str = "\u{E06C}";
    pub const TURN_LEFT: &str = "\u{E038}";
    pub const SWAP: &str = "\u{E83C}";
    pub const SIZE: &str = "\u{E18A}";
    pub const MORE: &str = "\u{E1FE}";
    pub const OPEN: &str = "\u{E136}";
    pub const FOLDED: &str = "\u{E13A}";
    pub const CHECK: &str = "\u{E182}";
    pub const WARNING: &str = "\u{E4E2}";
    pub const INFO: &str = "\u{E2CE}";
    pub const FILE: &str = "\u{E230}";
    pub const FILE_PLUS: &str = "\u{E236}";
    pub const FOLDER_OPEN: &str = "\u{E256}";
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "phosphor".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/Phosphor.ttf"))),
    );
    [FontFamily::Proportional, FontFamily::Monospace]
        .iter()
        .for_each(|family| {
            fonts
                .families
                .entry(family.clone())
                .or_default()
                .push("phosphor".into());
        });
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(apply);
}

pub fn apply(style: &mut Style) {
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(SMALL)),
        (TextStyle::Body, FontId::proportional(BODY)),
        (TextStyle::Button, FontId::proportional(BODY)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
        (TextStyle::Heading, FontId::proportional(HEADING)),
    ]
    .into();
    let s = &mut style.spacing;
    s.item_spacing = vec2(6.0, 5.0);
    s.button_padding = vec2(8.0, 4.0);
    s.interact_size = vec2(36.0, TARGET);
    s.icon_width = 16.0;
    s.icon_width_inner = 9.0;
    s.icon_spacing = 6.0;
    s.slider_width = 120.0;
    s.combo_width = 140.0;
    s.window_margin = Margin::same(10);
    s.menu_margin = Margin::same(6);
    style.interaction.tooltip_delay = 0.35;
    style.interaction.show_tooltips_only_when_still = true;
    let v = &mut style.visuals;
    v.dark_mode = true;
    v.panel_fill = PANEL;
    v.window_fill = Color32::from_gray(38);
    v.window_stroke = Stroke::new(1.0, Color32::from_gray(60));
    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(5);
    v.window_shadow = Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(90),
    };
    v.popup_shadow = v.window_shadow;
    v.extreme_bg_color = Color32::from_gray(22);
    v.faint_bg_color = Color32::from_gray(40);
    v.code_bg_color = Color32::from_gray(30);
    v.selection.bg_fill = Color32::from_rgb(38, 96, 128);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = REFUSED;
    v.error_fg_color = ERROR;
    v.override_text_color = None;
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_gray(52));
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.bg_fill = Color32::from_gray(52);
    w.inactive.weak_bg_fill = Color32::from_gray(46);
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.hovered.bg_fill = Color32::from_gray(64);
    w.hovered.weak_bg_fill = Color32::from_gray(60);
    w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_gray(96));
    w.hovered.fg_stroke = Stroke::new(1.5, Color32::from_gray(236));
    w.active.bg_fill = Color32::from_gray(74);
    w.active.weak_bg_fill = Color32::from_gray(70);
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(2.0, Color32::WHITE);
    w.open.bg_fill = Color32::from_gray(48);
    w.open.weak_bg_fill = Color32::from_gray(48);
    [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ]
    .into_iter()
    .for_each(|w| {
        w.corner_radius = CornerRadius::same(4);
        w.expansion = 0.0;
    });
}

pub fn surround(rgb: [u8; 3]) -> [f32; 4] {
    let [r, g, b] = rgb.map(|c| c as f32 / 255.0);
    [r, g, b, 1.0]
}

fn hinted_id() -> Id {
    Id::unique("klad hinted controls")
}

pub fn clear_hints(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp::<Vec<Rect>>(hinted_id(), Vec::new()));
}

#[cfg(test)]
pub fn hinted(ctx: &egui::Context) -> Vec<Rect> {
    ctx.data(|d| d.get_temp::<Vec<Rect>>(hinted_id()).unwrap_or_default())
}

pub trait Hint {
    fn hint(self, text: impl Into<WidgetText>) -> Response;
}

impl Hint for Response {
    fn hint(self, text: impl Into<WidgetText>) -> Response {
        let rect = self.rect;
        self.ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Vec<Rect>>(hinted_id()).push(rect);
        });
        self.on_hover_text(text)
    }
}
