use crate::theme::icon;
use egui::{Key, Modifiers};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cmd {
    Brush,
    Eraser,
    Fill,
    Pen,
    ShapeTool,
    TextTool,
    BoneTool,
    Symmetry,
    Parallax,
    MakeRepeat,
    MakeScatter,
    MakeMirror,
    Unwrap,
    ImportPicture,
    Shapes,
    Strokes,
    Points,
    Note,
    Eyedropper,
    Smaller,
    Bigger,
    Pan,
    Zoom,
    Fit,
    ActualSize,
    TurnRight,
    TurnLeft,
    Undo,
    Redo,
    Delete,
    Deselect,
    Copy,
    Paste,
    SelectAll,
    TransformLayer,
    CutLasso,
    Recolour,
    Thinner,
    Thicker,
    AttachedOnly,
    Detach,
    FlipH,
    FlipV,
    AlignLeft,
    AlignCentre,
    AlignRight,
    AlignTop,
    AlignMiddle,
    AlignBottom,
    DistributeAcross,
    DistributeDown,
    Corner,
    Smooth,
    Simplify,
    MakePath,
    AddSwatch,
    NewPaintLayer,
    NewPixelLayer,
    NewVectorLayer,
    ToggleVisible,
    ToggleLocked,
    ToggleClip,
    ToggleLockAlpha,
    AddMask,
    AddMaskHiding,
    PaintMask,
    BackToLayer,
    RemoveMask,
    ToggleMask,
    TintMask,
    OnionLayer,
    NextFrame,
    PrevFrame,
    NextDrawing,
    PrevDrawing,
    FirstFrame,
    LastFrame,
    NewDrawing,
    NewDrawingAfter,
    Duplicate,
    Link,
    Unlink,
    DeleteDrawing,
    HoldLonger,
    HoldShorter,
    InsertFrame,
    InsertFrameAll,
    RemoveFrame,
    RemoveFrameAll,
    ExposeOnes,
    ExposeTwos,
    ExposeThrees,
    FlipFrames,
    Roll,
    Play,
    LoopRange,
    Prepare,
    Onion,
    OnionSettings,
    OnOnes,
    OnTwos,
    OnThrees,
    FitFrames,
    Reverse,
    Pingpong,
    EveryLayer,
    Inbetween,
    RedoInbetweens,
    NewClip,
    DeleteClip,
    ClipLoops,
    ClipOwnFps,
    ExportGodot,
    Palette,
    Keys,
    HideAll,
    HideDock,
    LayersPane,
    ToolPane,
    BrushPane,
    InspectorPane,
    NotesPane,
    HistoryPane,
    Timeline,
    Numbers,
    Settings,
    BrushWindow,
    MessageLog,
    Spin,
    ResetNumbers,
    FlipView,
    ResetRotation,
    SwapColours,
    SizeDrag,
    TurnLeft90,
    TurnRight90,
    DuplicateInPlace,
    NudgeLeft,
    NudgeRight,
    NudgeUp,
    NudgeDown,
    NewGroupLayer,
    NewBonesLayer,
    NewAdjustLayer,
    DuplicateLayer,
    DeleteLayer,
    GroupLayer,
    RenameLayer,
    MoveEarlier,
    MoveLater,
    CopyCells,
    PasteCells,
    PasteCellsLinked,
    ClipSettings,
    NewDocument,
    OpenDocument,
    OpenRecent,
    CopyPath,
    InstallCommand,
    ReportProblem,
    ContactSupport,
    Guides,
    WhatsNew,
    PaintOver,
    CompareReference,
    About,
    Export,
    AddFont,
    Refill,
    FillLike,
    ShowGaps,
    ShowParts,
    ShowArcs,
    CloseDocument,
    Quit,
    FirstSteps,
    PickAllLayers,
    FillShape,
    OutlineShape,
    SoloLayer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Tools,
    View,
    Edit,
    Colour,
    Layers,
    Frames,
    Playback,
    Selected,
    Clips,
    Panels,
    File,
}

impl Group {
    pub fn name(self) -> &'static str {
        match self {
            Group::Tools => "tools",
            Group::View => "view",
            Group::Edit => "edit",
            Group::Colour => "colour",
            Group::Layers => "layers",
            Group::Frames => "frames",
            Group::Playback => "playback",
            Group::Selected => "selected frames",
            Group::Clips => "clips",
            Group::Panels => "panels",
            Group::File => "file",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keys {
    pub key: Key,
    pub shift: bool,
    pub alt: bool,
    pub command: bool,
    pub any_shift: bool,
}

const fn keys(key: Key, shift: bool, alt: bool, command: bool) -> Keys {
    Keys {
        key,
        shift,
        alt,
        command,
        any_shift: false,
    }
}

const fn plain(key: Key) -> Keys {
    keys(key, false, false, false)
}

const fn shift(key: Key) -> Keys {
    keys(key, true, false, false)
}

const fn alt(key: Key) -> Keys {
    keys(key, false, true, false)
}

const fn cmd(key: Key) -> Keys {
    keys(key, false, false, true)
}

const fn symbol(key: Key) -> Keys {
    Keys {
        any_shift: true,
        ..plain(key)
    }
}

impl Keys {
    pub fn matches(&self, key: Key, m: Modifiers) -> bool {
        self.key == key && self.alt == m.alt && self.command == m.command && (self.any_shift || self.shift == m.shift)
    }

    pub fn label(&self) -> String {
        let key = NAMED
            .iter()
            .find(|(k, _)| *k == self.key)
            .map_or_else(|| self.key.name().to_string(), |(_, n)| n.to_string());
        [(self.shift, "shift "), (self.alt, "alt "), (self.command, "cmd ")]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, word)| *word)
            .collect::<String>()
            + &key
    }
}

const NAMED: [(Key, &str); 15] = [
    (Key::Period, "."),
    (Key::Comma, ","),
    (Key::OpenBracket, "["),
    (Key::CloseBracket, "]"),
    (Key::Equals, "="),
    (Key::Plus, "+"),
    (Key::Minus, "-"),
    (Key::Questionmark, "?"),
    (Key::Enter, "Return"),
    (Key::Escape, "Esc"),
    (Key::Num1, "1"),
    (Key::ArrowLeft, "left"),
    (Key::ArrowRight, "right"),
    (Key::ArrowUp, "up"),
    (Key::ArrowDown, "down"),
];

const HELD: [(Key, &str); 4] = [
    (Key::Q, "flip through frames (hold Q)"),
    (Key::W, "show the previous drawing (hold W)"),
    (Key::Space, "pan (hold space)"),
    (Key::D, "parallax preview (hold D)"),
];

impl Keys {
    pub fn pressed(key: Key, m: Modifiers) -> Self {
        keys(key, m.shift, m.alt, m.command)
    }

    pub fn parse(text: &str) -> Option<Self> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let (name, mods) = words.split_last()?;
        let key = NAMED
            .iter()
            .find(|(_, n)| n == name)
            .map(|(k, _)| *k)
            .or_else(|| Key::from_name(name))?;
        let has = |w: &str| mods.contains(&w);
        if mods.iter().any(|m| !["shift", "alt", "cmd"].contains(m)) {
            return None;
        }
        Some(keys(key, has("shift"), has("alt"), has("cmd")))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bindings {
    overrides: BTreeMap<Cmd, Vec<Keys>>,
}

impl Bindings {
    pub fn from_settings(saved: &BTreeMap<String, Vec<String>>) -> Self {
        Self {
            overrides: saved
                .iter()
                .filter_map(|(id, list)| {
                    let cmd = Cmd::find(id).filter(|c| !c.spec().is_hold())?;
                    Some((cmd, list.iter().filter_map(|k| Keys::parse(k)).collect()))
                })
                .collect(),
        }
    }

    pub fn to_settings(&self) -> BTreeMap<String, Vec<String>> {
        self.overrides
            .iter()
            .map(|(c, list)| (c.spec().id.to_string(), list.iter().map(Keys::label).collect()))
            .collect()
    }

    pub fn keys(&self, cmd: Cmd) -> Vec<Keys> {
        match (self.overrides.get(&cmd), cmd.spec().trigger) {
            (Some(list), _) => list.clone(),
            (None, Trigger::Press(list)) => list.to_vec(),
            _ => Vec::new(),
        }
    }

    pub fn for_key(&self, key: Key, m: Modifiers) -> Option<Cmd> {
        Cmd::ALL
            .into_iter()
            .find(|c| self.keys(*c).iter().any(|k| k.matches(key, m)))
    }

    pub fn label(&self, cmd: Cmd) -> String {
        match cmd.spec().trigger {
            Trigger::Hold(how) => how.to_string(),
            _ => self.keys(cmd).iter().map(Keys::label).collect::<Vec<_>>().join(" or "),
        }
    }

    pub fn menu_key(&self, cmd: Cmd) -> String {
        let keys = self.keys(cmd);
        keys.iter()
            .find(|k| k.key == Key::Backspace)
            .or(keys.first())
            .map(Keys::label)
            .unwrap_or_default()
    }

    pub fn is_default(&self, cmd: Cmd) -> bool {
        !self.overrides.contains_key(&cmd)
    }

    pub fn bind(&mut self, cmd: Cmd, k: Keys) -> Result<(), String> {
        if cmd.spec().is_hold() {
            return Err(format!("{} is a hold; its keys do not change", cmd.spec().name));
        }
        if !k.shift
            && !k.alt
            && !k.command
            && let Some((_, what)) = HELD.iter().find(|(key, _)| *key == k.key)
        {
            return Err(format!("{} is taken by {what}", k.label()));
        }
        if let Some(other) = self.taken_by(k, cmd) {
            return Err(format!("{} already runs {}", k.label(), other.spec().name));
        }
        self.overrides.insert(cmd, vec![k]);
        Ok(())
    }

    pub fn taken_by(&self, k: Keys, except: Cmd) -> Option<Cmd> {
        let m = Modifiers {
            shift: k.shift,
            alt: k.alt,
            command: k.command,
            mac_cmd: k.command,
            ctrl: false,
        };
        Cmd::ALL
            .into_iter()
            .filter(|c| *c != except)
            .find(|c| self.keys(*c).iter().any(|o| o.matches(k.key, m)))
    }

    pub fn clear(&mut self, cmd: Cmd) {
        self.overrides.insert(cmd, Vec::new());
    }

    pub fn reset(&mut self, cmd: Cmd) {
        self.overrides.remove(&cmd);
    }

