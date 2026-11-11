use crate::layers;
use anyhow::{Context, Result, anyhow, bail};
use klad_brush::BrushSet;
use klad_core::bundle::{Bundle, write_atomic};
use klad_core::doc::{Item, Kind, Layer, Node};
use klad_core::markedit::{LassoAction, MarkChange, address, parse_address};
use klad_core::ops::{Applied, Op, Place, parse_lines};
use klad_core::stroke::Mark as PaintMark;
use klad_render::onion::{self, Onion, Onions};
use klad_render::overlay::{self, Mark, Overlay};
use klad_render::{Frame, Rasters, View, compose_view, vector};
use serde_json::{Map, Value, json};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const PREVIEW_SIDE: f32 = 1024.0;
pub const PIXEL_SIDE: f32 = 512.0;
const STRING_KEYS: [&str; 5] = ["name", "d", "id", "link", "text"];

pub struct Output {
    pub json: bool,
    pub render: bool,
    pub scale: Option<f32>,
    pub frame: u32,
    pub overlay: Option<String>,
}

pub struct Look {
    pub kinds: Option<String>,
    pub focus: Option<String>,
    pub depth: usize,
    pub onion: Onion,
    pub onion_layers: Option<Vec<String>>,
    pub tracks: Vec<String>,
    pub references: bool,
    pub repeat: u32,
}

impl Look {
    pub fn plain(kinds: Option<String>) -> Self {
        Self {
            kinds,
            focus: None,
            depth: 1,
            onion: Onion::default(),
            onion_layers: None,
            tracks: Vec::new(),
            references: false,
            repeat: 1,
        }
    }

    pub fn kinds(&self) -> Result<Kinds> {
        self.kinds
            .as_deref()
            .map(overlay_flags)
            .transpose()
            .map(Option::unwrap_or_default)
    }

    pub fn tracks(&self) -> Result<Vec<crate::arcs::Track>> {
        self.tracks.iter().map(|t| crate::arcs::Track::parse(t)).collect()
    }

    pub fn wants_onion(&self) -> Result<bool> {
        Ok(match &self.kinds {
            Some(kinds) => overlay_flags(kinds)?.onion,
            None => false,
        })
    }
}

pub fn parse_value(key: &str, text: &str) -> Value {
    let last = key.rsplit('.').next().unwrap_or(key);
    if STRING_KEYS.contains(&last) {
        return Value::String(text.to_string());
    }
    serde_json::from_str(text).unwrap_or_else(|_| {
        let numbers: Option<Vec<f64>> = text.split(',').map(|n| n.trim().parse().ok()).collect();
        match numbers {
            Some(list) if list.len() > 1 => json!(list),
            _ => Value::String(text.to_string()),
        }
    })
}

pub fn parse_props(pairs: &[String]) -> Result<Map<String, Value>> {
    pairs
        .iter()
        .map(|pair| {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| anyhow!("{pair:?} is not key=value"))?;
            Ok((key.trim().to_string(), parse_value(key.trim(), value)))
        })
        .collect()
}

fn insert_path(target: &mut Value, path: &str, value: Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let Some((last, parents)) = parts.split_last() else {
        return;
    };
    let parent = parents.iter().fold(target, |node, part| {
        if !node.is_object() {
            *node = json!({});
        }
        let map = node.as_object_mut().expect("object");
        let child = map.entry(part.to_string()).or_insert_with(|| json!({}));
        if !child.is_object() {
            *child = json!({});
        }
        child
    });
    if let Some(map) = parent.as_object_mut() {
        map.insert(last.to_string(), value);
    }
}

pub fn nest(props: Map<String, Value>) -> Value {
    props.into_iter().fold(json!({}), |mut acc, (key, value)| {
        insert_path(&mut acc, &key, value);
        acc
    })
}

