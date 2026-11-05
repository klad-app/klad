use super::selection::Selected;
use super::tweens::{self, SpanAction};
use super::{Editor, Tool};
use crate::commands::{Cmd, Repeat};
use crate::jobs::{Spec, Tag};
use crate::layout::Pane;
use crate::messages::Tone;
use crate::stats::{Stats, lock};
use crate::theme::{self, Hint};
use crate::view::View;
use anyhow::Result;
use egui::RichText;
use klad_core::doc::{Kind as LayerKind, MASK, Shape};
use klad_core::history::Stack;
use klad_core::markedit::LassoAction;
use klad_core::ops::{ClipEdit, Op, PaletteEdit};
use klad_core::pathedit::PathEdit;
use klad_core::retime::Retime;
use klad_core::timing::OPEN;
use serde_json::{Map, json};
use std::collections::BTreeSet;

const RECENT: usize = 12;

impl Editor {
    fn drawing_here(&self) -> Option<(klad_core::doc::Cel, u32)> {
        self.here().and_then(|(_, c)| c)
    }

    pub(super) fn active_paint(&self) -> bool {
        self.scene.active_layer().is_some_and(|l| l.kind() == LayerKind::Paint)
    }

    fn timeline_edit(&self) -> Option<String> {
        if self.timeline.player.is_some() {
            return Some("stop playback first (Return)".into());
        }
        if self.holding_frame_keys() {
            return Some("let go of Q or W first".into());
        }
        if self.busy() {
            return Some("finish the stroke or drag first".into());
        }
        None
    }

