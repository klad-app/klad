use anyhow::Result;
use klad_brush::PressureCurve;
use klad_core::bundle::write_atomic;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brush: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default = "two")]
    pub exposure: u32,
    #[serde(default = "two")]
    pub onion_before: u32,
    #[serde(default = "two")]
    pub onion_after: u32,
    #[serde(default = "half")]
    pub onion_opacity: f32,
    #[serde(default = "surround")]
    pub surround: [u8; 3],
}

fn two() -> u32 {
    2
}

fn half() -> f32 {
    0.5
}

fn surround() -> [u8; 3] {
    crate::theme::SURROUND
}

fn one() -> f32 {
    1.0
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            brush: None,
            size: None,
            exposure: two(),
            onion_before: two(),
            onion_after: two(),
            onion_opacity: half(),
            surround: surround(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub pens: BTreeMap<String, PenSettings>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent: Vec<String>,
    #[serde(default)]
    pub layout: crate::layout::Layout,
    #[serde(default, rename = "recentCommands", skip_serializing_if = "Vec::is_empty")]
    pub recent_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: BTreeMap<String, Vec<String>>,
    #[serde(default = "one", rename = "uiScale")]
    pub ui_scale: f32,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default, rename = "recentDocuments", skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawings: Option<String>,
    #[serde(default = "yes", rename = "checkUpdates")]
    pub check_updates: bool,
    #[serde(default, rename = "updatesChecked")]
    pub updates_checked: u64,
    #[serde(default)]
    pub welcomed: bool,
    #[serde(default = "yes", rename = "pickAllLayers")]
    pub pick_all_layers: bool,
    #[serde(default, rename = "developerTools", skip_serializing_if = "std::ops::Not::not")]
    pub developer: bool,
    #[serde(default, rename = "eraserFeather", skip_serializing_if = "Option::is_none")]
    pub eraser_feather: Option<f32>,
    #[serde(default = "yes", rename = "pixelGrid")]
    pub pixel_grid: bool,
    #[serde(default, rename = "realSizeShown", skip_serializing_if = "std::ops::Not::not")]
    pub real_size_shown: bool,
    #[serde(default = "one_u32", rename = "realSizeScale")]
    pub real_size_scale: u32,
}

fn one_u32() -> u32 {
    1
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION,
            pens: BTreeMap::new(),
            recent: Vec::new(),
            layout: crate::layout::Layout::default(),
            recent_commands: Vec::new(),
            keys: BTreeMap::new(),
            ui_scale: one(),
            defaults: Defaults::default(),
            documents: Vec::new(),
            drawings: None,
            check_updates: true,
            updates_checked: 0,
            welcomed: false,
            pick_all_layers: true,
            developer: false,
            eraser_feather: None,
            pixel_grid: true,
            real_size_shown: false,
            real_size_scale: 1,
        }
    }
}

fn migrate(mut value: serde_json::Value) -> serde_json::Value {
    const PANES: [&str; 7] = ["tool", "brush", "layers", "inspector", "notes", "history", "preview"];
    if let Some(panes) = value.pointer_mut("/layout/panes").and_then(|p| p.as_array_mut()) {
        panes.retain(|p| p["pane"].as_str().is_some_and(|name| PANES.contains(&name)));
    }
    if let Some(map) = value.as_object_mut() {
        map.insert("version".into(), VERSION.into());
        map.remove("agent");
    }
    value
}

pub const RECENT: usize = 16;

pub const DOCUMENTS: usize = 12;

pub const ANY_PEN: &str = "tablet";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PenSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<PressureCurve>,
    #[serde(default)]
    pub peak: f32,
}

pub fn default_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("KLAD_SETTINGS") {
        return Some(PathBuf::from(path));
    }
    Some(carried_over(&crate::places::config()?))
}