pub fn add_op(what: &str, rest: &[String], place: Place) -> Result<Op> {
    match what {
        "layer" => {
            let (kind, props) = match rest.split_first() {
                Some((kind, props)) if !kind.contains('=') => (kind.clone(), props),
                _ => bail!(
                    "say which kind: klad add <bundle> layer paint|pixel|vector|group|adjust|image [key=value ...]"
                ),
            };
            let layer = match kind.as_str() {
                "adjust" => crate::looks::adjust_layer(props)?,
                _ => {
                    let mut layer = nest(parse_props(props)?);
                    layer["kind"] = Value::String(kind);
                    layer
                }
            };
            Ok(Op::AddLayer { layer, place })
        }
        "cel" => {
            let layer = place
                .into
                .clone()
                .ok_or_else(|| anyhow!("add cel needs --in <layer>"))?;
            let mut cel = nest(parse_props(rest)?);
            if let Some(frame) = place.frame {
                cel["frame"] = json!(frame);
            }
            Ok(Op::AddCel { cel, layer })
        }
        "rect" | "ellipse" | "path" | "group" | "rays" | "scatter" | "repeat" | "mirror" | "text" => {
            let mut node = nest(parse_props(rest)?);
            node["type"] = Value::String(what.to_string());
            Ok(Op::AddNode { node, place })
        }
        "mask" => {
            let layer = place
                .into
                .clone()
                .ok_or_else(|| anyhow!("add mask needs --in <the layer it masks>"))?;
            let mut mask = nest(parse_props(rest)?);
            let hide = match mask.as_object_mut().and_then(|m| m.remove("hide")) {
                None => false,
                Some(Value::Bool(b)) => b,
                Some(other) => bail!("hide takes true or false, not {other}"),
            };
            Ok(Op::AddMask { mask, layer, hide })
        }
        other => bail!(
            "cannot add {other:?}: use layer, cel, mask, rect, ellipse, path, group, rays, image, scatter, repeat, mirror or text"
        ),
    }
}

pub fn pixel_scale(bundle: &Bundle, longest: f32) -> Option<f32> {
    (bundle.doc.pixel_art() && longest <= PIXEL_SIDE).then(|| (PIXEL_SIDE / longest.max(1.0)).floor().clamp(1.0, 64.0))
}

fn preview_scale(bundle: &Bundle, scale: Option<f32>) -> f32 {
    scale.unwrap_or_else(|| {
        let longest = bundle.doc.canvas.width.max(bundle.doc.canvas.height) as f32;
        pixel_scale(bundle, longest).unwrap_or((PREVIEW_SIDE / longest).min(1.0))
    })
}

pub fn render_path(bundle: &Bundle, frame: u32, view: View, tag: &str) -> PathBuf {
    let frame = if bundle.doc.timeline.frames > 1 {
        format!("-f{frame}")
    } else {
        String::new()
    };
    let region = view
        .region
        .map_or(String::new(), |[x, y, w, h]| format!("-{x}_{y}_{w}x{h}"));
    let scale = if (view.scale - 1.0).abs() < f32::EPSILON {
        String::new()
    } else {
        format!("@{:.3}", view.scale)
    };
    bundle
        .cache_dir()
        .join("renders")
        .join(format!("r{:016x}{frame}{region}{scale}{tag}.png", bundle.seen))
}

#[derive(Default)]
pub struct Kinds {
    pub ids: bool,
    pub bbox: bool,
    pub grid: bool,
    pub onion: bool,
    pub parts: bool,
    pub arcs: bool,
    pub bones: bool,
    pub weights: Option<String>,
}

pub fn overlay_flags(kinds: &str) -> Result<Kinds> {
    kinds
        .split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .try_fold(Kinds::default(), |acc, k| {
            Ok(match k {
                "ids" => Kinds { ids: true, ..acc },
                "bbox" | "boxes" => Kinds { bbox: true, ..acc },
                "grid" => Kinds { grid: true, ..acc },
                "onion" => Kinds { onion: true, ..acc },
                "parts" => Kinds { parts: true, ..acc },
                "arcs" => Kinds { arcs: true, ..acc },
                "bones" => Kinds { bones: true, ..acc },
                w if w.starts_with("weights:") => Kinds {
                    weights: Some(w.trim_start_matches("weights:").to_string()),
                    bones: true,
                    ..acc
                },
                "all" => Kinds {
                    ids: true,
                    bbox: true,
                    grid: true,
                    ..acc
                },
                other => bail!(
                    "unknown overlay {other:?}: use ids, bbox, grid, onion, parts, arcs, bones, weights:<bone> or all"
                ),
            })
        })
}

pub fn arcs_of(bundle: &Bundle, look: &Look, frame: u32, wanted: bool) -> Result<Vec<overlay::Arc>> {
    if !wanted {
        return Ok(Vec::new());
    }
    let tracks = look.tracks()?;
    if tracks.is_empty() {
        bail!("--overlay arcs needs --track: a mark (c4:s12), a part (leaf) or a point (x,y)");
    }
    let span = crate::arcs::range(&bundle.doc, frame, None, None);
    Ok(crate::arcs::arcs_overlay(&crate::arcs::track(bundle, &tracks, span)?))
}

pub fn onion_layers(bundle: &Bundle, look: &Look) -> Result<Vec<String>> {
    match &look.onion_layers {
        Some(keys) => keys
            .iter()
            .map(|k| bundle.doc.find_layer(k).map(|l| l.id.clone()))
            .collect(),
        None => Ok(onion::animated(&bundle.doc)),
    }
}