    pub(super) fn can_run(&self, cmd: Cmd) -> Option<String> {
        let spec = cmd.spec();
        if spec.is_hold() {
            return Some(format!("{}: {}", spec.name, self.bindings.label(cmd)));
        }
        let layer = self.scene.active_layer();
        let pick_layer = || Some("pick a layer in the timeline or the layers panel first".to_string());
        let mask = layer.and_then(|l| l.mask.as_deref());
        let picked = self.picked.is_some() && self.on_marks();
        let need_strokes = || (!picked).then(|| "select strokes with S first".to_string());
        let direct = self.direct.as_ref();
        let span = self.timeline.span.as_ref();
        let need_span = || {
            span.is_none()
                .then(|| "select frames first: shift click or shift drag over cells".to_string())
        };
        let clip = self.current_clip();
        let need_clip = || {
            clip.is_none()
                .then(|| "no clip here: click a clip's bar above the frame numbers".to_string())
        };
        let editing = self.busy().then(|| "finish the stroke or drag first".to_string());
        match cmd {
            Cmd::Undo | Cmd::Redo => editing,
            Cmd::Delete => match self.tool {
                Tool::Note => self
                    .note_open
                    .is_none()
                    .then(|| "right click a note's pin with N to select it first".to_string()),
                Tool::Strokes => need_strokes(),
                _ if picked => None,
                Tool::Direct => direct
                    .filter(|d| !d.points.is_empty())
                    .is_none()
                    .then(|| "select points with A first".to_string()),
                _ => self
                    .selection
                    .is_none()
                    .then(|| "select a shape or strokes with V first".to_string()),
            }
            .or(editing),
            Cmd::Copy => need_strokes(),
            Cmd::Paste => match (&self.clipboard, self.drag.is_some()) {
                (None, _) => Some("copy strokes first: S, select them, cmd C".into()),
                (_, true) => Some("finish the drag first".into()),
                _ => None,
            },
            Cmd::SelectAll => (!self.active_paint() || self.scene.active_marks().is_empty())
                .then(|| "the active layer has no strokes on this frame".to_string()),
            Cmd::Recolour | Cmd::Thinner | Cmd::Thicker => need_strokes().or(editing),
            Cmd::FlipH | Cmd::FlipV | Cmd::TurnLeft90 | Cmd::TurnRight90 | Cmd::DuplicateInPlace => {
                match self.selected() {
                    Some(Selected::Strokes | Selected::Shape(_)) => editing,
                    _ => Some("select strokes (S) or a shape (V) first".to_string()),
                }
            }
            Cmd::NudgeLeft | Cmd::NudgeRight | Cmd::NudgeUp | Cmd::NudgeDown => match self.selected() {
                Some(_) => editing.or(self
                    .path_commit
                    .as_ref()
                    .map(|_| "writing the last edit...".to_string())),
                None => Some("select strokes (S), a shape (V) or points (A) first".to_string()),
            },
            Cmd::FlipView | Cmd::ResetRotation => {
                self.view.is_none().then(|| "the canvas is not shown yet".to_string())
            }
            Cmd::Corner | Cmd::Smooth => direct
                .filter(|d| !d.points.is_empty())
                .is_none()
                .then(|| "select points with A first".to_string())
                .or(self
                    .path_commit
                    .as_ref()
                    .map(|_| "writing the last edit...".to_string())),
            Cmd::Simplify => direct
                .is_none()
                .then(|| "pick a shape with A first".to_string())
                .or(self
                    .path_commit
                    .as_ref()
                    .map(|_| "writing the last edit...".to_string())),
            Cmd::MakePath => match direct.and_then(|d| self.scene.bundle.doc.item(&d.node)) {
                Some(klad_core::doc::Item::Node(_, _, n)) if matches!(n.shape, Shape::Path { .. }) => {
                    Some("it is a path already".into())
                }
                Some(_) => None,
                None => Some("pick a rect or ellipse with A first".into()),
            },
            Cmd::ToggleVisible | Cmd::ToggleLocked | Cmd::ToggleClip => layer.is_none().then(pick_layer).flatten(),
            Cmd::ToggleLockAlpha => match layer {
                None => pick_layer(),
                Some(l) if l.kind() != LayerKind::Paint => Some("only paint layers lock their alpha".into()),
                _ => None,
            },
            Cmd::AddMask | Cmd::AddMaskHiding => match layer {
                None => pick_layer(),
                Some(_) if self.scene.active_is_mask() => Some("this is a mask already".into()),
                Some(l) if l.mask.is_some() => Some(format!("{} has a mask already", l.name)),
                _ => None,
            },
            Cmd::PaintMask | Cmd::ToggleMask | Cmd::RemoveMask | Cmd::TintMask => {
                match (mask.is_some(), self.scene.active_is_mask()) {
                    (_, true) if cmd == Cmd::PaintMask => Some("you are painting on the mask".into()),
                    (false, false) => Some("the active layer has no mask".into()),
                    _ => None,
                }
            }
            Cmd::BackToLayer => (!self.scene.active_is_mask()).then(|| "you are not painting on a mask".to_string()),
            Cmd::OnionLayer => match layer {
                None => pick_layer(),
                Some(l) if matches!(l.kind(), LayerKind::Paint | LayerKind::Vector) => None,
                Some(_) => Some("only paint and vector layers have an onion".into()),
            },
            Cmd::NewDrawing | Cmd::NewDrawingAfter | Cmd::InsertFrame | Cmd::InsertFrameAll => {
                self.timeline_edit().or_else(|| {
                    self.here()
                        .is_none()
                        .then(|| "pick a paint or vector layer first".to_string())
                })
            }
            Cmd::Duplicate | Cmd::DeleteDrawing => self.timeline_edit().or_else(|| {
                self.drawing_here()
                    .is_none()
                    .then(|| "no drawing on this frame of the active layer".to_string())
            }),
            Cmd::Unlink => self.timeline_edit().or_else(|| {
                self.drawing_here()
                    .filter(|(c, _)| c.link.is_some())
                    .is_none()
                    .then(|| "this frame does not show a link".to_string())
            }),
            Cmd::Link => self.timeline_edit().or_else(|| {
                layer
                    .filter(|l| l.cels().iter().any(|c| c.link.is_none()) && l.kind() != LayerKind::Group)
                    .is_none()
                    .then(|| "the active layer has no drawings to show".to_string())
            }),
            Cmd::HoldLonger => self.timeline_edit().or_else(|| {
                self.drawing_here()
                    .is_none()
                    .then(|| "no drawing on this frame".to_string())
            }),
            Cmd::HoldShorter => self.timeline_edit().or_else(|| match self.drawing_here() {
                None => Some("no drawing on this frame".into()),
                Some((cel, end)) if end != OPEN && end - cel.frame <= 1 => {
                    Some(format!("{} already shows for one frame", cel.id))
                }
                _ => None,
            }),
            Cmd::RemoveFrame | Cmd::RemoveFrameAll => self.timeline_edit().or_else(|| {
                if layer.is_none() {
                    pick_layer()
                } else {
                    (self.scene.bundle.doc.timeline.frames == 1).then(|| "the timeline has one frame".to_string())
                }
            }),
            Cmd::NewPaintLayer | Cmd::NewVectorLayer | Cmd::NewGroupLayer | Cmd::NewAdjustLayer => self.timeline_edit(),
            Cmd::Unwrap => match self.selection.as_deref().map(|id| self.scene.bundle.doc.item(id)) {
                Some(Some(klad_core::doc::Item::Node(_, _, n))) if n.shape.generates() => None,
                _ => Some("select a repeat, scatter or mirror with V first".to_string()),
            },
            Cmd::MakeRepeat | Cmd::MakeScatter | Cmd::MakeMirror => match &self.selection {
                Some(id) if matches!(self.scene.bundle.doc.item(id), Some(klad_core::doc::Item::Node(..))) => None,
                _ => Some("select a shape with V first".to_string()),
            },
            Cmd::DuplicateLayer | Cmd::GroupLayer | Cmd::RenameLayer => match layer {
                None => pick_layer(),
                Some(_) if self.scene.active_is_mask() => {
                    Some("a mask goes with its layer: pick the layer itself".into())
                }
                _ => self.timeline_edit(),
            },
            Cmd::DeleteLayer => match layer {
                None => pick_layer(),
                Some(_) if self.scene.active_is_mask() => Some("this is a mask: remove the mask instead".into()),
                _ => self.timeline_edit(),
            },
            Cmd::MoveEarlier | Cmd::MoveLater => self.timeline_edit().or_else(|| match self.drawing_here() {
                None => Some("no drawing on this frame of the active layer".into()),
                Some((c, _)) if cmd == Cmd::MoveEarlier && c.frame == 0 => {
                    Some("the drawing starts at frame 0 already".into())
                }
                _ => None,
            }),
            Cmd::CopyCells => span.is_none().then_some(()).and_then(|_| {
                self.drawing_here()
                    .is_none()
                    .then(|| "select cells first: drag over them in the timeline".to_string())
            }),
            Cmd::PasteCells | Cmd::PasteCellsLinked => match &self.timeline.cells {
                None => Some("copy cells first: select them and copy the cells".into()),
                Some(_) => self.timeline_edit(),
            },
            Cmd::OnOnes | Cmd::OnTwos | Cmd::OnThrees | Cmd::FitFrames | Cmd::Reverse | Cmd::Pingpong => {
                need_span().or_else(|| self.timeline_edit())
            }
            Cmd::Inbetween => need_span()
                .or_else(|| self.timeline_edit())
                .or_else(|| {
                    span.and_then(|s| tweens::keys(&self.scene.bundle.doc, s))
                        .is_none()
                        .then(|| {
                            "select from one drawing to the frame where another starts, on a paint layer".to_string()
                        })
                })
                .or_else(|| self.tweening().then(|| "in-betweens are being made".to_string())),
            Cmd::RedoInbetweens => self.tweening().then(|| "in-betweens are being made".to_string()),
            Cmd::NewClip => (span.is_none() && self.timeline.range.is_none())
                .then(|| "select frames or mark a loop range first".to_string()),
            Cmd::DeleteClip | Cmd::ClipLoops | Cmd::ClipOwnFps => need_clip(),
            Cmd::Refill | Cmd::ShowGaps if !self.active_paint() => {
                Some("pick a paint layer with fills first".to_string())
            }
            Cmd::Refill => self
                .jobs
                .busy(crate::jobs::Tag::Edit)
                .then(|| "a change is being made".to_string())
                .or(editing),
            Cmd::FillLike => need_span().or_else(|| {
                self.jobs
                    .busy(crate::jobs::Tag::Edit)
                    .then(|| "a change is being made".to_string())
            }),
            Cmd::ShowGaps => self
                .jobs
                .busy(crate::jobs::Tag::Gaps)
                .then(|| "the gaps are being looked for".to_string()),
            Cmd::ShowArcs if self.checks.arcs.is_some() => None,
            Cmd::ShowArcs => self.track().err().map(|e| e.to_string()),
            Cmd::CompareReference => super::compare::reference_image(&self.scene.bundle.root, &self.scene.bundle.doc)
                .is_none()
                .then(|| "add a reference picture first: a picture layer marked as a reference".to_string()),
            Cmd::ExportGodot => {
                if self.jobs.busy(crate::jobs::Tag::Export) {
                    Some("an export is running".into())
                } else {
                    self.godot_folder()
                        .is_empty()
                        .then(|| "type the Godot folder in the clip row first".to_string())
                }
            }
            Cmd::Play => self.busy().then(|| "finish the stroke or drag first".to_string()),
            _ => None,
        }
    }