pub fn carried_over(config: &Path) -> PathBuf {
    let path = config.join("klad").join("settings.json");
    let old = config.join("draw-anything").join("settings.json");
    if !path.exists() && old.is_file() {
        let _ = fs::create_dir_all(config.join("klad")).and_then(|_| fs::copy(&old, &path));
    }
    path
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let mut settings: Self = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .map(migrate)
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        settings.layout = settings.layout.repaired();
        settings.ui_scale = settings.ui_scale.clamp(0.75, 2.0);
        settings
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        write_atomic(path, text.as_bytes())
    }

    pub fn curve(&self, key: &str) -> PressureCurve {
        [key, ANY_PEN]
            .iter()
            .find_map(|k| self.pens.get(*k).and_then(|p| p.curve))
            .unwrap_or_default()
    }

    pub fn set_curve(&mut self, key: &str, curve: PressureCurve) {
        [key, ANY_PEN].iter().for_each(|k| {
            self.pens.entry(k.to_string()).or_default().curve = Some(curve);
        });
    }

    pub fn note_peak(&mut self, key: &str, pressure: f32) -> bool {
        let pen = self.pens.entry(key.to_string()).or_default();
        if pressure > pen.peak {
            pen.peak = pressure;
            return true;
        }
        false
    }

    pub fn peak(&self, key: &str) -> f32 {
        self.pens.get(key).map_or(0.0, |p| p.peak)
    }

    pub fn opened(&mut self, path: &Path) {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let text = path.display().to_string();
        self.documents.retain(|p| *p != text);
        self.documents.insert(0, text);
        self.documents.truncate(DOCUMENTS);
        self.welcomed = true;
    }

    pub fn forget(&mut self, path: &str) {
        self.documents.retain(|p| p != path);
    }

    pub fn drawings_folder(&self) -> PathBuf {
        self.drawings
            .as_ref()
            .map_or_else(crate::places::drawings, PathBuf::from)
    }

    pub fn used(&mut self, hex: &str) -> bool {
        if self.recent.first().is_some_and(|h| h.eq_ignore_ascii_case(hex)) {
            return false;
        }
        self.recent.retain(|h| !h.eq_ignore_ascii_case(hex));
        self.recent.insert(0, hex.to_string());
        self.recent.truncate(RECENT);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_with_the_removed_agent_pane_keep_everything_else() {
        let dir = std::env::temp_dir().join(format!("draw-settings-agent-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{"uiScale":1.25,"recentDocuments":["/w/a.klad"],"agent":{"model":"m","limit":2.0},
                "layout":{"dockWidth":400,"panes":[{"pane":"agent","open":true,"collapsed":false,"body":300},
                {"pane":"layers","open":true,"collapsed":false,"body":200},
                {"pane":"brush","open":false,"collapsed":false,"body":150},
                {"pane":"preview","open":true,"collapsed":true,"body":150}]}}"#,
        )
        .unwrap();
        let settings = Settings::load(&path);
        assert_eq!(settings.ui_scale, 1.25);
        assert_eq!(settings.documents, ["/w/a.klad"]);
        assert_eq!(settings.layout.dock_width, 400.0);
        assert_eq!(settings.layout.panes.len(), 7);
        assert_eq!(settings.layout.panes[0].pane, crate::layout::Pane::Layers);
        let pane = |pane| settings.layout.pane(pane).map(|p| (p.open, p.collapsed));
        assert_eq!(
            pane(crate::layout::Pane::Brush),
            Some((false, false)),
            "a closed brush pane stays closed"
        );
        assert_eq!(
            pane(crate::layout::Pane::Preview),
            Some((true, true)),
            "the real size pane comes back as it was left"
        );
        assert_eq!(
            Settings::default()
                .layout
                .pane(crate::layout::Pane::Preview)
                .map(|p| p.open),
            Some(false),
            "and is closed until a pixel drawing opens it"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn curves_are_kept_per_pen_and_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("draw-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut settings = Settings::load(&path);
        assert_eq!(settings.curve("pen-1"), PressureCurve::default());
        let heavy = PressureCurve {
            low: 0.02,
            high: 0.88,
            gamma: 1.2,
        };
        settings.set_curve("pen-1", heavy);
        assert!(settings.note_peak("pen-2", 0.7));
        assert!(!settings.note_peak("pen-2", 0.6));
        settings.save(&path).unwrap();
        let back = Settings::load(&path);
        assert_eq!(back.curve("pen-1"), heavy);
        assert_eq!(back.curve("pen-2"), heavy);
        assert_eq!(back.peak("pen-2"), 0.7);
        let mut two = back.clone();
        let light = PressureCurve {
            gamma: 0.7,
            ..PressureCurve::default()
        };
        two.set_curve("pen-2", light);
        assert_eq!((two.curve("pen-1"), two.curve("pen-2")), (heavy, light));
        assert_eq!(Settings::default().curve("pen-9"), PressureCurve::default());
        let mut colours = Settings::default();
        assert!(colours.used("#ff0000") && colours.used("#00ff00") && !colours.used("#00FF00"));
        assert!(colours.used("#ff0000"));
        (0..30).for_each(|i| {
            colours.used(&format!("#0000{i:02x}"));
        });
        assert_eq!(colours.recent.len(), RECENT);
        colours.layout.dock_width = 402.0;
        colours.layout.toggle(crate::layout::Pane::History);
        colours.recent_commands = vec!["clip.godot".into()];
        colours.save(&path).unwrap();
        let again = Settings::load(&path);
        assert_eq!(again.recent, colours.recent);
        assert_eq!(again.layout, colours.layout);
        assert_eq!(again.recent_commands, colours.recent_commands);
        fs::write(&path, r##"{"pens":{},"recent":["#ff0000"]}"##).unwrap();
        assert_eq!(Settings::load(&path).layout, crate::layout::Layout::default());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_from_before_the_rename_carry_over_once() {
        let config = std::env::temp_dir().join(format!("draw-settings-rename-{}", std::process::id()));
        let _ = fs::remove_dir_all(&config);
        fs::create_dir_all(config.join("draw-anything")).unwrap();
        fs::write(config.join("draw-anything/settings.json"), r#"{"uiScale":1.5}"#).unwrap();
        let path = carried_over(&config);
        assert_eq!(path, config.join("klad/settings.json"));
        assert_eq!(Settings::load(&path).ui_scale, 1.5);
        fs::write(&path, r#"{"uiScale":1.25}"#).unwrap();
        assert_eq!(Settings::load(&carried_over(&config)).ui_scale, 1.25);
        fs::remove_dir_all(&config).unwrap();
    }

    #[test]
    fn older_settings_load_and_new_ones_keep_keys_scale_and_defaults() {
        let dir = std::env::temp_dir().join(format!("draw-settings-v1-{}", std::process::id()));
        let path = dir.join("settings.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            &path,
            r##"{"pens":{"tablet":{"peak":0.8}},"recent":["#ff0000"],"recentCommands":["tool.brush"]}"##,
        )
        .unwrap();
        let old = Settings::load(&path);
        assert_eq!(old.version, VERSION);
        assert_eq!(old.peak("tablet"), 0.8);
        assert_eq!(old.recent_commands, ["tool.brush"]);
        assert_eq!(old.ui_scale, 1.0);
        assert_eq!(old.defaults, Defaults::default());
        let mut new = old.clone();
        new.keys.insert("layer.duplicate".into(), vec!["shift D".into()]);
        new.ui_scale = 1.5;
        new.defaults.brush = Some("pencil".into());
        new.defaults.size = Some(6.0);
        new.defaults.exposure = 3;
        new.defaults.surround = [20, 20, 24];
        new.save(&path).unwrap();
        assert_eq!(Settings::load(&path), new);
        let (a, b) = (dir.join("a.klad"), dir.join("b.klad"));
        new.opened(&a);
        new.opened(&b);
        new.opened(&a);
        assert_eq!(new.documents, [a.display().to_string(), b.display().to_string()]);
        (0..20).for_each(|i| new.opened(&dir.join(format!("{i}.klad"))));
        assert_eq!(new.documents.len(), DOCUMENTS);
        new.forget(&new.documents[0].clone());
        assert_eq!(new.documents.len(), DOCUMENTS - 1);
        new.drawings = Some("/art".into());
        assert_eq!(new.drawings_folder(), PathBuf::from("/art"));
        new.save(&path).unwrap();
        assert_eq!(Settings::load(&path), new);
        fs::write(&path, r#"{"version":2,"uiScale":9}"#).unwrap();
        assert_eq!(Settings::load(&path).ui_scale, 2.0);
        fs::remove_dir_all(&dir).unwrap();
    }
}