pub fn onions(bundle: &Bundle, frame: u32, layers: &[String], look: &Look, brushes: &BrushSet) -> Result<Onions> {
    let plan = onion::plan(&bundle.doc, frame, layers, &look.onion);
    let ghosts = crate::layers::load_cels(bundle, brushes, &onion::needed(&bundle.doc, &plan))?;
    Ok(onion::build(&bundle.doc, &plan, &ghosts))
}

fn paint_marks(bundle: &Bundle, layers: &[Layer], frame: u32, out: &mut Vec<Mark>) -> Result<()> {
    layers
        .iter()
        .filter(|l| l.visible)
        .try_for_each(|layer| match layer.kind() {
            Kind::Paint => {
                if let Some(cel) = layer.content_at(frame)
                    && let (_, Some(rect)) = paint_bounds(bundle, layer, &cel.id)?
                {
                    out.push(Mark {
                        label: format!("{} {}", layer.id, layer.name),
                        rect,
                    });
                }
                Ok(())
            }
            Kind::Group => paint_marks(bundle, layer.children(), frame, out),
            Kind::Vector | Kind::Adjust | Kind::Bones => Ok(()),
        })
}

pub fn marks(bundle: &Bundle, frame: u32, focus: Option<&str>, depth: usize) -> Result<Vec<Mark>> {
    let doc = &bundle.doc;
    let mut out = match focus {
        Some(id) => overlay::scoped_marks(doc, id, frame, depth)?,
        None => overlay::vector_marks(doc, frame, depth)?,
    };
    let scope: Vec<Layer> = match focus.and_then(|id| doc.layer(id)) {
        Some(layer) => vec![layer.clone()],
        None if focus.is_some() => Vec::new(),
        None => doc.layers.clone(),
    };
    paint_marks(bundle, &scope, frame, &mut out)?;
    Ok(out)
}

fn layer_box(bundle: &Bundle, layer: &Layer, frame: u32) -> Result<Option<[f32; 4]>> {
    Ok(match layer.kind() {
        Kind::Paint => match layer.content_at(frame) {
            Some(cel) => paint_bounds(bundle, layer, &cel.id)?.1,
            None => None,
        },
        Kind::Vector => layer
            .content_at(frame)
            .map_or(Ok(None), |c| vector::nodes_box(&c.nodes))?,
        Kind::Group => layer.children().iter().try_fold(None, |acc, c| {
            Ok::<_, anyhow::Error>(union(acc, layer_box(bundle, c, frame)?))
        })?,
        Kind::Adjust | Kind::Bones => None,
    })
}

pub fn find_mark(bundle: &Bundle, id: &str) -> Result<Option<(String, PaintMark)>> {
    let Some((cel, mark)) = parse_address(id) else {
        return Ok(None);
    };
    let (layer, cel) = match bundle.doc.item(cel) {
        Some(Item::Cel(layer, c)) if layer.kind() == Kind::Paint => (layer.id.clone(), layer.content_of(c).id.clone()),
        _ => bail!("no paint cel {cel}"),
    };
    let found = bundle
        .read_marks(&layer, &cel)?
        .into_iter()
        .find(|m| m.id() == mark)
        .ok_or_else(|| anyhow!("{id} does not exist (klad strokes lists the marks)"))?;
    Ok(Some((cel, found)))
}

pub fn item_box(bundle: &Bundle, id: &str, frame: u32) -> Result<Option<[f32; 4]>> {
    if let Some((_, mark)) = find_mark(bundle, id)? {
        return Ok(mark.bounds().map(|[x0, y0, x1, y1]| [x0, y0, x1 - x0, y1 - y0]));
    }
    match bundle.doc.item(id).ok_or_else(|| anyhow!("no item {id}"))? {
        Item::Layer(layer) => layer_box(bundle, layer, frame),
        Item::Cel(layer, cel) => match layer.kind() {
            Kind::Paint => Ok(paint_bounds(bundle, layer, &layer.content_of(cel).id)?.1),
            _ => vector::nodes_box(&layer.content_of(cel).nodes),
        },
        Item::Node(..) => overlay::node_world_box(&bundle.doc, id),
    }
}

pub fn focus_view(bundle: &Bundle, id: &str, frame: u32, scale: Option<f32>) -> Result<View> {
    let [x, y, w, h] =
        item_box(bundle, id, frame)?.ok_or_else(|| anyhow!("{id} has nothing to show at frame {frame}"))?;
    let pad = w.max(h) * 0.08 + 16.0;
    let (cw, ch) = (bundle.doc.canvas.width as f32, bundle.doc.canvas.height as f32);
    let (x0, y0) = ((x - pad).max(0.0).floor(), (y - pad).max(0.0).floor());
    let (x1, y1) = ((x + w + pad).min(cw).ceil(), (y + h + pad).min(ch).ceil());
    if x1 <= x0 || y1 <= y0 {
        bail!("{id} lies outside the canvas");
    }
    let (rw, rh) = (x1 - x0, y1 - y0);
    Ok(View {
        scale: scale.unwrap_or_else(|| pixel_scale(bundle, rw.max(rh)).unwrap_or(PREVIEW_SIDE / rw.max(rh))),
        region: Some([x0 as i32, y0 as i32, rw as i32, rh as i32]),
        references: false,
    })
}