    fn tweening(&self) -> bool {
        self.jobs.busy(crate::jobs::Tag::Inbetween)
    }

    pub(super) fn is_on(&self, cmd: Cmd) -> Option<bool> {
        let layer = self.scene.active_layer();
        let layout = &self.settings.layout;
        let erasing = self.brush == "eraser" || (self.readout.eraser && self.readout.tablet);
        let on = match cmd {
            Cmd::Brush => self.tool == Tool::Brush && !erasing,
            Cmd::Eraser => self.tool == Tool::Brush && erasing,
            Cmd::FlipView => self.view.is_some_and(|v| v.flip),
            Cmd::Fill => self.tool == Tool::Fill,
            Cmd::Shapes => self.tool == Tool::Select,
            Cmd::Strokes => self.tool == Tool::Strokes,
            Cmd::Points => self.tool == Tool::Direct,
            Cmd::Pen => self.tool == Tool::Pen,
            Cmd::ShapeTool => self.tool == Tool::Shape,
            Cmd::TextTool => self.tool == Tool::Text,
            Cmd::BoneTool => self.tool == Tool::Bone,
            Cmd::Symmetry => self.mirror.is_some(),
            Cmd::Note => self.tool == Tool::Note,
            Cmd::CutLasso => self.cut_lasso,
            Cmd::ToggleVisible => layer.is_some_and(|l| l.visible),
            Cmd::ToggleLocked => layer.is_some_and(|l| l.locked),
            Cmd::ToggleClip => layer.is_some_and(|l| l.clip),
            Cmd::ToggleLockAlpha => layer.is_some_and(|l| l.lock_alpha),
            Cmd::ToggleMask => self.mask_of_active().is_some_and(|m| m.visible),
            Cmd::TintMask => self.scene.tint_mask,
            Cmd::OnionLayer => layer.is_some_and(|l| self.scene.onion_marked.contains(&l.id)),
            Cmd::ExposeOnes => self.timeline.on == 1,
            Cmd::ExposeTwos => self.timeline.on == 2,
            Cmd::ExposeThrees => self.timeline.on == 3,
            Cmd::Play => self.timeline.player.is_some(),
            Cmd::LoopRange => self.timeline.range.is_some(),
            Cmd::Prepare => self.buffering.preparing.is_some(),
            Cmd::Onion => self.scene.onion_on,
            Cmd::OnionSettings => self.windows.onion,
            Cmd::EveryLayer => self.timeline.ask.every_layer,
            Cmd::ClipLoops => self.current_clip().is_some_and(|c| c.looping),
            Cmd::ClipOwnFps => self.current_clip().is_some_and(|c| c.fps.is_some()),
            Cmd::HideAll => layout.hidden,
            Cmd::HideDock => !layout.dock,
            Cmd::LayersPane => layout.dock && layout.is_open(Pane::Layers),
            Cmd::ToolPane => layout.dock && layout.is_open(Pane::Tool),
            Cmd::InspectorPane => layout.dock && layout.is_open(Pane::Inspector),
            Cmd::NotesPane => layout.dock && layout.is_open(Pane::Notes),
            Cmd::HistoryPane => layout.dock && layout.is_open(Pane::History),
            Cmd::AgentPane => layout.dock && layout.is_open(Pane::Agent),
            Cmd::Timeline => layout.timeline,
            Cmd::Numbers => layout.numbers,
            Cmd::Settings => self.windows.settings,
            Cmd::About => self.windows.about,
            Cmd::Export => self.windows.export,
            Cmd::AddFont => self.windows.font,
            Cmd::PaintOver => self.windows.paintover,
            Cmd::CompareReference => self.windows.compare,
            Cmd::ShowGaps => self.checks.gaps.is_some(),
            Cmd::ShowParts => self.checks.parts,
            Cmd::ShowArcs => self.checks.arcs.is_some(),
            Cmd::BrushWindow => self.windows.brush,
            Cmd::MessageLog => self.windows.log,
            Cmd::Spin => self.spin,
            Cmd::Link => self.windows.link,
            _ => return None,
        };
        Some(on)
    }