    pub fn reset_all(&mut self) {
        self.overrides.clear();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    None,
    Press(&'static [Keys]),
    Hold(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    No,
    Yes,
    WhenShown,
}

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub group: Group,
    pub trigger: Trigger,
    pub repeat: Repeat,
    pub about: &'static str,
}

impl Spec {
    pub fn key_label(&self) -> String {
        match self.trigger {
            Trigger::None => String::new(),
            Trigger::Press(list) => list.iter().map(Keys::label).collect::<Vec<_>>().join(" or "),
            Trigger::Hold(how) => how.to_string(),
        }
    }

    pub fn hint(&self) -> String {
        self.hint_with(&self.key_label())
    }

    pub fn hint_with(&self, key: &str) -> String {
        let key = key.to_string();
        match (key.is_empty(), self.trigger) {
            (true, _) => format!("{}\n{}", self.name, self.about),
            (false, Trigger::Hold(_)) => format!("{}: {key}\n{}", self.name, self.about),
            (false, _) => format!("{} ({key})\n{}", self.name, self.about),
        }
    }

    pub fn is_hold(&self) -> bool {
        matches!(self.trigger, Trigger::Hold(_))
    }
}

const fn spec(
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    group: Group,
    trigger: Trigger,
    repeat: Repeat,
    about: &'static str,
) -> Spec {
    Spec {
        id,
        name,
        icon,
        group,
        trigger,
        repeat,
        about,
    }
}

use Group as G;
use Repeat as R;
use Trigger as T;

impl Cmd {
    pub const ALL: [Cmd; 180] = [
        Cmd::Brush,
        Cmd::Eraser,
        Cmd::Fill,
        Cmd::Pen,
        Cmd::ShapeTool,
        Cmd::TextTool,
        Cmd::BoneTool,
        Cmd::Symmetry,
        Cmd::Parallax,
        Cmd::MakeRepeat,
        Cmd::MakeScatter,
        Cmd::MakeMirror,
        Cmd::Unwrap,
        Cmd::ImportPicture,
        Cmd::Shapes,
        Cmd::Strokes,
        Cmd::Points,
        Cmd::Note,
        Cmd::Eyedropper,
        Cmd::Smaller,
        Cmd::Bigger,
        Cmd::Pan,
        Cmd::Zoom,
        Cmd::Fit,
        Cmd::ActualSize,
        Cmd::TurnRight,
        Cmd::TurnLeft,
        Cmd::Undo,
        Cmd::Redo,
        Cmd::Delete,
        Cmd::Deselect,
        Cmd::Copy,
        Cmd::Paste,
        Cmd::SelectAll,
        Cmd::TransformLayer,
        Cmd::CutLasso,
        Cmd::Recolour,
        Cmd::Thinner,
        Cmd::Thicker,
        Cmd::AttachedOnly,
        Cmd::Detach,
        Cmd::FlipH,
        Cmd::FlipV,
        Cmd::AlignLeft,
        Cmd::AlignCentre,
        Cmd::AlignRight,
        Cmd::AlignTop,
        Cmd::AlignMiddle,
        Cmd::AlignBottom,
        Cmd::DistributeAcross,
        Cmd::DistributeDown,
        Cmd::Corner,
        Cmd::Smooth,
        Cmd::Simplify,
        Cmd::MakePath,
        Cmd::AddSwatch,
        Cmd::NewPaintLayer,
        Cmd::NewPixelLayer,
        Cmd::NewVectorLayer,
        Cmd::ToggleVisible,
        Cmd::ToggleLocked,
        Cmd::ToggleClip,
        Cmd::ToggleLockAlpha,
        Cmd::AddMask,
        Cmd::AddMaskHiding,
        Cmd::PaintMask,
        Cmd::BackToLayer,
        Cmd::RemoveMask,
        Cmd::ToggleMask,
        Cmd::TintMask,
        Cmd::OnionLayer,
        Cmd::NextFrame,
        Cmd::PrevFrame,
        Cmd::NextDrawing,
        Cmd::PrevDrawing,
        Cmd::FirstFrame,
        Cmd::LastFrame,
        Cmd::NewDrawing,
        Cmd::NewDrawingAfter,
        Cmd::Duplicate,
        Cmd::Link,
        Cmd::Unlink,
        Cmd::DeleteDrawing,
        Cmd::HoldLonger,
        Cmd::HoldShorter,
        Cmd::InsertFrame,
        Cmd::InsertFrameAll,
        Cmd::RemoveFrame,
        Cmd::RemoveFrameAll,
        Cmd::ExposeOnes,
        Cmd::ExposeTwos,
        Cmd::ExposeThrees,
        Cmd::FlipFrames,
        Cmd::Roll,
        Cmd::Play,
        Cmd::LoopRange,
        Cmd::Prepare,
        Cmd::Onion,
        Cmd::OnionSettings,
        Cmd::OnOnes,
        Cmd::OnTwos,
        Cmd::OnThrees,
        Cmd::FitFrames,
        Cmd::Reverse,
        Cmd::Pingpong,
        Cmd::EveryLayer,
        Cmd::Inbetween,
        Cmd::RedoInbetweens,
        Cmd::NewClip,
        Cmd::DeleteClip,
        Cmd::ClipLoops,
        Cmd::ClipOwnFps,
        Cmd::ExportGodot,
        Cmd::Palette,
        Cmd::Keys,
        Cmd::HideAll,
        Cmd::HideDock,
        Cmd::LayersPane,
        Cmd::ToolPane,
        Cmd::BrushPane,
        Cmd::InspectorPane,
        Cmd::NotesPane,
        Cmd::HistoryPane,
        Cmd::Timeline,
        Cmd::Numbers,
        Cmd::Settings,
        Cmd::BrushWindow,
        Cmd::MessageLog,
        Cmd::Spin,
        Cmd::ResetNumbers,
        Cmd::FlipView,
        Cmd::ResetRotation,
        Cmd::SwapColours,
        Cmd::SizeDrag,
        Cmd::TurnLeft90,
        Cmd::TurnRight90,
        Cmd::DuplicateInPlace,
        Cmd::NudgeLeft,
        Cmd::NudgeRight,
        Cmd::NudgeUp,
        Cmd::NudgeDown,
        Cmd::NewGroupLayer,
        Cmd::NewBonesLayer,
        Cmd::NewAdjustLayer,
        Cmd::DuplicateLayer,
        Cmd::DeleteLayer,
        Cmd::GroupLayer,
        Cmd::RenameLayer,
        Cmd::MoveEarlier,
        Cmd::MoveLater,
        Cmd::CopyCells,
        Cmd::PasteCells,
        Cmd::PasteCellsLinked,
        Cmd::ClipSettings,
        Cmd::NewDocument,
        Cmd::CloseDocument,
        Cmd::Quit,
        Cmd::FirstSteps,
        Cmd::PickAllLayers,
        Cmd::FillShape,
        Cmd::OutlineShape,
        Cmd::SoloLayer,
        Cmd::OpenDocument,
        Cmd::OpenRecent,
        Cmd::CopyPath,
        Cmd::InstallCommand,
        Cmd::ReportProblem,
        Cmd::ContactSupport,
        Cmd::Guides,
        Cmd::WhatsNew,
        Cmd::About,
        Cmd::Export,
        Cmd::AddFont,
        Cmd::Refill,
        Cmd::FillLike,
        Cmd::PaintOver,
        Cmd::ShowGaps,
        Cmd::ShowParts,
        Cmd::ShowArcs,
        Cmd::CompareReference,
    ];

    pub fn spec(self) -> Spec {
        match self {
            Cmd::Brush => spec(
                "tool.brush",
                "brush",
                icon::BRUSH,
                G::Tools,
                T::Press(const { &[plain(Key::B)] }),
                R::No,
                "paint strokes on the active paint layer",
            ),
            Cmd::Eraser => spec(
                "tool.eraser",
                "eraser",
                icon::ERASER,
                G::Tools,
                T::Press(const { &[plain(Key::E)] }),
                R::No,
                "switch between the brush and the eraser; the pen's eraser end erases too",
            ),
            Cmd::Fill => spec(
                "tool.fill",
                "fill",
                icon::FILL,
                G::Tools,
                T::Press(const { &[plain(Key::G)] }),
                R::No,
                "click inside lines to fill, drag a loop to lasso fill",
            ),
            Cmd::Shapes => spec(
                "tool.shapes",
                "select and move",
                icon::SHAPES,
                G::Tools,
                T::Press(const { &[plain(Key::V)] }),
                R::No,
                "pick the stroke or vector shape under the pen and drag it; alt drag drags a copy",
            ),
            Cmd::Strokes => spec(
                "tool.strokes",
                "select strokes",
                icon::STROKES,
                G::Tools,
                T::Press(const { &[plain(Key::S)] }),
                R::No,
                "pick strokes and fills on the active paint layer, or lasso them",
            ),
            Cmd::Pen => spec(
                "tool.pen",
                "pen",
                icon::PEN,
                G::Tools,
                T::Press(const { &[plain(Key::P)] }),
                R::No,
                "draw vector paths on a vector layer: click for corners, drag for curves, click the first point to close, Return to finish",
            ),
            Cmd::ShapeTool => spec(
                "tool.shape",
                "rectangles and ellipses",
                icon::RECTANGLE,
                G::Tools,
                T::Press(const { &[plain(Key::U)] }),
                R::No,
                "drag rectangles or ellipses on a vector layer: shift for a square or circle, alt from the centre",
            ),
            Cmd::TextTool => spec(
                "tool.text",
                "text",
                icon::TEXT_T,
                G::Tools,
                T::Press(const { &[plain(Key::T)] }),
                R::No,
                "click on the canvas and type: Return starts a new line, Esc finishes; click text to edit it",
            ),
            Cmd::BoneTool => spec(
                "tool.bones",
                "bones",
                icon::BONE,
                G::Tools,
                T::Press(const { &[plain(Key::J)] }),
                R::No,
                "draw a skeleton on the layer or group, paint how much each bone moves the drawing, and pose it",
            ),
            Cmd::Symmetry => spec(
                "brush.mirror",
                "mirror painting",
                icon::FLIP_H,
                G::Tools,
                T::None,
                R::No,
                "every stroke gets its mirror image across an upright line as you paint it",
            ),
            Cmd::Parallax => spec(
                "view.parallax",
                "parallax preview",
                icon::MOUNTAINS,
                G::View,
                T::Hold("hold D and move the pointer"),
                R::No,
                "the planes at the top of the stack slide by their depth, near ones most",
            ),
            Cmd::MakeRepeat => spec(
                "shape.repeat",
                "repeat it",
                icon::REPEAT,
                G::Layers,
                T::None,
                R::No,
                "put the selected shape in a repeat: copies along a line, an arc or a grid, kept as settings",
            ),
            Cmd::MakeScatter => spec(
                "shape.scatter",
                "scatter it",
                icon::SCATTER,
                G::Layers,
                T::None,
                R::No,
                "put the selected shape in a scatter: copies strewn over a region, sized and turned from a seed",
            ),
            Cmd::MakeMirror => spec(
                "shape.mirror",
                "mirror it",
                icon::FLIP_H,
                G::Layers,
                T::None,
                R::No,
                "put the selected shape in a mirror: it and its mirror image across an axis",
            ),
            Cmd::Unwrap => spec(
                "shape.unwrap",
                "take the shapes out",
                icon::REPEAT,
                G::Layers,
                T::None,
                R::No,
                "remove the selected repeat, scatter or mirror and keep the shapes inside it (where the first copy was)",
            ),
            Cmd::ImportPicture => spec(
                "layer.import-picture",
                "import a picture",
                icon::IMAGE,
                G::Layers,
                T::None,
                R::No,
                "place a PNG or JPEG on a new layer to trace over or paint from (dropping the file works too)",
            ),
            Cmd::Points => spec(
                "tool.points",
                "edit points",
                icon::POINTS,
                G::Tools,
                T::Press(const { &[plain(Key::A)] }),
                R::No,
                "move the points and handles of vector shapes and of brush strokes (a stroke keeps its pressure along its new path)",
            ),
            Cmd::Note => spec(
                "tool.note",
                "pin a note for the agent",
                icon::NOTE,
                G::Tools,
                T::Press(const { &[plain(Key::N)] }),
                R::No,
                "click the canvas to pin a note; the agent reads it with klad notes",
            ),
            Cmd::Eyedropper => spec(
                "colour.pick",
                "pick a colour",
                icon::EYEDROPPER,
                G::Colour,
                T::Hold("hold alt and click (shift alt: this layer only)"),
                R::No,
                "take the colour under the pointer with the brush or fill tool",
            ),
            Cmd::Smaller => spec(
                "brush.smaller",
                "smaller brush",
                icon::MINUS,
                G::Tools,
                T::Press(const { &[plain(Key::OpenBracket)] }),
                R::Yes,
                "the brush size goes down by a fifth",
            ),
            Cmd::Bigger => spec(
                "brush.bigger",
                "bigger brush",
                icon::PLUS,
                G::Tools,
                T::Press(const { &[plain(Key::CloseBracket)] }),
                R::Yes,
                "the brush size goes up by a quarter",
            ),
            Cmd::Pan => spec(
                "view.pan",
                "pan",
                icon::HAND,
                G::View,
                T::Hold("hold space and drag, or drag with the middle button"),
                R::No,
                "move the view",
            ),
            Cmd::Zoom => spec(
                "view.zoom",
                "zoom",
                icon::ZOOM,
                G::View,
                T::Hold("pinch, or cmd and scroll"),
                R::No,
                "zoom the view about the pointer",
            ),
            Cmd::Fit => spec(
                "view.fit",
                "fit the canvas in the window",
                icon::FIT,
                G::View,
                T::Press(const { &[plain(Key::F)] }),
                R::No,
                "zoom and centre so the whole canvas shows",
            ),
            Cmd::ActualSize => spec(
                "view.100",
                "zoom to 100%",
                icon::ZOOM,
                G::View,
                T::Press(const { &[plain(Key::Num1)] }),
                R::No,
                "one canvas pixel per screen pixel",
            ),
            Cmd::TurnRight => spec(
                "view.turn-right",
                "turn the view right",
                icon::TURN,
                G::View,
                T::Press(const { &[plain(Key::R)] }),
                R::Yes,
                "rotate the view 15 degrees clockwise",
            ),
            Cmd::TurnLeft => spec(
                "view.turn-left",
                "turn the view left",
                icon::TURN,
                G::View,
                T::Press(const { &[shift(Key::R)] }),
                R::Yes,
                "rotate the view 15 degrees anticlockwise",
            ),
            Cmd::Undo => spec(
                "edit.undo",
                "undo",
                icon::UNDO,
                G::Edit,
                T::Press(const { &[cmd(Key::Z)] }),
                R::Yes,
                "undo the last change, yours or the agent's",
            ),
            Cmd::Redo => spec(
                "edit.redo",
                "redo",
                icon::REDO,
                G::Edit,
                T::Press(const { &[keys(Key::Z, true, false, true)] }),
                R::Yes,
                "redo what was undone",
            ),
            Cmd::Delete => spec(
                "edit.delete",
                "delete the selection",
                icon::TRASH,
                G::Edit,
                T::Press(const { &[plain(Key::Delete), plain(Key::Backspace)] }),
                R::No,
                "remove the selected strokes, points or shape",
            ),
            Cmd::Deselect => spec(
                "edit.deselect",
                "clear the selection",
                icon::CLOSE,
                G::Edit,
                T::Press(const { &[plain(Key::Escape)] }),
                R::No,
                "drop the selection on the canvas and in the timeline",
            ),
            Cmd::Copy => spec(
                "edit.copy",
                "copy the selected strokes",
                icon::COPY,
                G::Edit,
                T::Press(const { &[cmd(Key::C)] }),
                R::No,
                "keep the strokes selected with S for cmd V",
            ),
            Cmd::Paste => spec(
                "edit.paste",
                "paste strokes into this frame",
                icon::PASTE,
                G::Edit,
                T::Press(const { &[cmd(Key::V)] }),
                R::No,
                "copy the kept strokes into the active layer's drawing at this frame, in place",
            ),
            Cmd::TransformLayer => spec(
                "edit.transform-layer",
                "transform the whole layer",
                icon::SHAPES,
                G::Edit,
                T::Press(const { &[cmd(Key::T)] }),
                R::No,
                "move, turn or scale the whole active layer or group, in every drawing, with its bones and poses",
            ),
            Cmd::SelectAll => spec(
                "edit.select-all",
                "select every stroke on this layer",
                icon::SELECT_ALL,
                G::Edit,
                T::Press(const { &[cmd(Key::A)] }),
                R::No,
                "every stroke and fill of the active layer's drawing at this frame",
            ),
            Cmd::CutLasso => spec(
                "edit.cut-lasso",
                "a lasso cuts strokes at its edge",
                icon::LASSO,
                G::Edit,
                T::None,
                R::No,
                "on: a lasso also takes the parts of strokes crossing it; off: only strokes wholly inside",
            ),
            Cmd::Recolour => spec(
                "edit.recolour",
                "recolour the selected strokes",
                icon::PALETTE,
                G::Edit,
                T::None,
                R::No,
                "give the selected strokes and fills the current colour",
            ),
            Cmd::Thinner => spec(
                "edit.thinner",
                "make the selected strokes thinner",
                icon::MINUS,
                G::Edit,
                T::None,
                R::No,
                "four fifths of their size",
            ),
            Cmd::Thicker => spec(
                "edit.thicker",
                "make the selected strokes thicker",
                icon::PLUS,
                G::Edit,
                T::None,
                R::No,
                "a quarter bigger",
            ),
            Cmd::AttachedOnly => spec(
                "edit.attached-only",
                "select only what is attached",
                icon::LINK,
                G::Edit,
                T::None,
                R::No,
                "select the erasers, smudges and blends attached to the selected strokes, to move or delete them on their own",
            ),
            Cmd::Detach => spec(
                "edit.detach",
                "detach erasers, smudges and blends",
                icon::UNLINK,
                G::Edit,
                T::None,
                R::No,
                "free what is attached to the selected strokes: it stays where it is and acts on whatever is under it",
            ),
            Cmd::AlignLeft => spec(
                "edit.align-left",
                "align left edges",
                icon::ALIGN_LEFT,
                G::Edit,
                T::None,
                R::No,
                "line the selected shapes up on their leftmost edge; one shape goes to the left edge of the canvas",
            ),
            Cmd::AlignCentre => spec(
                "edit.align-centre",
                "align horizontal centres",
                icon::ALIGN_CENTRE_ACROSS,
                G::Edit,
                T::None,
                R::No,
                "put the selected shapes on one vertical line through their middle; one shape goes to the middle of the canvas",
            ),
            Cmd::AlignRight => spec(
                "edit.align-right",
                "align right edges",
                icon::ALIGN_RIGHT,
                G::Edit,
                T::None,
                R::No,
                "line the selected shapes up on their rightmost edge; one shape goes to the right edge of the canvas",
            ),
            Cmd::AlignTop => spec(
                "edit.align-top",
                "align top edges",
                icon::ALIGN_TOP,
                G::Edit,
                T::None,
                R::No,
                "line the selected shapes up on their topmost edge; one shape goes to the top of the canvas",
            ),
            Cmd::AlignMiddle => spec(
                "edit.align-middle",
                "align vertical centres",
                icon::ALIGN_CENTRE_DOWN,
                G::Edit,
                T::None,
                R::No,
                "put the selected shapes on one horizontal line through their middle; one shape goes to the middle of the canvas",
            ),
            Cmd::AlignBottom => spec(
                "edit.align-bottom",
                "align bottom edges",
                icon::ALIGN_BOTTOM,
                G::Edit,
                T::None,
                R::No,
                "line the selected shapes up on their lowest edge; one shape goes to the bottom of the canvas",
            ),
            Cmd::DistributeAcross => spec(
                "edit.distribute-across",
                "distribute across",
                icon::DISTRIBUTE_ACROSS,
                G::Edit,
                T::None,
                R::No,
                "keep the leftmost and rightmost of the selected shapes and make the gaps between them equal",
            ),
            Cmd::DistributeDown => spec(
                "edit.distribute-down",
                "distribute down",
                icon::DISTRIBUTE_DOWN,
                G::Edit,
                T::None,
                R::No,
                "keep the top and bottom of the selected shapes and make the gaps between them equal",
            ),
            Cmd::FlipH => spec(
                "edit.flip-h",
                "flip the selection left-right",
                icon::FLIP_H,
                G::Edit,
                T::None,
                R::No,
                "mirror the selected strokes about their middle",
            ),
            Cmd::FlipV => spec(
                "edit.flip-v",
                "flip the selection up-down",
                icon::FLIP_V,
                G::Edit,
                T::None,
                R::No,
                "mirror the selected strokes about their middle",
            ),
            Cmd::Corner => spec(
                "points.corner",
                "make the selected points corners",
                icon::POINTS,
                G::Edit,
                T::None,
                R::No,
                "drop their handles",
            ),
            Cmd::Smooth => spec(
                "points.smooth",
                "make the selected points smooth",
                icon::POINTS,
                G::Edit,
                T::None,
                R::No,
                "handles in line, a third of the way to the neighbours",
            ),
            Cmd::Simplify => spec(
                "points.simplify",
                "simplify the path",
                icon::POINTS,
                G::Edit,
                T::None,
                R::No,
                "fewer, smooth points that stay within the set distance of the outline",
            ),
            Cmd::MakePath => spec(
                "points.make-path",
                "turn the shape into a path",
                icon::POINTS,
                G::Edit,
                T::None,
                R::No,
                "a rect or ellipse becomes a path that looks the same",
            ),
            Cmd::AddSwatch => spec(
                "colour.add",
                "add the colour to the palette",
                icon::PALETTE,
                G::Colour,
                T::None,
                R::No,
                "the current colour, with the name typed next to it",
            ),
            Cmd::NewPaintLayer => spec(
                "layer.new-paint",
                "new paint layer",
                icon::LAYERS,
                G::Layers,
                T::None,
                R::No,
                "a paint layer above the active one, with a drawing at this frame",
            ),
            Cmd::NewPixelLayer => spec(
                "layer.new-pixel",
                "new pixel layer",
                icon::PIXELS,
                G::Layers,
                T::None,
                R::No,
                "a pixel layer above the active one: whole pixels with no soft edges, each bound to a palette colour",
            ),
            Cmd::NewAdjustLayer => spec(
                "layer.new-adjust",
                "new adjustment layer",
                icon::ADJUST,
                G::Layers,
                T::None,
                R::No,
                "curves above the active layer, changing what is under it in its group; layer settings switch it to levels, hue, balance, a gradient map or a colour fill",
            ),
            Cmd::NewVectorLayer => spec(
                "layer.new-vector",
                "new vector layer",
                icon::LAYERS,
                G::Layers,
                T::None,
                R::No,
                "a vector layer above the active one",
            ),
            Cmd::ToggleVisible => spec(
                "layer.visible",
                "show or hide the active layer",
                icon::EYE,
                G::Layers,
                T::None,
                R::No,
                "the eye of the active layer",
            ),
            Cmd::ToggleLocked => spec(
                "layer.locked",
                "lock the active layer",
                icon::LOCK,
                G::Layers,
                T::None,
                R::No,
                "a locked layer cannot be picked or edited on the canvas",
            ),
            Cmd::ToggleClip => spec(
                "layer.clip",
                "clip the active layer to the one below",
                icon::CLIP,
                G::Layers,
                T::None,
                R::No,
                "show it only where the layer below has paint",
            ),
            Cmd::ToggleLockAlpha => spec(
                "layer.lock-alpha",
                "lock the active layer's alpha",
                icon::LOCK,
                G::Layers,
                T::None,
                R::No,
                "new strokes and fills land only where the layer already has paint",
            ),
            Cmd::AddMask => spec(
                "mask.add",
                "add a mask",
                icon::MASK,
                G::Layers,
                T::None,
                R::No,
                "an empty mask: paint on it to hide parts of the layer",
            ),
            Cmd::AddMaskHiding => spec(
                "mask.add-hiding",
                "add a mask hiding all",
                icon::MASK,
                G::Layers,
                T::None,
                R::No,
                "a full mask: erase it to show parts of the layer",
            ),
            Cmd::PaintMask => spec(
                "mask.paint",
                "paint on the mask",
                icon::MASK,
                G::Layers,
                T::None,
                R::No,
                "make the active layer's mask the layer you paint on",
            ),
            Cmd::BackToLayer => spec(
                "mask.back",
                "back to the layer",
                icon::LAYERS,
                G::Layers,
                T::None,
                R::No,
                "paint on the layer again instead of its mask",
            ),
            Cmd::RemoveMask => spec(
                "mask.remove",
                "remove the mask",
                icon::TRASH,
                G::Layers,
                T::None,
                R::No,
                "delete the active layer's mask (cmd Z brings it back)",
            ),
            Cmd::ToggleMask => spec(
                "mask.on",
                "turn the mask on or off",
                icon::MASK,
                G::Layers,
                T::None,
                R::No,
                "an off mask hides nothing",
            ),
            Cmd::TintMask => spec(
                "mask.tint",
                "show the mask in red",
                icon::MASK,
                G::Layers,
                T::None,
                R::No,
                "tint what the mask hides while you paint on it",
            ),
            Cmd::OnionLayer => spec(
                "onion.layer",
                "show this layer's onion too",
                icon::ONION,
                G::Layers,
                T::None,
                R::No,
                "the dot at the right of a layer's name in the timeline",
            ),
            Cmd::NextFrame => spec(
                "frame.next",
                "next frame",
                icon::STEP_ON,
                G::Frames,
                T::Press(const { &[plain(Key::Period)] }),
                R::WhenShown,
                "held, it waits for each frame to show",
            ),
            Cmd::PrevFrame => spec(
                "frame.previous",
                "previous frame",
                icon::STEP_BACK,
                G::Frames,
                T::Press(const { &[plain(Key::Comma)] }),
                R::WhenShown,
                "held, it waits for each frame to show",
            ),
            Cmd::NextDrawing => spec(
                "frame.next-drawing",
                "next drawing",
                icon::NEXT,
                G::Frames,
                T::Press(const { &[alt(Key::Period)] }),
                R::WhenShown,
                "jump to where the active layer's next drawing starts",
            ),
            Cmd::PrevDrawing => spec(
                "frame.previous-drawing",
                "previous drawing",
                icon::PREVIOUS,
                G::Frames,
                T::Press(const { &[alt(Key::Comma)] }),
                R::WhenShown,
                "jump to where the active layer's previous drawing starts",
            ),
            Cmd::FirstFrame => spec(
                "frame.first",
                "first frame",
                icon::PREVIOUS,
                G::Frames,
                T::Press(const { &[plain(Key::Home)] }),
                R::No,
                "go to frame 0",
            ),
            Cmd::LastFrame => spec(
                "frame.last",
                "last frame",
                icon::NEXT,
                G::Frames,
                T::Press(const { &[plain(Key::End)] }),
                R::No,
                "go to the timeline's last frame",
            ),
            Cmd::NewDrawing => spec(
                "drawing.new",
                "new drawing here",
                icon::NEW_DRAWING,
                G::Frames,
                T::Press(const { &[plain(Key::K)] }),
                R::No,
                "a blank drawing at this frame on the active layer",
            ),
            Cmd::NewDrawingAfter => spec(
                "drawing.new-after",
                "new drawing after this one",
                icon::NEW_DRAWING,
                G::Frames,
                T::Press(const { &[shift(Key::K)] }),
                R::No,
                "a blank drawing right after this exposure, on ones, twos or threes, and go there",
            ),
            Cmd::Duplicate => spec(
                "drawing.duplicate",
                "duplicate to the next frame",
                icon::COPY,
                G::Frames,
                T::None,
                R::No,
                "a copy of this drawing right after its exposure, with the same stroke ids",
            ),
            Cmd::Link => spec(
                "drawing.link",
                "show a drawing here (link)",
                icon::LINK,
                G::Frames,
                T::None,
                R::No,
                "expose one of the layer's drawings again at this frame; drawing on it changes both",
            ),
            Cmd::Unlink => spec(
                "drawing.unlink",
                "unlink",
                icon::UNLINK,
                G::Frames,
                T::None,
                R::No,
                "the link at this frame becomes a drawing of its own",
            ),
            Cmd::DeleteDrawing => spec(
                "drawing.delete",
                "delete the drawing on this frame",
                icon::TRASH,
                G::Frames,
                T::None,
                R::No,
                "links to it are handed the drawing",
            ),
            Cmd::HoldLonger => spec(
                "drawing.longer",
                "hold one frame longer",
                icon::PLUS,
                G::Frames,
                T::Press(const { &[plain(Key::Equals), symbol(Key::Plus)] }),
                R::Yes,
                "this drawing shows one frame more; later drawings move along",
            ),
            Cmd::HoldShorter => spec(
                "drawing.shorter",
                "hold one frame shorter",
                icon::MINUS,
                G::Frames,
                T::Press(const { &[plain(Key::Minus)] }),
                R::Yes,
                "this drawing shows one frame less",
            ),
            Cmd::InsertFrame => spec(
                "frames.insert",
                "insert a frame in this layer",
                icon::INSERT,
                G::Frames,
                T::Press(const { &[plain(Key::I)] }),
                R::No,
                "what shows here also shows one frame more",
            ),
            Cmd::InsertFrameAll => spec(
                "frames.insert-all",
                "insert a frame in every layer",
                icon::INSERT,
                G::Frames,
                T::Press(const { &[shift(Key::I)] }),
                R::No,
                "the timeline and its clips grow by a frame",
            ),
            Cmd::RemoveFrame => spec(
                "frames.remove",
                "remove this frame from this layer",
                icon::REMOVE,
                G::Frames,
                T::None,
                R::No,
                "later drawings of the layer move forward",
            ),
            Cmd::RemoveFrameAll => spec(
                "frames.remove-all",
                "remove this frame from every layer",
                icon::REMOVE,
                G::Frames,
                T::None,
                R::No,
                "the timeline and its clips shrink by a frame",
            ),
            Cmd::ExposeOnes => spec(
                "expose.ones",
                "new drawings on ones",
                icon::NEW_DRAWING,
                G::Frames,
                T::None,
                R::No,
                "shift K makes drawings that show one frame each",
            ),
            Cmd::ExposeTwos => spec(
                "expose.twos",
                "new drawings on twos",
                icon::NEW_DRAWING,
                G::Frames,
                T::None,
                R::No,
                "shift K makes drawings that show two frames each",
            ),
            Cmd::ExposeThrees => spec(
                "expose.threes",
                "new drawings on threes",
                icon::NEW_DRAWING,
                G::Frames,
                T::None,
                R::No,
                "shift K makes drawings that show three frames each",
            ),
            Cmd::FlipFrames => spec(
                "frame.flip",
                "flip through frames",
                icon::FLIP_FRAMES,
                G::Frames,
                T::Hold("hold Q and move the pen sideways"),
                R::No,
                "about one frame per 22 pt; let go to stay",
            ),
            Cmd::Roll => spec(
                "frame.roll",
                "see the previous drawing",
                icon::PREVIOUS,
                G::Frames,
                T::Hold("hold W"),
                R::No,
                "let go to come back",
            ),
            Cmd::Play => spec(
                "play.toggle",
                "play or stop",
                icon::PLAY,
                G::Playback,
                T::Press(const { &[plain(Key::Enter)] }),
                R::No,
                "plays the loop range, else the clip under the playhead, else everything",
            ),
            Cmd::LoopRange => spec(
                "play.loop",
                "loop a range of frames",
                icon::LOOP,
                G::Playback,
                T::None,
                R::No,
                "mark a range around the playhead (or shift drag on the ruler) that Return plays",
            ),
            Cmd::Prepare => spec(
                "play.prepare",
                "prepare frames",
                icon::PREPARE,
                G::Playback,
                T::None,
                R::No,
                "buffer the loop range, else the clip, else every frame: each frame's picture so it shows at once, each drawing's pixels so drawing there starts fast; the strip on the ruler shows what is ready; again to stop",
            ),
            Cmd::Onion => spec(
                "onion.toggle",
                "onion skin",
                icon::ONION,
                G::Playback,
                T::Press(const { &[plain(Key::O)] }),
                R::No,
                "see the drawings before (red) and after (green) under the active layer",
            ),
            Cmd::OnionSettings => spec(
                "onion.settings",
                "onion skin settings",
                icon::SETTINGS,
                G::Playback,
                T::None,
                R::No,
                "how many before and after, by drawings or frames, opacity, falloff, tint",
            ),
            Cmd::OnOnes => spec(
                "span.ones",
                "retime the selected frames on ones",
                icon::RETIME,
                G::Selected,
                T::None,
                R::No,
                "each picture shows one frame; later frames and clips move",
            ),
            Cmd::OnTwos => spec(
                "span.twos",
                "retime the selected frames on twos",
                icon::RETIME,
                G::Selected,
                T::None,
                R::No,
                "each picture shows two frames; later frames and clips move",
            ),
            Cmd::OnThrees => spec(
                "span.threes",
                "retime the selected frames on threes",
                icon::RETIME,
                G::Selected,
                T::None,
                R::No,
                "each picture shows three frames; later frames and clips move",
            ),
            Cmd::FitFrames => spec(
                "span.fit",
                "fit the selected frames into n frames",
                icon::RETIME,
                G::Selected,
                T::None,
                R::No,
                "keep their order and relative timing in the number set in the selection's menu",
            ),
            Cmd::Reverse => spec(
                "span.reverse",
                "reverse the selected frames",
                icon::RETIME,
                G::Selected,
                T::None,
                R::No,
                "play the pictures backwards",
            ),
            Cmd::Pingpong => spec(
                "span.pingpong",
                "ping-pong the selected frames",
                icon::RETIME,
                G::Selected,
                T::None,
                R::No,
                "add them again backwards as links, ends not repeated",
            ),
            Cmd::EveryLayer => spec(
                "span.every-layer",
                "retime on every layer",
                icon::LAYERS,
                G::Selected,
                T::None,
                R::No,
                "on: every layer and the clips follow; off: only the selected rows",
            ),
            Cmd::Inbetween => spec(
                "span.inbetween",
                "in-between the selection",
                icon::INBETWEEN,
                G::Selected,
                T::None,
                R::No,
                "make the drawings between the selection's two keys, with the count, spacing and pairing from its menu",
            ),
            Cmd::RedoInbetweens => spec(
                "span.redo",
                "redo the in-betweens in the selection",
                icon::INBETWEEN,
                G::Selected,
                T::None,
                R::No,
                "remake them after a key changed; ones drawn on are refused",
            ),
            Cmd::NewClip => spec(
                "clip.new",
                "new clip from the selection",
                icon::CLIP_BAR,
                G::Clips,
                T::None,
                R::No,
                "a looping clip over the selected frames, else the loop range",
            ),
            Cmd::DeleteClip => spec(
                "clip.delete",
                "delete the clip",
                icon::TRASH,
                G::Clips,
                T::None,
                R::No,
                "the picked clip, or the one under the playhead",
            ),
            Cmd::ClipLoops => spec(
                "clip.loops",
                "the clip loops",
                icon::LOOP,
                G::Clips,
                T::None,
                R::No,
                "off: it plays once and stops on its last frame",
            ),
            Cmd::ClipOwnFps => spec(
                "clip.fps",
                "the clip has its own fps",
                icon::CLIP_BAR,
                G::Clips,
                T::None,
                R::No,
                "off: it plays at the document's fps",
            ),
            Cmd::ExportGodot => spec(
                "clip.godot",
                "export to Godot",
                icon::EXPORT,
                G::Clips,
                T::None,
                R::No,
                "write the sprite sheet, SpriteFrames and scene into the Godot folder",
            ),
            Cmd::Palette => spec(
                "panel.palette",
                "find a command",
                icon::SEARCH,
                G::Panels,
                T::Press(const { &[cmd(Key::K)] }),
                R::No,
                "type a few letters of what you want to do",
            ),
            Cmd::Keys => spec(
                "panel.keys",
                "every key",
                icon::KEYBOARD,
                G::Panels,
                T::Press(const { &[symbol(Key::Questionmark)] }),
                R::No,
                "the command list with every key",
            ),
            Cmd::HideAll => spec(
                "panel.hide-all",
                "hide the panels",
                icon::SIDEBAR,
                G::Panels,
                T::Press(const { &[plain(Key::Tab)] }),
                R::No,
                "canvas only, with the status line; again to bring them back",
            ),
            Cmd::HideDock => spec(
                "panel.hide-dock",
                "hide the dock",
                icon::SIDEBAR,
                G::Panels,
                T::Press(const { &[shift(Key::Tab)] }),
                R::No,
                "the panels on the right",
            ),
            Cmd::LayersPane => spec(
                "panel.layers",
                "layers panel",
                icon::LAYERS,
                G::Panels,
                T::None,
                R::No,
                "show or hide it in the dock",
            ),
            Cmd::ToolPane => spec(
                "panel.tool",
                "colour panel",
                icon::PALETTE,
                G::Panels,
                T::None,
                R::No,
                "show or hide it in the dock",
            ),
            Cmd::BrushPane => spec(
                "panel.brushes",
                "brush panel",
                icon::BRUSH,
                G::Panels,
                T::None,
                R::No,
                "show or hide it in the dock",
            ),
            Cmd::InspectorPane => spec(
                "panel.inspector",
                "layer settings panel",
                icon::INSPECTOR,
                G::Panels,
                T::None,
                R::No,
                "show or hide it in the dock",
            ),
            Cmd::NotesPane => spec(
                "panel.notes",
                "notes panel",
                icon::NOTE,
                G::Panels,
                T::None,
                R::No,
                "show or hide it in the dock",
            ),
            Cmd::HistoryPane => spec(
                "panel.history",
                "history panel",
                icon::HISTORY,
                G::Panels,
                T::None,
                R::No,
                "show or hide it in the dock",
            ),
            Cmd::Timeline => spec(
                "panel.timeline",
                "timeline",
                icon::TIMELINE,
                G::Panels,
                T::None,
                R::No,
                "show or hide the timeline",
            ),
            Cmd::Numbers => spec(
                "panel.numbers",
                "numbers",
                icon::NUMBERS,
                G::Panels,
                T::None,
                R::No,
                "times and memory over the canvas: pen to GPU, frame, compose, steps, drops",
            ),
            Cmd::BrushWindow => spec(
                "panel.brush",
                "edit the brush",
                icon::BRUSH,
                G::Panels,
                T::Press(const { &[plain(Key::F5)] }),
                R::No,
                "tip, dual tip, texture, pressure and tilt, colour dynamics and wet paint, with a live sample; save it in this drawing",
            ),
            Cmd::Settings => spec(
                "panel.settings",
                "settings",
                icon::SETTINGS,
                G::Panels,
                T::Press(const { &[cmd(Key::Comma)] }),
                R::No,
                "pen calibration and the layout",
            ),
            Cmd::MessageLog => spec(
                "panel.log",
                "message log",
                icon::LOG,
                G::Panels,
                T::None,
                R::No,
                "the last 200 messages with their times",
            ),
            Cmd::Spin => spec(
                "numbers.spin",
                "spin the view",
                icon::TURN,
                G::Panels,
                T::None,
                R::No,
                "turn and zoom the view every frame to measure the frame rate",
            ),
            Cmd::ResetNumbers => spec(
                "numbers.reset",
                "reset the numbers",
                icon::NUMBERS,
                G::Panels,
                T::None,
                R::No,
                "start the measurements again",
            ),
            Cmd::FlipView => spec(
                "view.flip",
                "flip the view left-right",
                icon::FLIP_H,
                G::View,
                T::Press(const { &[plain(Key::H)] }),
                R::No,
                "see the drawing mirrored to check it; the document does not change",
            ),
            Cmd::ResetRotation => spec(
                "view.upright",
                "turn the view upright",
                icon::TURN,
                G::View,
                T::None,
                R::No,
                "rotation back to 0, about the middle of the window",
            ),
            Cmd::SwapColours => spec(
                "colour.swap",
                "swap the two colours",
                icon::SWAP,
                G::Colour,
                T::Press(const { &[plain(Key::X)] }),
                R::No,
                "the current colour and the one before it change places",
            ),
            Cmd::SizeDrag => spec(
                "brush.size-drag",
                "change the brush size on the canvas",
                icon::SIZE,
                G::Tools,
                T::Hold("hold cmd alt and drag sideways"),
                R::No,
                "the circle shows the size; let go to keep it",
            ),
            Cmd::TurnLeft90 => spec(
                "edit.turn-left",
                "turn the selection 90 degrees left",
                icon::TURN_LEFT,
                G::Edit,
                T::None,
                R::No,
                "about the middle of the selected strokes or the shape",
            ),
            Cmd::TurnRight90 => spec(
                "edit.turn-right",
                "turn the selection 90 degrees right",
                icon::TURN,
                G::Edit,
                T::None,
                R::No,
                "about the middle of the selected strokes or the shape",
            ),
            Cmd::DuplicateInPlace => spec(
                "edit.duplicate",
                "duplicate the selection in place",
                icon::COPY,
                G::Edit,
                T::Press(const { &[cmd(Key::D)] }),
                R::No,
                "a copy of the selected strokes or shape right on top, selected",
            ),
            Cmd::NudgeLeft => spec(
                "edit.nudge-left",
                "nudge the selection left",
                icon::LEFT,
                G::Edit,
                T::Press(const { &[symbol(Key::ArrowLeft)] }),
                R::Yes,
                "1 px, 10 px with shift; strokes, a shape or points",
            ),
            Cmd::NudgeRight => spec(
                "edit.nudge-right",
                "nudge the selection right",
                icon::RIGHT,
                G::Edit,
                T::Press(const { &[symbol(Key::ArrowRight)] }),
                R::Yes,
                "1 px, 10 px with shift; strokes, a shape or points",
            ),
            Cmd::NudgeUp => spec(
                "edit.nudge-up",
                "nudge the selection up",
                icon::UP,
                G::Edit,
                T::Press(const { &[symbol(Key::ArrowUp)] }),
                R::Yes,
                "1 px, 10 px with shift; strokes, a shape or points",
            ),
            Cmd::NewGroupLayer => spec(
                "layer.new-group",
                "new group",
                icon::LAYERS,
                G::Layers,
                T::None,
                R::No,
                "an empty group above the active layer; drag layers into it",
            ),
            Cmd::NewBonesLayer => spec(
                "layer.new-bones",
                "new bones layer",
                icon::BONE,
                G::Layers,
                T::None,
                R::No,
                "a skeleton of its own above the active layer: draw its bones with the bone tool, then attach layers to them. Bones are a preview: posing may still change",
            ),
            Cmd::DuplicateLayer => spec(
                "layer.duplicate",
                "duplicate the layer",
                icon::COPY,
                G::Layers,
                T::None,
                R::No,
                "a copy of the active layer with its drawings, links and mask, above it",
            ),
            Cmd::DeleteLayer => spec(
                "layer.delete",
                "delete the layer",
                icon::TRASH,
                G::Layers,
                T::None,
                R::No,
                "remove the active layer (cmd Z brings it back)",
            ),
            Cmd::GroupLayer => spec(
                "layer.group",
                "put the layer in a new group",
                icon::LAYERS,
                G::Layers,
                T::None,
                R::No,
                "a new group in the active layer's place, holding it",
            ),
            Cmd::RenameLayer => spec(
                "layer.rename",
                "rename the layer",
                icon::NOTE,
                G::Layers,
                T::None,
                R::No,
                "type the active layer's new name in its row",
            ),
            Cmd::MoveEarlier => spec(
                "drawing.earlier",
                "move the drawing a frame earlier",
                icon::LEFT,
                G::Frames,
                T::Press(const { &[alt(Key::ArrowLeft)] }),
                R::Yes,
                "drawings in the way are pushed along",
            ),
            Cmd::MoveLater => spec(
                "drawing.later",
                "move the drawing a frame later",
                icon::RIGHT,
                G::Frames,
                T::Press(const { &[alt(Key::ArrowRight)] }),
                R::Yes,
                "drawings in the way are pushed along",
            ),
            Cmd::CopyCells => spec(
                "cells.copy",
                "copy the selected cells",
                icon::COPY,
                G::Selected,
                T::None,
                R::No,
                "the drawings and holds of the selected frames and rows (or the drawing under the playhead)",
            ),
            Cmd::PasteCells => spec(
                "cells.paste",
                "paste cells as copies",
                icon::PASTE,
                G::Selected,
                T::None,
                R::No,
                "new drawings with the copied strokes, at the playhead, on the same layers",
            ),
            Cmd::PasteCellsLinked => spec(
                "cells.paste-links",
                "paste cells as links",
                icon::LINK,
                G::Selected,
                T::None,
                R::No,
                "show the copied drawings again at the playhead; drawing on them changes the originals",
            ),
            Cmd::ClipSettings => spec(
                "clip.settings",
                "clip settings",
                icon::CLIP_BAR,
                G::Clips,
                T::None,
                R::No,
                "name, loop, own fps, first and last frame, delete, export to Godot",
            ),
            Cmd::PickAllLayers => spec(
                "edit.pick-all",
                "pick from all layers",
                icon::LAYERS,
                G::Edit,
                T::None,
                R::No,
                "a click with S or V picks what you see on any layer; off, it picks on the active layer only",
            ),
            Cmd::FillShape => spec(
                "shape.fill",
                "fill the shape with the colour",
                icon::FILL,
                G::Edit,
                T::None,
                R::No,
                "the selected shape's fill becomes the current colour",
            ),
            Cmd::OutlineShape => spec(
                "shape.outline",
                "outline the shape in the colour",
                icon::PEN,
                G::Edit,
                T::None,
                R::No,
                "the selected shape's outline becomes the current colour (an outline is added at the pen's width when it has none)",
            ),
            Cmd::SoloLayer => spec(
                "layer.solo",
                "show only this layer",
                icon::EYE,
                G::Layers,
                T::None,
                R::No,
                "hide every other layer (alt click an eye does the same); again shows them as they were",
            ),
            Cmd::FirstSteps => spec(
                "help.first-steps",
                "first steps",
                icon::INFO,
                G::Panels,
                T::None,
                R::No,
                "three steps for someone new: draw a shape, fill it, hand the drawing to an agent",
            ),
            Cmd::CloseDocument => spec(
                "file.close",
                "close the drawing",
                icon::CLOSE,
                G::File,
                T::Press(const { &[cmd(Key::W)] }),
                R::No,
                "back to the start window; everything is saved as you go",
            ),
            Cmd::Quit => spec(
                "file.quit",
                "quit klad",
                icon::CLOSE,
                G::File,
                T::Press(const { &[cmd(Key::Q)] }),
                R::No,
                "close the window and quit; everything is saved as you go",
            ),
            Cmd::NewDocument => spec(
                "file.new",
                "new drawing",
                icon::FILE_PLUS,
                G::File,
                T::Press(const { &[cmd(Key::N)] }),
                R::No,
                "a new .klad bundle: name, folder, size, background; this one stays saved",
            ),
            Cmd::OpenDocument => spec(
                "file.open",
                "open a drawing",
                icon::FOLDER_OPEN,
                G::File,
                T::Press(const { &[cmd(Key::O)] }),
                R::No,
                "pick a .klad folder and switch to it; this one stays saved",
            ),
            Cmd::OpenRecent => spec(
                "file.recent",
                "open a recent drawing",
                icon::HISTORY,
                G::File,
                T::None,
                R::No,
                "the drawings opened lately, with thumbnails",
            ),
            Cmd::Export => spec(
                "file.export",
                "export",
                icon::EXPORT,
                G::File,
                T::Press(const { &[cmd(Key::E)] }),
                R::No,
                "export this frame, a layer, numbered frames or a sprite sheet as PNGs, as klad export does",
            ),
            Cmd::AddFont => spec(
                "file.font",
                "add a font",
                icon::PLUS,
                G::File,
                T::None,
                R::No,
                "copy a font file into the drawing so its text can use it",
            ),
            Cmd::Refill => spec(
                "colour.refill",
                "refill this drawing",
                icon::FILL,
                G::Colour,
                T::None,
                R::No,
                "make the flood fills of the active drawing again after its lines changed",
            ),
            Cmd::FillLike => spec(
                "colour.fill-like",
                "fill like the first drawing",
                icon::COPY,
                G::Colour,
                T::None,
                R::No,
                "carry the fills of the first selected drawing to the other selected frames",
            ),
            Cmd::ShowGaps => spec(
                "colour.gaps",
                "show gaps",
                icon::SEARCH,
                G::Colour,
                T::None,
                R::No,
                "outline the closed regions no fill covers on the active layer, on this frame or the selected frames",
            ),
            Cmd::ShowParts => spec(
                "view.parts",
                "show parts",
                icon::SHAPES,
                G::View,
                T::None,
                R::No,
                "box and name each part of the active drawing",
            ),
            Cmd::ShowArcs => spec(
                "view.arcs",
                "show arcs",
                icon::TURN,
                G::View,
                T::None,
                R::No,
                "draw the path the selected mark or part takes over the clip, a dot per frame",
            ),
            Cmd::About => spec(
                "file.about",
                "about klad and licences",
                icon::INFO,
                G::File,
                T::None,
                R::No,
                "the version, the licence and every open source part it is built on",
            ),
            Cmd::CompareReference => spec(
                "view.compare",
                "compare with the reference",
                icon::INFO,
                G::View,
                T::None,
                R::No,
                "measure this frame against the reference picture: values, saturation, texture, edges, and where they differ",
            ),
            Cmd::PaintOver => spec(
                "layer.paint-over",
                "paint over",
                icon::BRUSH,
                G::Layers,
                T::None,
                R::No,
                "repaint what is below this layer with brush strokes that follow its forms, and carry them through an animation",
            ),
            Cmd::ReportProblem => spec(
                "file.report",
                "report a problem",
                icon::LOG,
                G::File,
                T::None,
                R::No,
                "the version, the system and the last messages, to email to support or paste into an issue on GitHub",
            ),
            Cmd::ContactSupport => spec(
                "help.contact",
                "contact support",
                icon::NOTE,
                G::File,
                T::None,
                R::No,
                "an email to klad's support in your email app, with the version and your system filled in",
            ),
            Cmd::Guides => spec(
                "help.guides",
                "guides",
                icon::FILE,
                G::Panels,
                T::None,
                R::No,
                "klad's guides on klad.studio, in your browser: drawing, colour, animating, posing and exporting",
            ),
            Cmd::WhatsNew => spec(
                "help.whats-new",
                "what's new",
                icon::SPARKLE,
                G::Panels,
                T::None,
                R::No,
                "what changed in each version of klad, on klad.studio",
            ),
            Cmd::InstallCommand => spec(
                "file.install-command",
                "install the klad command",
                icon::DOWNLOAD,
                G::File,
                T::None,
                R::No,
                "puts klad on the terminal's path (klad install), for you and for agents",
            ),
            Cmd::CopyPath => spec(
                "file.copy-path",
                "copy this drawing's path",
                icon::COPY,
                G::File,
                T::None,
                R::No,
                "puts the .klad folder's path on the clipboard, to hand to the agent",
            ),
            Cmd::NudgeDown => spec(
                "edit.nudge-down",
                "nudge the selection down",
                icon::DOWN,
                G::Edit,
                T::Press(const { &[symbol(Key::ArrowDown)] }),
                R::Yes,
                "1 px, 10 px with shift; strokes, a shape or points",
            ),
        }
    }

    pub fn menu(self) -> &'static str {
        match self {
            Cmd::Brush => "Brush",
            Cmd::Eraser => "Eraser",
            Cmd::Fill => "Fill",
            Cmd::Pen => "Pen",
            Cmd::ShapeTool => "Rectangles and ellipses",
            Cmd::TextTool => "Text",
            Cmd::BoneTool => "Bones",
            Cmd::Symmetry => "Mirror painting",
            Cmd::MakeRepeat => "Repeat",
            Cmd::MakeScatter => "Scatter",
            Cmd::MakeMirror => "Mirror",
            Cmd::Unwrap => "Take shapes out",
            Cmd::ImportPicture => "Import picture…",
            Cmd::Shapes => "Select and move",
            Cmd::Strokes => "Select strokes",
            Cmd::Points => "Edit points",
            Cmd::Note => "Note",
            Cmd::Smaller => "Smaller brush",
            Cmd::Bigger => "Bigger brush",
            Cmd::Fit => "Fit",
            Cmd::ActualSize => "100%",
            Cmd::TurnRight => "Rotate right",
            Cmd::TurnLeft => "Rotate left",
            Cmd::Undo => "Undo",
            Cmd::Redo => "Redo",
            Cmd::Delete => "Delete",
            Cmd::Deselect => "Deselect",
            Cmd::Copy => "Copy",
            Cmd::Paste => "Paste",
            Cmd::SelectAll => "Select all",
            Cmd::TransformLayer => "Transform layer",
            Cmd::CutLasso => "Lasso cuts strokes",
            Cmd::Recolour => "Recolour",
            Cmd::Thinner => "Thinner",
            Cmd::Thicker => "Thicker",
            Cmd::AttachedOnly => "Attached only",
            Cmd::Detach => "Detach",
            Cmd::AlignLeft => "Left edges",
            Cmd::AlignCentre => "Horizontal centres",
            Cmd::AlignRight => "Right edges",
            Cmd::AlignTop => "Top edges",
            Cmd::AlignMiddle => "Vertical centres",
            Cmd::AlignBottom => "Bottom edges",
            Cmd::DistributeAcross => "Distribute across",
            Cmd::DistributeDown => "Distribute down",
            Cmd::FlipH => "Flip horizontal",
            Cmd::FlipV => "Flip vertical",
            Cmd::Corner => "Corner",
            Cmd::Smooth => "Smooth",
            Cmd::Simplify => "Simplify",
            Cmd::MakePath => "Make path",
            Cmd::AddSwatch => "Add colour to palette",
            Cmd::NewPaintLayer => "New paint layer",
            Cmd::NewPixelLayer => "New pixel layer",
            Cmd::NewVectorLayer => "New vector layer",
            Cmd::ToggleVisible => "Show",
            Cmd::ToggleLocked => "Lock",
            Cmd::ToggleClip => "Clip to layer below",
            Cmd::ToggleLockAlpha => "Lock alpha",
            Cmd::AddMask => "Add mask",
            Cmd::AddMaskHiding => "Add mask hiding all",
            Cmd::PaintMask => "Paint on mask",
            Cmd::BackToLayer => "Back to layer",
            Cmd::RemoveMask => "Remove mask",
            Cmd::ToggleMask => "Mask on",
            Cmd::TintMask => "Show mask in red",
            Cmd::OnionLayer => "This layer's onion",
            Cmd::NextFrame => "Next frame",
            Cmd::PrevFrame => "Previous frame",
            Cmd::NextDrawing => "Next drawing",
            Cmd::PrevDrawing => "Previous drawing",
            Cmd::FirstFrame => "First frame",
            Cmd::LastFrame => "Last frame",
            Cmd::NewDrawing => "New drawing",
            Cmd::NewDrawingAfter => "New drawing after",
            Cmd::Duplicate => "Duplicate to next frame",
            Cmd::Link => "Link a drawing here…",
            Cmd::Unlink => "Unlink",
            Cmd::DeleteDrawing => "Delete drawing",
            Cmd::HoldLonger => "Hold longer",
            Cmd::HoldShorter => "Hold shorter",
            Cmd::InsertFrame => "Insert frame",
            Cmd::InsertFrameAll => "Insert frame in every layer",
            Cmd::RemoveFrame => "Remove frame",
            Cmd::RemoveFrameAll => "Remove frame from every layer",
            Cmd::ExposeOnes => "Ones",
            Cmd::ExposeTwos => "Twos",
            Cmd::ExposeThrees => "Threes",
            Cmd::Play => "Play",
            Cmd::LoopRange => "Loop range",
            Cmd::Prepare => "Prepare frames",
            Cmd::Onion => "Onion skin",
            Cmd::OnionSettings => "Onion settings…",
            Cmd::OnOnes => "Retime on ones",
            Cmd::OnTwos => "Retime on twos",
            Cmd::OnThrees => "Retime on threes",
            Cmd::FitFrames => "Fit into n frames",
            Cmd::Reverse => "Reverse",
            Cmd::Pingpong => "Ping-pong",
            Cmd::EveryLayer => "On every layer",
            Cmd::Inbetween => "In-between…",
            Cmd::RedoInbetweens => "Redo in-betweens",
            Cmd::NewClip => "New clip",
            Cmd::DeleteClip => "Delete clip",
            Cmd::ClipLoops => "Loops",
            Cmd::ClipOwnFps => "Own fps",
            Cmd::ExportGodot => "Export to Godot",
            Cmd::Palette => "Find a command…",
            Cmd::Keys => "Keyboard shortcuts",
            Cmd::HideAll => "Hide panels",
            Cmd::HideDock => "Hide dock",
            Cmd::LayersPane => "Layers",
            Cmd::ToolPane => "Colour",
            Cmd::BrushPane => "Brush",
            Cmd::InspectorPane => "Layer settings",
            Cmd::NotesPane => "Notes",
            Cmd::HistoryPane => "History",
            Cmd::Timeline => "Timeline",
            Cmd::Numbers => "Show numbers",
            Cmd::Settings => "Settings…",
            Cmd::BrushWindow => "Edit brush…",
            Cmd::MessageLog => "Message log",
            Cmd::Spin => "Spin the view",
            Cmd::ResetNumbers => "Reset numbers",
            Cmd::FlipView => "Flip",
            Cmd::ResetRotation => "Upright",
            Cmd::SwapColours => "Swap colours",
            Cmd::TurnLeft90 => "Rotate 90° left",
            Cmd::TurnRight90 => "Rotate 90° right",
            Cmd::DuplicateInPlace => "Duplicate",
            Cmd::NudgeLeft => "Nudge left",
            Cmd::NudgeRight => "Nudge right",
            Cmd::NudgeUp => "Nudge up",
            Cmd::NudgeDown => "Nudge down",
            Cmd::NewGroupLayer => "New group",
            Cmd::NewBonesLayer => "New bones layer",
            Cmd::NewAdjustLayer => "New adjustment layer",
            Cmd::DuplicateLayer => "Duplicate",
            Cmd::DeleteLayer => "Delete",
            Cmd::GroupLayer => "Group",
            Cmd::RenameLayer => "Rename",
            Cmd::MoveEarlier => "Move earlier",
            Cmd::MoveLater => "Move later",
            Cmd::CopyCells => "Copy cells",
            Cmd::PasteCells => "Paste cells",
            Cmd::PasteCellsLinked => "Paste cells as links",
            Cmd::ClipSettings => "Clip settings…",
            Cmd::NewDocument => "New…",
            Cmd::OpenDocument => "Open…",
            Cmd::OpenRecent => "All recent drawings…",
            Cmd::CopyPath => "Copy path",
            Cmd::InstallCommand => "Install the klad command",
            Cmd::ReportProblem => "Report a problem…",
            Cmd::ContactSupport => "Contact support",
            Cmd::Guides => "Guides",
            Cmd::WhatsNew => "What's new",
            Cmd::PaintOver => "Paint over…",
            Cmd::CompareReference => "Compare with reference",
            Cmd::About => "About klad and licences",
            Cmd::Export => "Export…",
            Cmd::AddFont => "Add font…",
            Cmd::Refill => "Refill drawing",
            Cmd::FillLike => "Fill like first drawing",
            Cmd::ShowGaps => "Show gaps",
            Cmd::ShowParts => "Show parts",
            Cmd::ShowArcs => "Show arcs",
            Cmd::CloseDocument => "Close",
            Cmd::Quit => "Quit klad",
            Cmd::FirstSteps => "First steps",
            Cmd::PickAllLayers => "Pick from all layers",
            Cmd::FillShape => "Fill with colour",
            Cmd::OutlineShape => "Outline with colour",
            Cmd::SoloLayer => "Show only this",
            Cmd::Parallax | Cmd::Eyedropper | Cmd::Pan | Cmd::Zoom | Cmd::FlipFrames | Cmd::Roll | Cmd::SizeDrag => {
                self.spec().name
            }
        }
    }

    pub fn ticks(self) -> bool {
        !self.menu().ends_with('…')
    }

    pub fn find(id: &str) -> Option<Cmd> {
        Cmd::ALL.into_iter().find(|c| c.spec().id == id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Run(Cmd),
    Line,
    Sub(&'static str, &'static [Item]),
    Recent,
}

#[derive(Clone, Copy, Debug)]
pub struct Menu {
    pub title: &'static str,
    pub items: &'static [Item],
}

use Item::{Line, Run, Sub};

pub const MENUS: [Menu; 9] = [
    Menu {
        title: "File",
        items: &[
            Run(Cmd::NewDocument),
            Run(Cmd::OpenDocument),
            Item::Recent,
            Run(Cmd::CloseDocument),
            Line,
            Run(Cmd::ImportPicture),
            Run(Cmd::AddFont),
            Line,
            Run(Cmd::Export),
            Run(Cmd::ExportGodot),
            Line,
            Run(Cmd::CopyPath),
            Run(Cmd::InstallCommand),
            Line,
            Run(Cmd::Quit),
        ],
    },
    Menu {
        title: "Edit",
        items: &[
            Run(Cmd::Undo),
            Run(Cmd::Redo),
            Line,
            Run(Cmd::Copy),
            Run(Cmd::Paste),
            Run(Cmd::DuplicateInPlace),
            Run(Cmd::Delete),
            Line,
            Run(Cmd::SelectAll),
            Run(Cmd::Deselect),
            Run(Cmd::CutLasso),
            Run(Cmd::PickAllLayers),
            Line,
            Sub(
                "Transform",
                &[
                    Run(Cmd::TransformLayer),
                    Line,
                    Run(Cmd::FlipH),
                    Run(Cmd::FlipV),
                    Run(Cmd::TurnLeft90),
                    Run(Cmd::TurnRight90),
                    Line,
                    Run(Cmd::NudgeLeft),
                    Run(Cmd::NudgeRight),
                    Run(Cmd::NudgeUp),
                    Run(Cmd::NudgeDown),
                ],
            ),
            Sub(
                "Align",
                &[
                    Run(Cmd::AlignLeft),
                    Run(Cmd::AlignCentre),
                    Run(Cmd::AlignRight),
                    Line,
                    Run(Cmd::AlignTop),
                    Run(Cmd::AlignMiddle),
                    Run(Cmd::AlignBottom),
                    Line,
                    Run(Cmd::DistributeAcross),
                    Run(Cmd::DistributeDown),
                ],
            ),
            Sub(
                "Strokes",
                &[
                    Run(Cmd::Recolour),
                    Run(Cmd::Thinner),
                    Run(Cmd::Thicker),
                    Line,
                    Run(Cmd::AttachedOnly),
                    Run(Cmd::Detach),
                ],
            ),
            Sub("Shape", &[Run(Cmd::FillShape), Run(Cmd::OutlineShape)]),
            Sub(
                "Points",
                &[
                    Run(Cmd::Corner),
                    Run(Cmd::Smooth),
                    Run(Cmd::Simplify),
                    Run(Cmd::MakePath),
                ],
            ),
            Line,
            Run(Cmd::Settings),
        ],
    },
    Menu {
        title: "Tools",
        items: &[
            Run(Cmd::Brush),
            Run(Cmd::Eraser),
            Run(Cmd::Fill),
            Run(Cmd::Shapes),
            Run(Cmd::Strokes),
            Run(Cmd::Points),
            Run(Cmd::Pen),
            Run(Cmd::ShapeTool),
            Run(Cmd::TextTool),
            Run(Cmd::BoneTool),
            Run(Cmd::Note),
            Line,
            Run(Cmd::Smaller),
            Run(Cmd::Bigger),
            Run(Cmd::Symmetry),
            Run(Cmd::BrushWindow),
            Line,
            Run(Cmd::SwapColours),
            Run(Cmd::AddSwatch),
        ],
    },
    Menu {
        title: "Layer",
        items: &[
            Run(Cmd::NewPaintLayer),
            Run(Cmd::NewPixelLayer),
            Run(Cmd::NewVectorLayer),
            Run(Cmd::NewAdjustLayer),
            Run(Cmd::NewGroupLayer),
            Run(Cmd::NewBonesLayer),
            Line,
            Run(Cmd::DuplicateLayer),
            Run(Cmd::RenameLayer),
            Run(Cmd::GroupLayer),
            Run(Cmd::DeleteLayer),
            Line,
            Run(Cmd::ToggleVisible),
            Run(Cmd::SoloLayer),
            Run(Cmd::ToggleLocked),
            Run(Cmd::ToggleClip),
            Run(Cmd::ToggleLockAlpha),
            Line,
            Sub(
                "Mask",
                &[
                    Run(Cmd::AddMask),
                    Run(Cmd::AddMaskHiding),
                    Run(Cmd::PaintMask),
                    Run(Cmd::BackToLayer),
                    Run(Cmd::ToggleMask),
                    Run(Cmd::TintMask),
                    Run(Cmd::RemoveMask),
                ],
            ),
            Sub(
                "Shape generators",
                &[
                    Run(Cmd::MakeRepeat),
                    Run(Cmd::MakeScatter),
                    Run(Cmd::MakeMirror),
                    Run(Cmd::Unwrap),
                ],
            ),
            Run(Cmd::PaintOver),
            Line,
            Run(Cmd::Refill),
            Run(Cmd::ShowGaps),
        ],
    },
    Menu {
        title: "Frame",
        items: &[
            Run(Cmd::FirstFrame),
            Run(Cmd::PrevFrame),
            Run(Cmd::NextFrame),
            Run(Cmd::LastFrame),
            Run(Cmd::PrevDrawing),
            Run(Cmd::NextDrawing),
            Line,
            Run(Cmd::NewDrawing),
            Run(Cmd::NewDrawingAfter),
            Run(Cmd::Duplicate),
            Run(Cmd::Link),
            Run(Cmd::Unlink),
            Run(Cmd::DeleteDrawing),
            Line,
            Run(Cmd::HoldLonger),
            Run(Cmd::HoldShorter),
            Run(Cmd::MoveEarlier),
            Run(Cmd::MoveLater),
            Line,
            Run(Cmd::InsertFrame),
            Run(Cmd::InsertFrameAll),
            Run(Cmd::RemoveFrame),
            Run(Cmd::RemoveFrameAll),
        ],
    },
    Menu {
        title: "Animation",
        items: &[
            Run(Cmd::Play),
            Run(Cmd::LoopRange),
            Run(Cmd::Prepare),
            Line,
            Run(Cmd::Onion),
            Run(Cmd::OnionSettings),
            Run(Cmd::OnionLayer),
            Line,
            Sub(
                "New drawings on",
                &[Run(Cmd::ExposeOnes), Run(Cmd::ExposeTwos), Run(Cmd::ExposeThrees)],
            ),
            Sub(
                "Selected frames",
                &[
                    Run(Cmd::OnOnes),
                    Run(Cmd::OnTwos),
                    Run(Cmd::OnThrees),
                    Run(Cmd::FitFrames),
                    Run(Cmd::Reverse),
                    Run(Cmd::Pingpong),
                    Run(Cmd::EveryLayer),
                    Line,
                    Run(Cmd::Inbetween),
                    Run(Cmd::RedoInbetweens),
                    Line,
                    Run(Cmd::CopyCells),
                    Run(Cmd::PasteCells),
                    Run(Cmd::PasteCellsLinked),
                ],
            ),
            Sub(
                "Clip",
                &[
                    Run(Cmd::NewClip),
                    Run(Cmd::ClipSettings),
                    Run(Cmd::ClipLoops),
                    Run(Cmd::ClipOwnFps),
                    Run(Cmd::DeleteClip),
                ],
            ),
            Line,
            Run(Cmd::FillLike),
            Run(Cmd::ShowParts),
            Run(Cmd::ShowArcs),
        ],
    },
    Menu {
        title: "View",
        items: &[
            Run(Cmd::Fit),
            Run(Cmd::ActualSize),
            Line,
            Run(Cmd::TurnRight),
            Run(Cmd::TurnLeft),
            Run(Cmd::ResetRotation),
            Run(Cmd::FlipView),
            Line,
            Run(Cmd::CompareReference),
        ],
    },
    Menu {
        title: "Window",
        items: &[
            Run(Cmd::HideAll),
            Run(Cmd::HideDock),
            Line,
            Run(Cmd::ToolPane),
            Run(Cmd::BrushPane),
            Run(Cmd::LayersPane),
            Run(Cmd::InspectorPane),
            Run(Cmd::NotesPane),
            Run(Cmd::HistoryPane),
            Run(Cmd::Timeline),
            Line,
            Run(Cmd::MessageLog),
        ],
    },
    Menu {
        title: "Help",
        items: &[
            Run(Cmd::FirstSteps),
            Run(Cmd::Guides),
            Run(Cmd::WhatsNew),
            Run(Cmd::Palette),
            Run(Cmd::Keys),
            Line,
            Run(Cmd::ContactSupport),
            Run(Cmd::ReportProblem),
            Run(Cmd::About),
        ],
    },
];

pub const DEVELOPER: [Cmd; 3] = [Cmd::Numbers, Cmd::ResetNumbers, Cmd::Spin];

#[cfg(any(target_os = "macos", test))]
pub const IN_APP_MENU: [Cmd; 2] = [Cmd::Settings, Cmd::Quit];

pub fn menu_label(cmd: Cmd) -> &'static str {
    cmd.menu()
}

pub fn menu_hint(menu: &Menu) -> String {
    let names: Vec<&str> = in_menus(menu.items).iter().take(4).map(|c| c.menu()).collect();
    format!("{}: {}, ...", menu.title, names.join(", "))
}

pub fn in_menus(items: &[Item]) -> Vec<Cmd> {
    items
        .iter()
        .flat_map(|i| match i {
            Item::Run(c) => vec![*c],
            Item::Sub(_, inner) => in_menus(inner),
            Item::Recent => vec![Cmd::OpenRecent],
            Item::Line => Vec::new(),
        })
        .collect()
}

fn squash(text: &str) -> String {
    text.to_lowercase().chars().filter(|c| *c != '-').collect()
}

fn subsequence(needle: &str, hay: &str) -> bool {
    let mut rest = hay.chars();
    needle.chars().all(|n| rest.any(|h| h == n))
}

pub fn score(query: &str, cmd: Cmd) -> Option<u32> {
    let q = squash(query.trim());
    if q.is_empty() {
        return Some(5);
    }
    let s = cmd.spec();
    let name = squash(s.name);
    let words: Vec<&str> = name.split_whitespace().collect();
    let key = s.key_label().to_lowercase();
    if name.starts_with(&q) {
        return Some(0);
    }
    if key == q || key.split(" or ").any(|k| k == q) {
        return Some(1);
    }
    if words.iter().any(|w| w.starts_with(&q)) {
        return Some(2);
    }
    if name.contains(&q) {
        return Some(3);
    }
    if subsequence(&q, &name) {
        return Some(4);
    }
    squash(s.group.name()).contains(&q).then_some(6)
}

pub fn rank(query: &str, recent: &[Cmd]) -> Vec<Cmd> {
    let mut found: Vec<(u32, usize, usize, Cmd)> = Cmd::ALL
        .into_iter()
        .enumerate()
        .filter(|(i, c)| Cmd::ALL[..*i].iter().all(|o| o != c))
        .filter_map(|(i, c)| {
            let used = recent.iter().position(|r| *r == c).unwrap_or(usize::MAX);
            score(query, c).map(|s| (s, used, i, c))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, _, _, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const INVENTORY: &[(&str, &str)] = &[
        ("brush tool, B (M0)", "tool.brush"),
        ("eraser, E (M2 batch 1)", "tool.eraser"),
        ("fill tool, G (M2 batch 2)", "tool.fill"),
        ("select shapes, V (M1 batch 3)", "tool.shapes"),
        ("stroke select, S (M2 batch 3)", "tool.strokes"),
        ("direct select, A (M2 batch 3)", "tool.points"),
        ("pen, P (M4 test round 1)", "tool.pen"),
        ("prepare frames (M4 test round 1)", "play.prepare"),
        ("brush window, F5 (M5 batch 1)", "panel.brush"),
        ("pin a note, N (M1 batch 3)", "tool.note"),
        ("eyedropper, alt click (M2 batch 2)", "colour.pick"),
        ("brush size, [ and ] (M0)", "brush.smaller"),
        ("brush size up", "brush.bigger"),
        ("pan, space or middle drag (M0)", "view.pan"),
        ("zoom, pinch or cmd scroll (M0)", "view.zoom"),
        ("fit, F (M0)", "view.fit"),
        ("100%, 1 (M0)", "view.100"),
        ("rotate the view, R and shift R (M0)", "view.turn-right"),
        ("rotate the view left", "view.turn-left"),
        ("undo, cmd Z (M1 batch 3)", "edit.undo"),
        ("redo, shift cmd Z (M1 batch 3)", "edit.redo"),
        ("delete the selection (M1 batch 3)", "edit.delete"),
        ("clear the selection, Esc", "edit.deselect"),
        ("copy strokes, cmd C (M3 batch 4)", "edit.copy"),
        ("paste strokes, cmd V (M3 batch 4)", "edit.paste"),
        ("select all on this layer (M2 batch 3)", "edit.select-all"),
        (
            "transform a whole layer or group, cmd T (2026-11-08)",
            "edit.transform-layer",
        ),
        ("a lasso cuts strokes at its edge (M2 batch 4)", "edit.cut-lasso"),
        ("recolour (M2 batch 3)", "edit.recolour"),
        ("thinner (M2 batch 3)", "edit.thinner"),
        ("thicker (M2 batch 3)", "edit.thicker"),
        ("flip left-right (M2 batch 4)", "edit.flip-h"),
        ("flip up-down (M2 batch 4)", "edit.flip-v"),
        ("corner points (M2 batch 3)", "points.corner"),
        ("smooth points (M2 batch 3)", "points.smooth"),
        ("simplify (M2 batch 3)", "points.simplify"),
        ("make it a path (M2 batch 3)", "points.make-path"),
        ("add a palette colour (M2 batch 2)", "colour.add"),
        ("+ paint layer (M3 batch 1)", "layer.new-paint"),
        ("+ vector layer (M3 batch 1)", "layer.new-vector"),
        ("layer visibility (M1 batch 3)", "layer.visible"),
        ("layer locked (M1 batch 3)", "layer.locked"),
        ("clip to the layer below (M2 batch 4)", "layer.clip"),
        ("lock alpha (M2 batch 4)", "layer.lock-alpha"),
        ("add mask (M2 batch 4)", "mask.add"),
        ("add mask hiding all (M2 batch 4)", "mask.add-hiding"),
        ("paint on the mask (M2 batch 4)", "mask.paint"),
        ("back to the layer (M2 batch 4)", "mask.back"),
        ("remove mask (M2 batch 4)", "mask.remove"),
        ("mask on (M2 batch 4)", "mask.on"),
        ("show the mask in red (M2 batch 4)", "mask.tint"),
        ("a layer's onion dot (M3 batch 2)", "onion.layer"),
        ("step a frame, . and , (M3 batch 1)", "frame.next"),
        ("step back", "frame.previous"),
        ("next drawing, alt . (M3 batch 1)", "frame.next-drawing"),
        ("previous drawing, alt , (M3 batch 1)", "frame.previous-drawing"),
        ("first frame, Home (M3 batch 1)", "frame.first"),
        ("last frame, End (M3 batch 1)", "frame.last"),
        ("new drawing, K (M3 batch 1)", "drawing.new"),
        ("new drawing after this one, shift K (M3 batch 2)", "drawing.new-after"),
        ("duplicate to the next frame (M3 batch 1)", "drawing.duplicate"),
        ("show a drawing here, link (M3 batch 1)", "drawing.link"),
        ("unlink (M3 batch 1)", "drawing.unlink"),
        ("delete a drawing (M3 batch 1)", "drawing.delete"),
        ("hold longer, = (M3 batch 1)", "drawing.longer"),
        ("hold shorter, - (M3 batch 1)", "drawing.shorter"),
        ("insert a frame, I (M3 batch 1)", "frames.insert"),
        (
            "insert a frame in every layer, shift I (M3 batch 1)",
            "frames.insert-all",
        ),
        ("remove a frame (M3 batch 1)", "frames.remove"),
        ("remove a frame from every layer (M3 batch 1)", "frames.remove-all"),
        ("shift K exposes on ones (M3 batch 2)", "expose.ones"),
        ("on twos", "expose.twos"),
        ("on threes", "expose.threes"),
        ("flip, hold Q (M3 batch 2)", "frame.flip"),
        ("roll, hold W (M3 batch 2)", "frame.roll"),
        ("play and stop, Return (M3 batch 2)", "play.toggle"),
        ("loop range (M3 batch 2)", "play.loop"),
        ("onion skin, O (M3 batch 2)", "onion.toggle"),
        ("onion settings (M3 batch 2)", "onion.settings"),
        ("retime on ones (M3 batch 4)", "span.ones"),
        ("retime on twos (M3 batch 4)", "span.twos"),
        ("retime on threes (M3 batch 4)", "span.threes"),
        ("fit into n frames (M3 batch 4)", "span.fit"),
        ("reverse (M3 batch 4)", "span.reverse"),
        ("ping-pong (M3 batch 4)", "span.pingpong"),
        ("retime on every layer (M3 batch 4)", "span.every-layer"),
        ("make the in-betweens (M3 batch 4)", "span.inbetween"),
        ("redo the in-betweens (M3 batch 4)", "span.redo"),
        ("new clip, a drag on the strip (M3 batch 3)", "clip.new"),
        ("delete clip (M3 batch 3)", "clip.delete"),
        ("clip loops (M3 batch 3)", "clip.loops"),
        ("clip own fps (M3 batch 3)", "clip.fps"),
        ("export to Godot (M3 batch 3)", "clip.godot"),
        ("pen calibration (M2 batch 1)", "panel.settings"),
        ("pen and performance numbers (M0)", "panel.numbers"),
        ("spin, the fps test (M0)", "numbers.spin"),
        ("reset numbers (M0)", "numbers.reset"),
    ];

    #[test]
    fn every_command_is_in_a_menu_once_but_the_holds() {
        let listed: Vec<Cmd> = MENUS.iter().flat_map(|m| in_menus(m.items)).collect();
        let twice: Vec<&Cmd> = listed
            .iter()
            .enumerate()
            .filter(|(i, c)| listed[..*i].contains(c))
            .map(|(_, c)| c)
            .collect();
        assert!(twice.is_empty(), "in two places: {twice:?}");
        let missing: Vec<Cmd> = Cmd::ALL
            .into_iter()
            .filter(|c| !listed.contains(c) && !c.spec().is_hold() && !DEVELOPER.contains(c))
            .collect();
        assert!(missing.is_empty(), "not in any menu: {missing:?}");
        assert!(
            DEVELOPER.iter().all(|c| !listed.contains(c)),
            "developer tools stay out of the menus"
        );
        assert!(
            listed.iter().all(|c| !c.spec().is_hold()),
            "holds are gestures, not menu items"
        );
        assert!(IN_APP_MENU.iter().all(|c| listed.contains(c)));
        assert_eq!(menu_label(Cmd::NewDocument), "New…");
        assert_eq!(Bindings::default().menu_key(Cmd::Delete), "Backspace");
        assert_eq!(Bindings::default().menu_key(Cmd::Redo), "shift cmd Z");
    }

    #[test]
    fn menu_labels_are_short_and_never_twice_in_one_menu() {
        fn check(title: &str, items: &[Item]) {
            let mut seen: Vec<&str> = Vec::new();
            items.iter().for_each(|item| {
                let label = match item {
                    Item::Run(cmd) => cmd.menu(),
                    Item::Sub(name, inner) => {
                        check(name, inner);
                        name
                    }
                    Item::Recent => "Open recent",
                    Item::Line => return,
                };
                assert!(!seen.contains(&label), "{label} twice in {title}");
                assert!(label.chars().count() <= 30, "{label} in {title} is long for a menu");
                seen.push(label);
            });
            assert!(
                items.first() != Some(&Item::Line) && items.last() != Some(&Item::Line),
                "{title} starts or ends with a line"
            );
        }
        MENUS.iter().for_each(|m| check(m.title, m.items));
        let titles: Vec<&str> = MENUS.iter().map(|m| m.title).collect();
        assert_eq!(
            titles,
            [
                "File",
                "Edit",
                "Tools",
                "Layer",
                "Frame",
                "Animation",
                "View",
                "Window",
                "Help"
            ]
        );
        [
            Cmd::Export,
            Cmd::Settings,
            Cmd::BrushWindow,
            Cmd::ReportProblem,
            Cmd::Link,
        ]
        .iter()
        .for_each(|c| assert!(c.menu().ends_with('…'), "{c:?} opens a window"));
    }

    #[test]
    fn every_editor_action_of_m1_to_m3_is_a_named_command() {
        let missing: Vec<&str> = INVENTORY
            .iter()
            .filter(|(_, id)| Cmd::find(id).is_none())
            .map(|(what, _)| *what)
            .collect();
        assert!(missing.is_empty(), "no command for {missing:?}");
        let unique: BTreeSet<Cmd> = Cmd::ALL.into_iter().collect();
        let ids: BTreeSet<&str> = unique.iter().map(|c| c.spec().id).collect();
        assert_eq!(ids.len(), unique.len(), "two commands share an id");
        unique.iter().for_each(|c| {
            let s = c.spec();
            assert!(
                !s.name.is_empty() && !s.about.is_empty() && !s.icon.is_empty(),
                "{c:?} needs a name, words and an icon"
            );
        });
    }

    #[test]
    fn keys_rebind_refuse_a_key_in_use_and_survive_the_settings_file() {
        Cmd::ALL.into_iter().for_each(|c| {
            if let Trigger::Press(list) = c.spec().trigger {
                list.iter().filter(|k| !k.any_shift).for_each(|k| {
                    assert_eq!(Keys::parse(&k.label()), Some(*k), "{}", k.label());
                });
            }
        });
        let mut b = Bindings::default();
        let none = Modifiers::NONE;
        assert_eq!(b.for_key(Key::B, none), Some(Cmd::Brush));
        let taken = b.bind(Cmd::DuplicateLayer, Keys::parse("B").unwrap()).unwrap_err();
        assert!(taken.contains("brush"), "{taken}");
        assert!(
            b.bind(Cmd::DuplicateLayer, Keys::parse("Q").unwrap())
                .unwrap_err()
                .contains("hold Q")
        );
        assert!(b.bind(Cmd::FlipFrames, Keys::parse("F9").unwrap()).is_err());
        b.bind(Cmd::DuplicateLayer, Keys::parse("shift D").unwrap()).unwrap();
        let shift = Modifiers { shift: true, ..none };
        assert_eq!(b.for_key(Key::D, shift), Some(Cmd::DuplicateLayer));
        assert_eq!(b.label(Cmd::DuplicateLayer), "shift D");
        b.bind(Cmd::Brush, Keys::parse("Y").unwrap()).unwrap();
        assert_eq!(b.for_key(Key::B, none), None);
        assert_eq!(b.for_key(Key::Y, none), Some(Cmd::Brush));
        b.bind(Cmd::DuplicateLayer, Keys::parse("B").unwrap()).unwrap();
        let saved = b.to_settings();
        assert_eq!(saved["layer.duplicate"], ["B"]);
        let back = Bindings::from_settings(&saved);
        assert_eq!(back, b);
        let mut back = back;
        back.reset(Cmd::Brush);
        assert!(back.bind(Cmd::Brush, Keys::parse("B").unwrap()).is_err());
        back.reset_all();
        assert_eq!(back.for_key(Key::B, none), Some(Cmd::Brush));
        assert!(Keys::parse("hyper B").is_none());
    }

    #[test]
    fn no_key_runs_two_commands() {
        let bound: Vec<(Keys, Cmd)> = Cmd::ALL
            .into_iter()
            .flat_map(|c| match c.spec().trigger {
                Trigger::Press(list) => list.iter().map(|k| (*k, c)).collect(),
                _ => Vec::new(),
            })
            .collect();
        bound.iter().for_each(|(k, c)| {
            let mods = |shift| Modifiers {
                shift,
                alt: k.alt,
                command: k.command,
                mac_cmd: k.command,
                ctrl: false,
            };
            [k.shift, !k.shift]
                .into_iter()
                .filter(|shift| *shift == k.shift || k.any_shift)
                .for_each(|shift| {
                    let runs: BTreeSet<Cmd> = bound
                        .iter()
                        .filter(|(o, _)| o.matches(k.key, mods(shift)))
                        .map(|(_, c)| *c)
                        .collect();
                    assert_eq!(runs.len(), 1, "{} runs {runs:?}", k.label());
                    assert_eq!(Bindings::default().for_key(k.key, mods(shift)), Some(*c));
                });
        });
        let shifted = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        assert_eq!(
            Bindings::default().for_key(Key::K, Modifiers::NONE),
            Some(Cmd::NewDrawing)
        );
        assert_eq!(Bindings::default().for_key(Key::K, shifted), Some(Cmd::NewDrawingAfter));
        assert_eq!(Bindings::default().for_key(Key::Plus, shifted), Some(Cmd::HoldLonger));
        assert_eq!(Bindings::default().for_key(Key::Z, Modifiers::COMMAND), Some(Cmd::Undo));
        assert_eq!(
            Bindings::default().for_key(Key::Comma, Modifiers::COMMAND),
            Some(Cmd::Settings)
        );
        assert_eq!(
            Bindings::default().for_key(Key::Comma, Modifiers::NONE),
            Some(Cmd::PrevFrame)
        );
        assert_eq!(Bindings::default().for_key(Key::Q, Modifiers::NONE), None);
    }

    #[test]
    fn the_palette_finds_what_is_typed_first() {
        assert_eq!(rank("inb", &[])[0], Cmd::Inbetween);
        assert_eq!(rank("in-b", &[])[0], Cmd::Inbetween);
        assert_eq!(rank("undo", &[])[0], Cmd::Undo);
        assert_eq!(rank("shift k", &[])[0], Cmd::NewDrawingAfter);
        assert_eq!(rank("onion", &[])[..2], [Cmd::Onion, Cmd::OnionSettings]);
        assert!(rank("ping", &[]).starts_with(&[Cmd::Pingpong]));
        assert!(rank("godot", &[]).contains(&Cmd::ExportGodot));
        assert!(rank("zzqx", &[]).is_empty());
        let all = rank("", &[Cmd::ExportGodot, Cmd::Play]);
        assert_eq!(all[..2], [Cmd::ExportGodot, Cmd::Play]);
        assert_eq!(all.len(), Cmd::ALL.iter().collect::<BTreeSet<_>>().len());
        assert_eq!(rank("new", &[Cmd::NewClip])[0], Cmd::NewClip);
    }

    #[test]
    fn keys_read_the_way_the_docs_write_them() {
        assert_eq!(Cmd::Redo.spec().key_label(), "shift cmd Z");
        assert_eq!(Cmd::NextDrawing.spec().key_label(), "alt .");
        assert_eq!(Cmd::Delete.spec().key_label(), "Delete or Backspace");
        assert_eq!(Cmd::Play.spec().key_label(), "Return");
        assert_eq!(
            Cmd::Pan.spec().hint(),
            "pan: hold space and drag, or drag with the middle button\nmove the view"
        );
        assert_eq!(
            Cmd::Unlink.spec().hint(),
            "unlink\nthe link at this frame becomes a drawing of its own"
        );
        assert_eq!(
            Cmd::NewDrawing.spec().hint(),
            "new drawing here (K)\na blank drawing at this frame on the active layer"
        );
    }
}