pub struct Rendered {
    pub path: PathBuf,
    pub frame: Frame,
    pub note: String,
}

pub fn render_look(
    bundle: &Bundle,
    frame: u32,
    view: View,
    look: &Look,
    out: Option<PathBuf>,
    use_cache: bool,
) -> Result<Rendered> {
    let view = View {
        references: view.references || look.references,
        ..view
    };
    let brushes = BrushSet::for_bundle(bundle)?;
    let loaded = layers::load(bundle, frame, &brushes, use_cache)?;
    let kinds = look.kinds()?;
    let overlay = (kinds.ids || kinds.bbox || kinds.grid || kinds.parts || kinds.arcs || kinds.bones)
        .then(|| {
            let mut found = if kinds.ids || kinds.bbox {
                marks(bundle, frame, look.focus.as_deref(), look.depth)?
            } else {
                Vec::new()
            };
            if kinds.parts {
                found.extend(crate::arcs::part_marks(bundle, frame)?);
            }
            Ok::<_, anyhow::Error>(Overlay {
                marks: found,
                boxes: kinds.bbox || kinds.parts,
                ids: kinds.ids || kinds.parts,
                grid: kinds.grid,
                cells: bundle.doc.pixel_art(),
                arcs: arcs_of(bundle, look, frame, kinds.arcs)?,
                frame: Some(frame),
                numbers: true,
                bones: match kinds.bones {
                    true => crate::rig::bones_at(bundle, frame)?,
                    false => Vec::new(),
                },
                targets: match kinds.bones {
                    true => crate::rig::targets_at(bundle, frame),
                    false => Vec::new(),
                },
                heat: kinds
                    .weights
                    .as_deref()
                    .map(|b| crate::rig::heat(bundle, b))
                    .transpose()?,
            })
        })
        .transpose()?;
    let onions = kinds
        .onion
        .then(|| onions(bundle, frame, &onion_layers(bundle, look)?, look, &brushes))
        .transpose()?;
    let rasters: &Rasters = &loaded.rasters;
    let image = compose_view(&bundle.doc, frame, rasters, view, overlay.as_ref(), onions.as_ref())?;
    let tag = match (overlay.is_some(), onions.is_some()) {
        (true, true) => "-overlay-onion",
        (true, false) => "-overlay",
        (false, true) => "-onion",
        (false, false) => "",
    };
    let tag = match look.repeat > 1 {
        true => format!("{tag}-x{}", look.repeat),
        false => tag.to_string(),
    };
    let mut image = match look.repeat > 1 {
        true => image.tiled(look.repeat),
        false => image,
    };
    let path = out.unwrap_or_else(|| render_path(bundle, frame, view, &tag));
    write_atomic(&path, &image.png()?)?;
    let note = match loaded.updated {
        0 => format!(
            "paint cels: {} replayed ({} strokes, {} dabs), {} cached",
            loaded.replayed, loaded.strokes, loaded.dabs, loaded.cached
        ),
        updated => format!(
            "paint cels: {updated} updated in place, {} replayed ({} marks redrawn), {} cached",
            loaded.replayed, loaded.strokes, loaded.cached
        ),
    };
    Ok(Rendered {
        path,
        frame: image,
        note,
    })
}

pub fn resolve_strokes(bundle: &Bundle, ops: &mut [Op]) -> Result<()> {
    ops.iter_mut().try_for_each(|op| {
        let Op::Stroke(spec) = op else {
            return Ok(());
        };
        let Some(brush) = bundle.brush(&spec.brush)? else {
            bail!(
                "unknown brush {:?}: brushes are {}",
                spec.brush,
                bundle.brush_ids()?.join(", ")
            );
        };
        spec.taper.get_or_insert(brush.taper);
        Ok(())
    })
}