    fn mask_of_active(&self) -> Option<&klad_core::doc::Layer> {
        match self.scene.active_is_mask() {
            true => self.scene.active_layer(),
            false => self.scene.active_layer().and_then(|l| l.mask.as_deref()),
        }
    }

    fn owner_path(&self) -> Vec<usize> {
        match self.scene.active.last() {
            Some(&MASK) => self.scene.active[..self.scene.active.len() - 1].to_vec(),
            _ => self.scene.active.clone(),
        }
    }

    fn set_active(&mut self, key: &str, value: serde_json::Value) -> Result<()> {
        let Some(id) = self.scene.active_id() else {
            return Ok(());
        };
        self.set(&id, vec![(key.to_string(), value)])
    }

    fn pane(&mut self, pane: Pane) {
        self.settings.layout.toggle(pane);
        self.settings_dirty = true;
    }

    fn retime(&mut self, how: Retime) -> Result<()> {
        self.span_action(SpanAction::Retime(how))
    }

    pub(super) fn invoke(&mut self, cmd: Cmd) -> Result<()> {
        if let Some(reason) = self.can_run(cmd) {
            self.say(Tone::Refused, reason);
            return Ok(());
        }
        let id = cmd.spec().id.to_string();
        let recent = &mut self.settings.recent_commands;
        if recent.first() != Some(&id) {
            recent.retain(|r| *r != id);
            recent.insert(0, id);
            recent.truncate(RECENT);
            self.settings_dirty = true;
        }
        self.run(cmd)
    }

    pub(super) fn recent_commands(&self) -> Vec<Cmd> {
        self.settings
            .recent_commands
            .iter()
            .filter_map(|id| Cmd::find(id))
            .collect()
    }

