use egui::Vec2;
use serde::{Deserialize, Serialize};

pub const STRIP: f32 = 44.0;
pub const STATUS: f32 = 28.0;
pub const TITLE: f32 = 30.0;
pub const DOCK_MIN: f32 = 260.0;
pub const DOCK_DEFAULT: f32 = 330.0;
pub const TIMELINE_MIN: f32 = 150.0;
pub const TIMELINE_DEFAULT: f32 = 330.0;
pub const TIMELINE_FOLDED: f32 = 64.0;
pub const CANVAS_MIN: Vec2 = Vec2::new(320.0, 200.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pane {
    Tool,
    Layers,
    Inspector,
    Notes,
    History,
}

impl Pane {
    pub fn title(self) -> &'static str {
        match self {
            Pane::Tool => "Colour and brush",
            Pane::Layers => "Layers",
            Pane::Inspector => "Layer settings",
            Pane::Notes => "Notes for the agent",
            Pane::History => "History",
        }
    }

    pub fn min_body(self) -> f32 {
        match self {
            Pane::Tool => 160.0,
            Pane::Layers => 130.0,
            Pane::Inspector => 110.0,
            Pane::Notes => 60.0,
            Pane::History => 60.0,
        }
    }

    fn default_body(self) -> f32 {
        match self {
            Pane::Tool => 360.0,
            Pane::Layers => 240.0,
            Pane::Inspector => 220.0,
            Pane::Notes => 90.0,
            Pane::History => 80.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneState {
    pub pane: Pane,
    pub open: bool,
    pub collapsed: bool,
    pub body: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Layout {
    pub strip: bool,
    pub dock: bool,
    pub dock_width: f32,
    pub timeline: bool,
    pub timeline_height: f32,
    pub timeline_folded: bool,
    pub numbers: bool,
    pub panes: Vec<PaneState>,
    #[serde(skip)]
    pub hidden: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            strip: true,
            dock: true,
            dock_width: DOCK_DEFAULT,
            timeline: true,
            timeline_height: TIMELINE_DEFAULT,
            timeline_folded: false,
            numbers: false,
            panes: [
                (Pane::Tool, false),
                (Pane::Layers, false),
                (Pane::Inspector, false),
                (Pane::Notes, true),
                (Pane::History, true),
            ]
            .into_iter()
            .map(|(pane, collapsed)| PaneState {
                pane,
                open: true,
                collapsed,
                body: pane.default_body(),
            })
            .collect(),
            hidden: false,
        }
    }
}

impl Layout {
    pub fn repaired(mut self) -> Self {
        let known: Vec<Pane> = self.panes.iter().map(|p| p.pane).collect();
        self.panes.dedup_by_key(|p| p.pane);
        Layout::default()
            .panes
            .into_iter()
            .filter(|p| !known.contains(&p.pane))
            .for_each(|p| self.panes.push(p));
        self.panes
            .iter_mut()
            .for_each(|p| p.body = p.body.max(p.pane.min_body()));
        self
    }

    pub fn shows_strip(&self) -> bool {
        self.strip && !self.hidden
    }

    pub fn shows_dock(&self) -> bool {
        self.dock && !self.hidden && self.panes.iter().any(|p| p.open)
    }

    pub fn shows_timeline(&self) -> bool {
        self.timeline && !self.hidden
    }

    pub fn pane(&self, pane: Pane) -> Option<&PaneState> {
        self.panes.iter().find(|p| p.pane == pane)
    }

    pub fn pane_mut(&mut self, pane: Pane) -> Option<&mut PaneState> {
        self.panes.iter_mut().find(|p| p.pane == pane)
    }

    pub fn is_open(&self, pane: Pane) -> bool {
        self.pane(pane).is_some_and(|p| p.open)
    }

    pub fn toggle(&mut self, pane: Pane) {
        let show = !(self.is_open(pane) && self.dock);
        if let Some(p) = self.pane_mut(pane) {
            p.open = show;
            if show {
                p.collapsed = false;
            }
        }
        if show {
            self.dock = true;
            self.hidden = false;
        }
    }

    pub fn dock_range(window: Vec2) -> (f32, f32) {
        (DOCK_MIN, (window.x * 0.45).max(DOCK_MIN))
    }

    pub fn timeline_range(window: Vec2) -> (f32, f32) {
        (TIMELINE_MIN, (window.y * 0.6).max(TIMELINE_MIN))
    }

    pub fn clamp(&mut self, window: Vec2) {
        let (lo, hi) = Self::dock_range(window);
        self.dock_width = self.dock_width.clamp(lo, hi);
        let (lo, hi) = Self::timeline_range(window);
        self.timeline_height = self.timeline_height.clamp(lo, hi);
        let room = window.y - STATUS - CANVAS_MIN.y;
        if self.timeline_height > room {
            self.timeline_height = room.max(TIMELINE_MIN);
        }
    }

    pub fn bodies(&self, available: f32) -> Vec<(Pane, Option<f32>)> {
        let open: Vec<&PaneState> = self.panes.iter().filter(|p| p.open).collect();
        let expanded: Vec<&PaneState> = open.iter().copied().filter(|p| !p.collapsed).collect();
        let room = (available - open.len() as f32 * TITLE).max(0.0);
        let wanted: f32 = expanded.iter().map(|p| p.body).sum();
        let floor: f32 = expanded.iter().map(|p| p.pane.min_body()).sum();
        let sizes: Vec<f32> = match wanted <= room {
            true => {
                let spare = room - wanted;
                expanded
                    .iter()
                    .enumerate()
                    .map(|(i, p)| p.body + if i + 1 == expanded.len() { spare } else { 0.0 })
                    .collect()
            }
            false if floor >= room => expanded.iter().map(|p| p.pane.min_body()).collect(),
            false => {
                let k = (room - floor) / (wanted - floor);
                expanded
                    .iter()
                    .map(|p| p.pane.min_body() + (p.body - p.pane.min_body()) * k)
                    .collect()
            }
        };
        open.iter()
            .map(|p| {
                let size = expanded.iter().position(|e| e.pane == p.pane).map(|i| sizes[i]);
                (p.pane, size)
            })
            .collect()
    }

    pub fn resize_pane(&mut self, pane: Pane, by: f32) {
        if let Some(p) = self.pane_mut(pane) {
            p.body = (p.body + by).max(p.pane.min_body());
        }
    }
}

pub fn take_tabs(raw: &mut egui::RawInput) -> Vec<bool> {
    let mut shifts = Vec::new();
    raw.events.retain(|e| match e {
        egui::Event::Key {
            key: egui::Key::Tab,
            pressed,
            modifiers,
            ..
        } if !modifiers.alt && !modifiers.command => {
            if *pressed {
                shifts.push(modifiers.shift);
            }
            false
        }
        _ => true,
    });
    shifts
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn panes_fit_the_dock_and_the_last_one_takes_what_is_left() {
        let layout = Layout::default();
        let bodies = layout.bodies(1200.0);
        assert_eq!(bodies.len(), 5);
        let used: f32 = bodies.iter().map(|(_, b)| b.unwrap_or(0.0)).sum::<f32>() + 5.0 * TITLE;
        assert!((used - 1200.0).abs() < 0.01, "{used}");
        assert_eq!(bodies[3], (Pane::Notes, None));
        assert!(bodies[2].1.unwrap() > Pane::Inspector.default_body());
        let small = layout.bodies(560.0);
        small.iter().filter_map(|(p, b)| b.map(|b| (p, b))).for_each(|(p, b)| {
            assert!(b >= p.min_body() - 0.01, "{p:?} {b}");
        });
        let used: f32 = small.iter().map(|(_, b)| b.unwrap_or(0.0)).sum::<f32>() + 5.0 * TITLE;
        assert!((used - 560.0).abs() < 0.01);
        let tiny = layout.bodies(200.0);
        assert!(tiny.iter().all(|(p, b)| b.is_none_or(|b| b == p.min_body())));
    }

    #[test]
    fn closing_collapsing_and_toggling_panes() {
        let mut layout = Layout::default();
        layout.toggle(Pane::Layers);
        assert!(!layout.is_open(Pane::Layers));
        assert_eq!(layout.bodies(900.0).len(), 4);
        layout.toggle(Pane::Notes);
        layout.toggle(Pane::Notes);
        assert!(layout.is_open(Pane::Notes) && !layout.pane(Pane::Notes).unwrap().collapsed);
        layout.dock = false;
        layout.toggle(Pane::Tool);
        assert!(layout.dock && layout.is_open(Pane::Tool));
        layout.panes.iter_mut().for_each(|p| p.open = false);
        assert!(!layout.shows_dock());
        layout.resize_pane(Pane::Tool, -1000.0);
        assert_eq!(layout.pane(Pane::Tool).unwrap().body, Pane::Tool.min_body());
    }

    #[test]
    fn sizes_clamp_to_the_window() {
        let mut layout = Layout {
            dock_width: 2000.0,
            timeline_height: 2000.0,
            ..Layout::default()
        };
        layout.clamp(vec2(1280.0, 800.0));
        assert_eq!(layout.dock_width, 1280.0 * 0.45);
        assert_eq!(layout.timeline_height, 800.0 * 0.6);
        layout.timeline_height = 2000.0;
        layout.clamp(vec2(1280.0, 500.0));
        assert_eq!(layout.timeline_height, 500.0 - STATUS - CANVAS_MIN.y);
        layout.dock_width = 10.0;
        layout.timeline_height = 10.0;
        layout.clamp(vec2(1280.0, 800.0));
        assert_eq!((layout.dock_width, layout.timeline_height), (DOCK_MIN, TIMELINE_MIN));
        layout.clamp(vec2(300.0, 260.0));
        assert_eq!((layout.dock_width, layout.timeline_height), (DOCK_MIN, TIMELINE_MIN));
        let tab = Layout {
            hidden: true,
            ..Layout::default()
        };
        assert!(!tab.shows_dock() && !tab.shows_strip() && !tab.shows_timeline());
    }

    #[test]
    fn the_layout_survives_a_restart_and_old_settings_gain_new_panes() {
        let mut layout = Layout {
            dock_width: 410.0,
            ..Layout::default()
        };
        layout.toggle(Pane::History);
        layout.hidden = true;
        let text = serde_json::to_string(&layout).unwrap();
        let back: Layout = serde_json::from_str(&text).unwrap();
        assert_eq!(back.dock_width, 410.0);
        assert!(!back.hidden && !back.is_open(Pane::History));
        let old = serde_json::from_str::<Layout>(
            r#"{"dockWidth":300,"panes":[{"pane":"layers","open":true,"collapsed":false,"body":5}]}"#,
        )
        .unwrap()
        .repaired();
        assert_eq!(old.panes.len(), 5);
        assert_eq!(old.panes[0].pane, Pane::Layers);
        assert_eq!(old.panes[0].body, Pane::Layers.min_body());
    }

    #[test]
    fn tab_is_taken_before_egui_moves_focus_with_it() {
        let key = |key, shift, pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers {
                shift,
                ..Default::default()
            },
        };
        let mut raw = egui::RawInput {
            events: vec![
                key(egui::Key::Tab, false, true),
                key(egui::Key::Tab, false, false),
                key(egui::Key::K, false, true),
                key(egui::Key::Tab, true, true),
            ],
            ..Default::default()
        };
        assert_eq!(take_tabs(&mut raw), [false, true]);
        assert_eq!(raw.events.len(), 1);
    }
}