fn report(bundle: &Bundle, applied: &Applied, render: Option<PathBuf>, out: &Output) {
    let painted: Vec<Value> = applied
        .marks
        .iter()
        .map(|p| {
            let ids = |fill: bool| -> Vec<u32> {
                p.added()
                    .filter(|m| m.as_fill().is_some() == fill)
                    .map(|m| m.id())
                    .collect()
            };
            let lassoed: Vec<u32> = p
                .changes
                .iter()
                .filter(|c| matches!(c, MarkChange::Lasso { .. }))
                .flat_map(MarkChange::ids)
                .collect();
            json!({ "layer": p.layer, "cel": p.cel, "ids": ids(false), "fills": ids(true), "lassoed": lassoed })
        })
        .filter(|v| v["ids"] != json!([]) || v["fills"] != json!([]) || v["lassoed"] != json!([]))
        .collect();
    let copied: Vec<(String, String)> = applied
        .marks
        .iter()
        .flat_map(|p| p.changes.iter().map(move |c| (p, c)))
        .flat_map(|(p, c)| match c {
            MarkChange::Paste { made, .. } => made
                .iter()
                .map(|m| (address(&m.cel, m.from), address(&p.cel, m.to)))
                .collect(),
            _ => Vec::new(),
        })
        .collect();
    if out.json {
        let value = json!({
            "rev": bundle.doc.rev,
            "added": applied.added,
            "changed": applied.changed,
            "removed": applied.removed,
            "strokes": painted,
            "copied": copied.iter().map(|(from, to)| json!({"from": from, "to": to})).collect::<Vec<_>>(),
            "notes": applied.notes,
            "render": render.map(|p| p.display().to_string()),
        });
        println!("{value}");
        return;
    }
    applied
        .marks
        .iter()
        .filter(|p| p.added().next().is_some())
        .for_each(|p| {
            let (fills, strokes): (Vec<&PaintMark>, Vec<&PaintMark>) = p.added().partition(|m| m.as_fill().is_some());
            let (pixels, fills): (Vec<&PaintMark>, Vec<&PaintMark>) = fills
                .into_iter()
                .partition(|m| m.as_fill().is_some_and(|f| f.on_grid()));
            let list = |marks: &[&PaintMark], what: &str| {
                let ids: Vec<String> = marks.iter().map(|m| m.id().to_string()).collect();
                (!ids.is_empty()).then(|| format!("{what} {}", ids.join(" ")))
            };
            let parts: Vec<String> = [
                list(&strokes, "strokes"),
                list(&fills, "fills"),
                list(&pixels, "pixel marks"),
            ]
            .into_iter()
            .flatten()
            .collect();
            println!("painted on {} cel {}: {}", p.layer, p.cel, parts.join(", "));
        });
    applied
        .marks
        .iter()
        .flat_map(|p| p.changes.iter().map(move |c| (p, c)))
        .filter(|(_, c)| matches!(c, MarkChange::Lasso { .. }))
        .for_each(|(p, c)| {
            let ids: Vec<String> = c.ids().iter().map(|id| format!("{}:s{id}", p.cel)).collect();
            let verb = match c {
                MarkChange::Lasso {
                    action: LassoAction::Remove,
                    ..
                } => "removed",
                _ => "moved",
            };
            println!("lasso on {} {verb} {}", p.layer, ids.join(" "));
        });
    if !copied.is_empty() {
        let pairs: Vec<String> = copied.iter().map(|(from, to)| format!("{from} -> {to}")).collect();
        println!("copied {}", pairs.join(", "));
    }
    applied.notes.iter().for_each(|n| println!("{n}"));
    applied.added.iter().for_each(|id| println!("added {id}"));
    applied.changed.iter().for_each(|id| println!("changed {id}"));
    applied.removed.iter().for_each(|id| println!("removed {id}"));
    println!("rev {}", bundle.doc.rev);
    if let Some(path) = render {
        println!("{}", path.display());
    }
}

pub fn preview(bundle: &Bundle, out: &Output) -> Result<Option<PathBuf>> {
    if !out.render {
        return Ok(None);
    }
    let view = View::scaled(preview_scale(bundle, out.scale));
    let look = Look::plain(out.overlay.clone());
    Ok(Some(render_look(bundle, out.frame, view, &look, None, true)?.path))
}