    fn run(&mut self, cmd: Cmd) -> Result<()> {
        let area = self.canvas_rect.size();
        let frame = self.shown_frame();
        let last = self.scene.bundle.doc.timeline.frames - 1;
        match cmd {
            Cmd::Brush => {
                self.tool = Tool::Brush;
                if self.brush == "eraser" {
                    let back = self.previous_brush.clone();
                    self.pick(&back);
                }
            }
            Cmd::Eraser => {
                self.tool = Tool::Brush;
                if self.brush == "eraser" {
                    let back = self.previous_brush.clone();
                    self.pick(&back);
                } else {
                    self.previous_brush = self.brush.clone();
                    self.pick("eraser");
                }
            }
            Cmd::Fill => self.tool = Tool::Fill,
            Cmd::Shapes => self.tool = Tool::Select,
            Cmd::Strokes => self.tool = Tool::Strokes,
            Cmd::Points => self.tool = Tool::Direct,
            Cmd::Pen => self.tool = Tool::Pen,
            Cmd::ShapeTool => self.tool = Tool::Shape,
            Cmd::TextTool => self.tool = Tool::Text,
            Cmd::BoneTool => self.tool = Tool::Bone,
            Cmd::Symmetry => {
                self.mirror = match self.mirror {
                    Some(_) => None,
                    None => Some(self.scene.size().0 as f32 / 2.0),
                }
            }
            Cmd::MakeRepeat => self.wrap("repeat")?,
            Cmd::MakeScatter => self.wrap("scatter")?,
            Cmd::MakeMirror => self.wrap("mirror")?,
            Cmd::Unwrap => self.unwrap()?,
            Cmd::ImportPicture => self.windows.import = !self.windows.import,
            Cmd::Parallax => {}
            Cmd::Note => self.tool = Tool::Note,
            Cmd::Smaller => self.size = (self.size / 1.25).max(0.5),
            Cmd::Bigger => self.size = (self.size * 1.25).min(800.0),
            Cmd::Fit => self.view = Some(View::fit(self.canvas_size(), area)),
            Cmd::ActualSize => {
                let ppp = self.ctx.pixels_per_point();
                if let Some(view) = self.view.as_mut() {
                    view.zoom_at(1.0 / (view.zoom * ppp), area / 2.0, area);
                }
            }
            Cmd::TurnRight | Cmd::TurnLeft => {
                let step = if cmd == Cmd::TurnLeft { -15.0f32 } else { 15.0 };
                if let Some(view) = self.view.as_mut() {
                    view.rotate_at(step.to_radians(), area / 2.0, area);
                }
            }
            Cmd::Undo => self.step(Stack::Undo)?,
            Cmd::Redo => self.step(Stack::Redo)?,
            Cmd::Delete => match self.tool {
                Tool::Note => self.remove_open_pin()?,
                _ if self.on_marks() => self.edit_picked(LassoAction::Remove)?,
                Tool::Direct => {
                    let points: Vec<usize> = self
                        .direct
                        .as_ref()
                        .map_or(Vec::new(), |d| d.points.iter().copied().collect());
                    self.commit_path(PathEdit::Remove { points })?;
                }
                _ => {
                    if let Some(id) = self.selection.take() {
                        self.apply(vec![Op::Remove { ids: vec![id] }])?;
                    }
                }
            },
            Cmd::Deselect => {
                self.timeline.span = None;
                match self.tool {
                    Tool::Strokes => self.picked = None,
                    Tool::Direct => self.direct = None,
                    Tool::Select => {
                        self.selection = None;
                        self.picked = None;
                    }
                    _ => self.selection = None,
                }
                self.note_at = None;
                self.lasso = None;
                self.point_drag = None;
            }
            Cmd::Copy => {
                let ctx = self.ctx.clone();
                self.copy_picked(&ctx);
            }
            Cmd::Paste => self.paste()?,
            Cmd::SelectAll => self.select_all_strokes(),
            Cmd::CutLasso => self.cut_lasso = !self.cut_lasso,
            Cmd::Recolour => self.recolour_picked()?,
            Cmd::Thinner => self.size_picked(0.8)?,
            Cmd::Thicker => self.size_picked(1.25)?,
            Cmd::FlipH => self.flip(true)?,
            Cmd::FlipV => self.flip(false)?,
            Cmd::TurnLeft90 => self.turn(-90.0)?,
            Cmd::TurnRight90 => self.turn(90.0)?,
            Cmd::DuplicateInPlace => self.duplicate_in_place()?,
            Cmd::NudgeLeft => self.nudge(-1.0, 0.0)?,
            Cmd::NudgeRight => self.nudge(1.0, 0.0)?,
            Cmd::NudgeUp => self.nudge(0.0, -1.0)?,
            Cmd::NudgeDown => self.nudge(0.0, 1.0)?,
            Cmd::FlipView => {
                if let Some(view) = self.view.as_mut() {
                    view.flip_at(area / 2.0, area);
                }
            }
            Cmd::ResetRotation => {
                if let Some(view) = self.view.as_mut() {
                    let angle = view.angle;
                    view.rotate_at(-angle, area / 2.0, area);
                }
            }
            Cmd::SwapColours => self.swap_colours(),
            Cmd::Corner | Cmd::Smooth => {
                let points: Vec<usize> = self
                    .direct
                    .as_ref()
                    .map_or(Vec::new(), |d| d.points.iter().copied().collect());
                self.commit_path(match cmd {
                    Cmd::Corner => PathEdit::Corner { points },
                    _ => PathEdit::Smooth { points },
                })?;
            }
            Cmd::Simplify => self.commit_path(PathEdit::Simplify {
                tolerance: self.simplify_px,
            })?,
            Cmd::MakePath => self.commit_path(PathEdit::Convert)?,
            Cmd::AddSwatch => {
                let edit = PaletteEdit::Add {
                    color: crate::color::hex(self.color),
                    name: self.swatch_name.trim().to_string(),
                    index: None,
                };
                self.swatch_name.clear();
                self.apply(vec![Op::Palette(edit)])?;
            }
            Cmd::NewPaintLayer => self.add_layer("paint")?,
            Cmd::NewGroupLayer => self.new_group(false)?,
            Cmd::NewAdjustLayer => {
                self.add_layer_value(json!({"kind": "adjust", "name": "curves", "adjust": {"curves": {}}}))?
            }
            Cmd::GroupLayer => self.new_group(true)?,
            Cmd::DuplicateLayer => self.duplicate_layer()?,
            Cmd::DeleteLayer => self.delete_layer()?,
            Cmd::RenameLayer => {
                if let Some(l) = self.scene.active_layer() {
                    let place = match self.settings.layout.is_open(crate::layout::Pane::Layers) {
                        true => super::layers::Where::Panel,
                        false => super::layers::Where::Timeline,
                    };
                    self.renaming = Some(super::layers::Rename {
                        id: l.id.clone(),
                        text: l.name.clone(),
                        place,
                    });
                }
            }
            Cmd::MoveEarlier => self.move_drawing(-1)?,
            Cmd::MoveLater => self.move_drawing(1)?,
            Cmd::CopyCells => self.copy_selected_cells(),
            Cmd::PasteCells => self.paste_cells(false)?,
            Cmd::PasteCellsLinked => self.paste_cells(true)?,
            Cmd::ClipSettings => self.windows.clip = !self.windows.clip,
            Cmd::NewDocument => self.wants = Some(super::Want::New),
            Cmd::OpenDocument => self.wants = Some(super::Want::Open),
            Cmd::OpenRecent => self.wants = Some(super::Want::Recent),
            Cmd::About => self.windows.about = !self.windows.about,
            Cmd::Export => self.windows.export = !self.windows.export,
            Cmd::AddFont => self.windows.font = !self.windows.font,
            Cmd::PaintOver => self.windows.paintover = !self.windows.paintover,
            Cmd::CompareReference => self.compare_reference()?,
            Cmd::Refill => self.refill()?,
            Cmd::FillLike => self.fill_like()?,
            Cmd::ShowGaps => self.show_gaps()?,
            Cmd::ShowParts => self.checks.parts = !self.checks.parts,
            Cmd::ShowArcs => self.show_arcs()?,
            Cmd::InstallCommand => self.start_job(install_job()),
            Cmd::ReportProblem => {
                let text = self.problem_report();
                self.ctx.copy_text(text);
                self.say(
                    Tone::Done,
                    "copied a problem report: paste it into an issue at github.com/klad-app/klad",
                );
            }
            Cmd::CopyPath => {
                let path = self.scene.bundle.root.display().to_string();
                self.ctx.copy_text(path.clone());
                self.say(Tone::Done, format!("copied {path}"));
            }
            Cmd::NewVectorLayer => self.add_layer("vector")?,
            Cmd::ToggleVisible => {
                let on = self.is_on(cmd) == Some(true);
                self.set_active("visible", json!(!on))?;
            }
            Cmd::ToggleLocked => {
                let on = self.is_on(cmd) == Some(true);
                self.set_active("locked", json!(!on))?;
            }
            Cmd::ToggleClip => {
                let on = self.is_on(cmd) == Some(true);
                self.set_active("clip", json!(!on))?;
            }
            Cmd::ToggleLockAlpha => {
                let on = self.is_on(cmd) == Some(true);
                self.set_active("lockAlpha", json!(!on))?;
            }
            Cmd::AddMask | Cmd::AddMaskHiding => {
                if let Some(id) = self.scene.active_id() {
                    self.apply(vec![Op::AddMask {
                        mask: json!({}),
                        layer: id,
                        hide: cmd == Cmd::AddMaskHiding,
                    }])?;
                    let mut path = self.scene.active.clone();
                    path.push(MASK);
                    self.activate(path)?;
                }
            }
            Cmd::PaintMask => {
                let mut path = self.owner_path();
                path.push(MASK);
                self.activate(path)?;
            }
            Cmd::BackToLayer => {
                let path = self.owner_path();
                self.activate(path)?;
            }
            Cmd::RemoveMask => {
                if let Some(id) = self.mask_of_active().map(|m| m.id.clone()) {
                    let path = self.owner_path();
                    self.activate(path)?;
                    self.apply(vec![Op::Remove { ids: vec![id] }])?;
                }
            }
            Cmd::ToggleMask => {
                if let Some((id, on)) = self.mask_of_active().map(|m| (m.id.clone(), m.visible)) {
                    self.apply(vec![Op::Set {
                        id,
                        props: Map::from_iter([("visible".to_string(), json!(!on))]),
                    }])?;
                }
            }
            Cmd::TintMask => {
                self.scene.tint_mask = !self.scene.tint_mask;
                self.dirty.extend(self.scene.all_tiles());
            }
            Cmd::OnionLayer => {
                if let Some(id) = self.scene.active_id() {
                    if !self.scene.onion_marked.remove(&id) {
                        self.scene.onion_marked.insert(id);
                    }
                    self.onion_changed()?;
                }
            }
            Cmd::NextFrame => self.go_to((frame + 1).min(last))?,
            Cmd::PrevFrame => self.go_to(frame.saturating_sub(1))?,
            Cmd::NextDrawing => {
                if let Some(next) = self.drawing_starts().into_iter().find(|f| *f > frame) {
                    self.go_to(next)?;
                }
            }
            Cmd::PrevDrawing => {
                if let Some(previous) = self.drawing_starts().into_iter().rev().find(|f| *f < frame) {
                    self.go_to(previous)?;
                }
            }
            Cmd::FirstFrame => self.go_to(0)?,
            Cmd::LastFrame => self.go_to(last)?,
            Cmd::NewDrawing => self.new_drawing()?,
            Cmd::NewDrawingAfter => self.next_drawing()?,
            Cmd::Duplicate => self.duplicate_next()?,
            Cmd::Link => self.windows.link = !self.windows.link,
            Cmd::Unlink => self.unlink_here()?,
            Cmd::DeleteDrawing => self.delete_here()?,
            Cmd::HoldLonger => self.hold(1)?,
            Cmd::HoldShorter => self.hold(-1)?,
            Cmd::InsertFrame => self.frames_at(true, false)?,
            Cmd::InsertFrameAll => self.frames_at(true, true)?,
            Cmd::RemoveFrame => self.frames_at(false, false)?,
            Cmd::RemoveFrameAll => self.frames_at(false, true)?,
            Cmd::ExposeOnes => self.timeline.on = 1,
            Cmd::ExposeTwos => self.timeline.on = 2,
            Cmd::ExposeThrees => self.timeline.on = 3,
            Cmd::Play => self.play()?,
            Cmd::Prepare => self.prepare_frames()?,
            Cmd::LoopRange => {
                self.timeline.range = match self.timeline.range {
                    Some(_) => None,
                    None => {
                        let f = self.scene.frame;
                        Some((f.saturating_sub(3), (f + 4).min(last)))
                    }
                };
                self.onion_changed()?;
            }
            Cmd::Onion => self.toggle_onion()?,
            Cmd::OnionSettings => self.windows.onion = !self.windows.onion,
            Cmd::OnOnes => self.retime(Retime::On(1))?,
            Cmd::OnTwos => self.retime(Retime::On(2))?,
            Cmd::OnThrees => self.retime(Retime::On(3))?,
            Cmd::FitFrames => self.retime(Retime::Fit(self.timeline.ask.fit))?,
            Cmd::Reverse => self.retime(Retime::Reverse)?,
            Cmd::Pingpong => self.retime(Retime::Pingpong)?,
            Cmd::EveryLayer => self.timeline.ask.every_layer = !self.timeline.ask.every_layer,
            Cmd::Inbetween => self.span_action(SpanAction::Inbetween)?,
            Cmd::RedoInbetweens => self.span_action(SpanAction::Redo)?,
            Cmd::NewClip => {
                let (from, to) = match (&self.timeline.span, self.timeline.range) {
                    (Some(s), _) => (s.from, s.to),
                    (None, Some(range)) => range,
                    (None, None) => return Ok(()),
                };
                self.add_clip(from, to)?;
            }
            Cmd::DeleteClip | Cmd::ClipLoops | Cmd::ClipOwnFps => {
                if let Some(clip) = self.current_clip() {
                    let fps = self.scene.bundle.doc.timeline.fps;
                    let set = |looping, fps| {
                        Op::Clip(ClipEdit::Set {
                            name: clip.name.clone(),
                            rename: None,
                            from: None,
                            to: None,
                            looping,
                            fps,
                        })
                    };
                    let op = match cmd {
                        Cmd::DeleteClip => Op::Clip(ClipEdit::Remove {
                            name: clip.name.clone(),
                        }),
                        Cmd::ClipLoops => set(Some(!clip.looping), None),
                        _ => set(None, Some(clip.fps.is_none().then_some(fps))),
                    };
                    self.timeline_apply(vec![op])?;
                }
            }
            Cmd::ExportGodot => self.export_godot(),
            Cmd::Palette | Cmd::Keys => {
                self.palette = match self.palette {
                    Some(_) => None,
                    None => Some(super::PaletteState::default()),
                }
            }
            Cmd::HideAll => self.settings.layout.hidden = !self.settings.layout.hidden,
            Cmd::HideDock => {
                let layout = &mut self.settings.layout;
                layout.dock = !(layout.dock && !layout.hidden);
                layout.hidden = false;
                self.settings_dirty = true;
            }
            Cmd::LayersPane => self.pane(Pane::Layers),
            Cmd::ToolPane => self.pane(Pane::Tool),
            Cmd::InspectorPane => self.pane(Pane::Inspector),
            Cmd::NotesPane => self.pane(Pane::Notes),
            Cmd::HistoryPane => self.pane(Pane::History),
            Cmd::AgentPane => self.pane(Pane::Agent),
            Cmd::Timeline => {
                self.settings.layout.timeline = !self.settings.layout.timeline;
                self.settings_dirty = true;
            }
            Cmd::Numbers => {
                self.settings.layout.numbers = !self.settings.layout.numbers;
                self.settings_dirty = true;
            }
            Cmd::Settings => self.windows.settings = !self.windows.settings,
            Cmd::BrushWindow => self.windows.brush = !self.windows.brush,
            Cmd::MessageLog => self.windows.log = !self.windows.log,
            Cmd::Spin => self.spin = !self.spin,
            Cmd::ResetNumbers => *lock(&self.stats) = Stats::default(),
            Cmd::Eyedropper | Cmd::Pan | Cmd::Zoom | Cmd::FlipFrames | Cmd::Roll | Cmd::SizeDrag => {}
        }
        Ok(())
    }