pub fn edit(path: &Path, ops: &[Op], out: &Output) -> Result<()> {
    let mut bundle = Bundle::open(path)?;
    let mut ops = ops.to_vec();
    crate::paintover::resolve(&bundle, &mut ops)?
        .iter()
        .for_each(|n| eprintln!("{n}"));
    let mut ops = crate::shapes::expand(&bundle, ops)?;
    klad_render::shift::resolve(&bundle, &mut ops)?;
    let mut ops = vector::pinned(&bundle.doc, &ops)?;
    resolve_strokes(&bundle, &mut ops)?;
    crate::fonts::settle(&bundle, &mut ops)?;
    let like = ops.iter().any(|op| matches!(op, Op::Fill(f) if f.like.is_some()));
    let notes = crate::fills::resolve(&bundle, &mut ops)?;
    notes.iter().for_each(|n| match like {
        true => println!("{n}"),
        false => eprintln!("{n}"),
    });
    crate::rig::resolve(&bundle, &mut ops)?
        .iter()
        .for_each(|n| println!("{n}"));
    let made = crate::inbetween::resolve(&bundle, &mut ops)?;
    made.iter().for_each(|n| println!("{n}"));
    if let Some(ms) = std::env::var("DRAW_TEST_WRITE_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        klad_core::bundle::write_atomic(&bundle.cache_dir().join("write-delay"), b"")?;
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    bundle.ensure_unchanged()?;
    let applied = bundle.apply(&ops)?;
    bundle.stamp("cli")?;
    let render = preview(&bundle, out)?;
    report(&bundle, &applied, render, out);
    Ok(())
}

pub fn apply(path: &Path, file: Option<PathBuf>, out: &Output) -> Result<()> {
    let text = match file {
        Some(file) => std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?,
        None => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text)?;
            text
        }
    };
    let ops = parse_lines(&text)?;
    if ops.is_empty() {
        bail!("no ops: give JSON lines on stdin or with --file");
    }
    edit(path, &ops, out)
}

fn fmt_bounds(bounds: Option<[f32; 4]>) -> String {
    bounds.map_or("empty".into(), |[x, y, w, h]| {
        format!("{:.0},{:.0} {:.0}x{:.0}", x, y, w, h)
    })
}

fn union(a: Option<[f32; 4]>, b: Option<[f32; 4]>) -> Option<[f32; 4]> {
    match (a, b) {
        (Some([ax, ay, aw, ah]), Some([bx, by, bw, bh])) => {
            let (x, y) = (ax.min(bx), ay.min(by));
            Some([x, y, (ax + aw).max(bx + bw) - x, (ay + ah).max(by + bh) - y])
        }
        (a, b) => a.or(b),
    }
}

struct PaintCount {
    strokes: usize,
    fills: usize,
}

fn paint_bounds(bundle: &Bundle, layer: &Layer, cel: &str) -> Result<(PaintCount, Option<[f32; 4]>)> {
    let marks = bundle.read_marks(&layer.id, cel)?;
    let bounds = marks.iter().fold(None, |acc, m| {
        union(acc, m.bounds().map(|[x0, y0, x1, y1]| [x0, y0, x1 - x0, y1 - y0]))
    });
    let fills = marks.iter().filter(|m| m.as_fill().is_some()).count();
    Ok((
        PaintCount {
            strokes: marks.len() - fills,
            fills,
        },
        bounds,
    ))
}

fn put_counts(value: &mut Value, count: &PaintCount) {
    value["strokes"] = json!(count.strokes);
    if count.fills > 0 {
        value["fills"] = json!(count.fills);
    }
}

fn layer_summary(bundle: &Bundle, layer: &Layer, frame: u32) -> Result<Value> {
    let cel = layer.cel_at(frame);
    let content = layer.content_at(frame);
    let mut value = json!({
        "id": layer.id,
        "kind": layer.kind_name(),
        "name": layer.name,
        "visible": layer.visible,
        "opacity": layer.opacity,
        "blend": layer.blend,
    });
    if let Some(cel) = cel {
        value["cel"] = json!({ "id": cel.id, "frame": cel.frame, "hold": cel.hold, "link": cel.link });
    }
    if layer.clip {
        value["clip"] = json!(true);
    }
    if layer.lock_alpha {
        value["lockAlpha"] = json!(true);
    }
    if layer.locked {
        value["locked"] = json!(true);
    }
    if let Some(rig) = &layer.rig {
        value["rig"] = json!(rig);
    }
    if let Some(mask) = &layer.mask {
        let mut summary = layer_summary(bundle, mask, frame)?;
        summary["role"] = json!("mask");
        value["mask"] = summary;
    }
    let bounds = match layer.kind() {
        Kind::Paint => match content {
            Some(c) => {
                let (count, bounds) = paint_bounds(bundle, layer, &c.id)?;
                put_counts(&mut value, &count);
                bounds
            }
            None => None,
        },
        Kind::Vector => {
            let nodes = content.map_or(&[][..], |c| &c.nodes[..]);
            value["nodes"] = Value::Array(nodes.iter().map(node_summary).collect::<Result<_>>()?);
            vector::nodes_box(nodes)?
        }
        Kind::Group => {
            let children = layer
                .children()
                .iter()
                .map(|c| layer_summary(bundle, c, frame))
                .collect::<Result<Vec<_>>>()?;
            let bounds = children
                .iter()
                .filter_map(|c| serde_json::from_value::<[f32; 4]>(c["bounds"].clone()).ok())
                .fold(None, |acc, b| union(acc, Some(b)));
            value["children"] = Value::Array(children);
            bounds
        }
        Kind::Bones => {
            value["bones"] = json!(layer.bones.iter().map(|b| &b.name).collect::<Vec<_>>());
            value["attached"] = json!(
                klad_core::rig::painted(&bundle.doc, &layer.id)
                    .iter()
                    .map(|l| &l.id)
                    .collect::<Vec<_>>()
            );
            None
        }
        Kind::Adjust => {
            if let Some(adjust) = layer.adjustment() {
                value["adjust"] = serde_json::to_value(adjust)?;
            }
            None
        }
    };
    if !layer.effects.is_empty() {
        value["effects"] = serde_json::to_value(&layer.effects)?;
    }
    value["bounds"] = json!(bounds);
    Ok(value)
}

fn node_summary(node: &Node) -> Result<Value> {
    let bounds = vector::node_box(node)?;
    let mut value = json!({ "id": node.id, "type": node.type_name(), "bounds": bounds });
    if !node.name.is_empty() {
        value["name"] = json!(node.name);
    }
    if !node.visible {
        value["visible"] = json!(false);
    }
    if !node.children().is_empty() {
        value["children"] = Value::Array(node.children().iter().map(node_summary).collect::<Result<_>>()?);
    }
    Ok(value)
}

const LIST_LIMIT: usize = 24;

fn print_nodes(nodes: &[Value], depth: usize) {
    let hidden = nodes.len().saturating_sub(LIST_LIMIT);
    nodes.iter().rev().take(LIST_LIMIT).for_each(|n| {
        let name = n["name"].as_str().unwrap_or("");
        let hidden = if n["visible"] == json!(false) { "  hidden" } else { "" };
        let bounds = serde_json::from_value::<Option<[f32; 4]>>(n["bounds"].clone())
            .ok()
            .flatten();
        println!(
            "{:indent$}{:<6} {:<8} {:<16} {}{hidden}",
            "",
            n["id"].as_str().unwrap_or(""),
            n["type"].as_str().unwrap_or(""),
            name,
            fmt_bounds(bounds),
            indent = depth * 2
        );
        if let Some(children) = n["children"].as_array() {
            print_nodes(children, depth + 1);
        }
    });
    if hidden > 0 {
        println!(
            "{:indent$}... {hidden} more (klad show <group id> or ls --json)",
            "",
            indent = depth * 2
        );
    }
}

fn print_layer(v: &Value, depth: usize, frames: u32) {
    let mut flags = Vec::new();
    if v["visible"] == json!(false) {
        flags.push("hidden".to_string());
    }
    if v["blend"] != json!("normal") {
        flags.push(v["blend"].as_str().unwrap_or("").to_string());
    }
    if let Some(o) = v["opacity"].as_f64().filter(|o| *o < 1.0) {
        flags.push(format!("{:.0}%", o * 100.0));
    }
    ["clip", "lockAlpha", "locked"]
        .iter()
        .filter(|k| v[**k] == json!(true))
        .for_each(|k| flags.push(k.to_string()));
    let content = match v["kind"].as_str() {
        Some("paint") => match v["fills"].as_u64() {
            Some(fills) => format!("{} strokes, {fills} fills", v["strokes"].as_u64().unwrap_or(0)),
            None => format!("{} strokes", v["strokes"].as_u64().unwrap_or(0)),
        },
        Some("pixel") => format!("{} pixel marks", v["fills"].as_u64().unwrap_or(0)),
        Some("vector") => format!("{} nodes", v["nodes"].as_array().map_or(0, Vec::len)),
        Some("adjust") => v["adjust"]
            .as_object()
            .and_then(|m| m.keys().next())
            .map_or("adjust".into(), |k| klad_core::look::label_of(k).to_string()),
        _ => format!("{} layers", v["children"].as_array().map_or(0, Vec::len)),
    };
    if let Some(list) = v["effects"].as_array() {
        let names: Vec<&str> = list
            .iter()
            .filter_map(|e| e.as_object().and_then(|m| m.keys().next()).map(String::as_str))
            .collect();
        flags.push(format!("fx: {}", names.join(" ")));
    }
    let cel = if frames > 1 {
        match v.get("cel") {
            Some(c) => {
                let link = c["link"].as_str().map_or(String::new(), |l| format!(" -> {l}"));
                format!("  {} from {}{link}", c["id"].as_str().unwrap_or(""), c["frame"])
            }
            None if v["kind"] != json!("group") && v["kind"] != json!("adjust") => "  no cel".into(),
            None => String::new(),
        }
    } else {
        String::new()
    };
    let bounds = serde_json::from_value::<Option<[f32; 4]>>(v["bounds"].clone())
        .ok()
        .flatten();
    let kind = match v["kind"].as_str() {
        Some("paint") if depth > 0 && v["role"] == json!("mask") => "mask",
        other => other.unwrap_or(""),
    };
    println!(
        "{:indent$}{:<6} {:<6} {:<16} {:<12} {}{}{}",
        "",
        v["id"].as_str().unwrap_or(""),
        kind,
        v["name"].as_str().unwrap_or(""),
        content,
        fmt_bounds(bounds),
        if flags.is_empty() {
            String::new()
        } else {
            format!("  {}", flags.join(" "))
        },
        cel,
        indent = depth * 2
    );
    if v["mask"].is_object() {
        print_layer(&v["mask"], depth + 1, frames);
    }
    if let Some(nodes) = v["nodes"].as_array() {
        print_nodes(nodes, depth + 1);
    }
    if let Some(children) = v["children"].as_array() {
        children.iter().rev().for_each(|c| print_layer(c, depth + 1, frames));
    }
}