    pub(super) fn command_keys(&mut self, ui: &egui::Ui) -> Result<()> {
        if ui.ctx().egui_wants_keyboard_input() {
            return Ok(());
        }
        if self.capturing.is_some() {
            self.capture_key(ui);
            return Ok(());
        }
        self.flip_and_roll(ui)?;
        let bindings = self.bindings.clone();
        let pressed: Vec<(Option<Cmd>, bool, bool)> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Copy => Some((Some(Cmd::Copy), false, false)),
                    egui::Event::Paste(_) => Some((Some(Cmd::Paste), false, false)),
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat,
                        modifiers,
                        ..
                    } => Some((bindings.for_key(*key, *modifiers), *repeat, modifiers.shift)),
                    _ => None,
                })
                .collect()
        });
        let mut done = BTreeSet::new();
        pressed
            .into_iter()
            .filter_map(|(cmd, repeat, shift)| cmd.map(|c| (c, repeat, shift)))
            .try_for_each(|(cmd, repeat, shift)| {
                self.key_shift = shift;
                let skip = repeat
                    && match cmd.spec().repeat {
                        Repeat::No => true,
                        Repeat::Yes => false,
                        Repeat::WhenShown => self.waiting(),
                    };
                let once = matches!(cmd, Cmd::Copy | Cmd::Paste);
                if skip || (once && !done.insert(cmd)) {
                    return Ok(());
                }
                match cmd {
                    Cmd::Copy if !self.on_marks() => Ok(()),
                    Cmd::Paste if self.clipboard.is_none() => Ok(()),
                    _ => self.invoke(cmd),
                }
            })
    }

    pub(super) fn tab_keys(&mut self) -> Result<()> {
        std::mem::take(&mut self.tabs)
            .into_iter()
            .try_for_each(|shift| self.invoke(if shift { Cmd::HideDock } else { Cmd::HideAll }))
    }

    pub(super) fn run_queued(&mut self) -> Result<()> {
        std::mem::take(&mut self.queued)
            .into_iter()
            .try_for_each(|cmd| self.invoke(cmd))
    }

    pub(super) fn command_hint(&self, cmd: Cmd) -> String {
        let hint = cmd.spec().hint_with(&self.bindings.label(cmd));
        match self.can_run(cmd) {
            Some(reason) if !cmd.spec().is_hold() => format!("{hint}\n\nnot now: {reason}"),
            _ => hint,
        }
    }

    pub(super) fn command_button(&mut self, ui: &mut egui::Ui, cmd: Cmd, label: Option<&str>) -> egui::Response {
        let size = match label {
            Some(_) => theme::BODY,
            None => theme::ICON,
        };
        self.command_sized(ui, cmd, label, size, egui::vec2(theme::TARGET, theme::TARGET))
    }

    pub(super) fn command_sized(
        &mut self,
        ui: &mut egui::Ui,
        cmd: Cmd,
        label: Option<&str>,
        size: f32,
        min: egui::Vec2,
    ) -> egui::Response {
        let spec = cmd.spec();
        let text = match label {
            Some(label) => format!("{} {label}", spec.icon),
            None => spec.icon.to_string(),
        };
        let button = egui::Button::new(RichText::new(text).size(size))
            .selected(self.is_on(cmd) == Some(true))
            .min_size(min);
        let response = ui
            .add_enabled(self.can_run(cmd).is_none(), button)
            .hint(self.command_hint(cmd));
        if response.clicked() {
            self.queued.push(cmd);
        }
        response
    }

    pub(super) fn command_check(&mut self, ui: &mut egui::Ui, cmd: Cmd, label: &str) -> egui::Response {
        let mut on = self.is_on(cmd) == Some(true);
        let response = ui
            .add_enabled(self.can_run(cmd).is_none(), egui::Checkbox::new(&mut on, label))
            .hint(self.command_hint(cmd));
        if response.changed() {
            self.queued.push(cmd);
        }
        response
    }

    pub(super) fn key_letter(&self, ui: &egui::Ui, rect: egui::Rect, cmd: Cmd) {
        if let Some(k) = self
            .bindings
            .keys(cmd)
            .first()
            .filter(|k| !k.command && !k.alt && !k.shift)
        {
            ui.painter().text(
                rect.right_bottom() - egui::vec2(3.0, 2.0),
                egui::Align2::RIGHT_BOTTOM,
                k.label(),
                egui::FontId::proportional(9.0),
                theme::WEAK,
            );
        }
    }
}

pub(crate) fn install_job() -> Spec {
    Spec {
        tag: Tag::Install,
        name: "install the klad command".into(),
        exe: std::path::PathBuf::new(),
        args: vec!["install".into()],
        env: Vec::new(),
        reruns: 0,
        summary: |out, _| out.lines().next().unwrap_or_default().to_string(),
        failure: |_, err| err.lines().last().unwrap_or_default().to_string(),
    }
}

impl Editor {
    pub(crate) fn problem_report(&self) -> String {
        let lines: Vec<String> = self
            .messages
            .log()
            .rev()
            .take(40)
            .map(|m| format!("{:?}: {}", m.tone, m.text))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!(
            "klad {}\n{} {}\ncanvas {}x{}, {} layers, {} frames\n\nlast messages:\n{}\n",
            crate::VERSION,
            std::env::consts::OS,
            std::env::consts::ARCH,
            self.scene.bundle.doc.canvas.width,
            self.scene.bundle.doc.canvas.height,
            self.scene.bundle.doc.all_layers().len(),
            self.scene.bundle.doc.timeline.frames,
            lines.join("\n")
        )
    }
}