pub fn ls(path: &Path, frame: u32, as_json: bool) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    let layers = doc
        .layers
        .iter()
        .map(|l| layer_summary(&bundle, l, frame))
        .collect::<Result<Vec<_>>>()?;
    if as_json {
        let value = json!({
            "canvas": doc.canvas,
            "timeline": doc.timeline,
            "rev": doc.rev,
            "copy": doc.here.tag,
            "frame": frame,
            "layers": layers,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let t = &doc.timeline;
    let timing = if t.frames > 1 {
        format!("  {} frames at {} fps, showing frame {frame}", t.frames, t.fps)
    } else {
        String::new()
    };
    let background = doc
        .canvas
        .background
        .as_deref()
        .map_or(String::new(), |b| format!("  background {b}"));
    println!(
        "{}x{}  rev {}{timing}{background}{}",
        doc.canvas.width,
        doc.canvas.height,
        doc.rev,
        crate::copy_tag(doc)
    );
    layers.iter().rev().for_each(|l| print_layer(l, 0, t.frames));
    Ok(())
}

pub fn show(path: &Path, id: &str) -> Result<()> {
    let bundle = Bundle::open(path)?;
    if let Some((cel, mark)) = find_mark(&bundle, id)? {
        println!(
            "{}",
            serde_json::to_string_pretty(&crate::marks::mark_json(&cel, &mark))?
        );
        return Ok(());
    }
    let value = match bundle.doc.item(id).ok_or_else(|| anyhow!("no item {id}"))? {
        Item::Layer(layer) => serde_json::to_value(layer)?,
        Item::Cel(layer, cel) => {
            let mut value = serde_json::to_value(cel)?;
            if layer.kind() == Kind::Paint {
                let content = layer.content_of(cel);
                let (count, bounds) = paint_bounds(&bundle, layer, &content.id)?;
                put_counts(&mut value, &count);
                value["bounds"] = json!(bounds);
            }
            value
        }
        Item::Node(_, _, node) => {
            let mut value = serde_json::to_value(node)?;
            value["bounds"] = json!(vector::node_box(node)?);
            value
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_parse_as_json_or_strings() {
        assert_eq!(parse_value("x", "12.5"), json!(12.5));
        assert_eq!(parse_value("fill", "#336699"), json!("#336699"));
        assert_eq!(parse_value("name", "42"), json!("42"));
        assert_eq!(parse_value("pivot", "[1,2]"), json!([1, 2]));
        assert_eq!(parse_value("light", "-1,-0.5"), json!([-1.0, -0.5]));
        assert_eq!(parse_value("name", "1,2"), json!("1,2"));
        assert_eq!(parse_value("visible", "false"), json!(false));
    }

    #[test]
    fn dotted_props_nest() {
        let props = parse_props(&["x=1".into(), "transform.rotate=15".into(), "stroke.width=2".into()]).unwrap();
        assert_eq!(
            nest(props),
            json!({"x": 1, "transform": {"rotate": 15}, "stroke": {"width": 2}})
        );
        assert!(parse_props(&["oops".into()]).is_err());
    }

    #[test]
    fn add_ops_carry_their_kind() {
        let op = add_op("layer", &["vector".into(), "name=sky".into()], Place::default()).unwrap();
        assert!(matches!(op, Op::AddLayer { ref layer, .. } if layer["kind"] == "vector" && layer["name"] == "sky"));
        let node = add_op("rect", &["w=5".into()], Place::default()).unwrap();
        assert!(matches!(node, Op::AddNode { ref node, .. } if node["type"] == "rect"));
        assert!(add_op("layer", &["name=x".into()], Place::default()).is_err());
        assert!(add_op("cel", &[], Place::default()).is_err());
    }
}
