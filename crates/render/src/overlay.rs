use crate::{Frame, View, read_rgba8, vector};
use anyhow::{Result, anyhow};
use klad_core::doc::{Document, Kind, Layer, Node};
use skia_safe::{
    AlphaType, Canvas, Color4f, ColorSpace, ColorType, Data, ImageInfo, Matrix, Paint, PaintStyle, Rect, images,
    surfaces,
};

const GLYPHS: &[(char, [u8; 7])] = &[
    ('0', [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E]),
    ('1', [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E]),
    ('2', [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F]),
    ('3', [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E]),
    ('4', [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02]),
    ('5', [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E]),
    ('6', [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E]),
    ('7', [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08]),
    ('8', [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E]),
    ('9', [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C]),
    ('A', [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]),
    ('B', [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E]),
    ('C', [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E]),
    ('D', [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C]),
    ('E', [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F]),
    ('F', [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10]),
    ('G', [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F]),
    ('H', [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]),
    ('I', [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E]),
    ('J', [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C]),
    ('K', [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11]),
    ('L', [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F]),
    ('M', [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11]),
    ('N', [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11]),
    ('O', [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
    ('P', [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10]),
    ('Q', [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D]),
    ('R', [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11]),
    ('S', [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E]),
    ('T', [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04]),
    ('U', [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
    ('V', [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04]),
    ('W', [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A]),
    ('X', [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11]),
    ('Y', [0x11, 0x11, 0x11, 0x0A, 0x04, 0x04, 0x04]),
    ('Z', [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F]),
    ('a', [0x00, 0x00, 0x0E, 0x01, 0x0F, 0x11, 0x0F]),
    ('b', [0x10, 0x10, 0x16, 0x19, 0x11, 0x11, 0x1E]),
    ('c', [0x00, 0x00, 0x0E, 0x10, 0x10, 0x11, 0x0E]),
    ('d', [0x01, 0x01, 0x0D, 0x13, 0x11, 0x11, 0x0F]),
    ('e', [0x00, 0x00, 0x0E, 0x11, 0x1F, 0x10, 0x0E]),
    ('f', [0x06, 0x09, 0x08, 0x1C, 0x08, 0x08, 0x08]),
    ('g', [0x00, 0x00, 0x0F, 0x11, 0x0F, 0x01, 0x0E]),
    ('h', [0x10, 0x10, 0x16, 0x19, 0x11, 0x11, 0x11]),
    ('i', [0x04, 0x00, 0x0C, 0x04, 0x04, 0x04, 0x0E]),
    ('j', [0x02, 0x00, 0x06, 0x02, 0x02, 0x12, 0x0C]),
    ('k', [0x10, 0x10, 0x12, 0x14, 0x18, 0x14, 0x12]),
    ('l', [0x0C, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E]),
    ('m', [0x00, 0x00, 0x1A, 0x15, 0x15, 0x11, 0x11]),
    ('n', [0x00, 0x00, 0x16, 0x19, 0x11, 0x11, 0x11]),
    ('o', [0x00, 0x00, 0x0E, 0x11, 0x11, 0x11, 0x0E]),
    ('p', [0x00, 0x00, 0x1E, 0x11, 0x1E, 0x10, 0x10]),
    ('q', [0x00, 0x00, 0x0D, 0x13, 0x0F, 0x01, 0x01]),
    ('r', [0x00, 0x00, 0x16, 0x19, 0x10, 0x10, 0x10]),
    ('s', [0x00, 0x00, 0x0E, 0x10, 0x0E, 0x01, 0x1E]),
    ('t', [0x08, 0x08, 0x1C, 0x08, 0x08, 0x09, 0x06]),
    ('u', [0x00, 0x00, 0x11, 0x11, 0x11, 0x13, 0x0D]),
    ('v', [0x00, 0x00, 0x11, 0x11, 0x11, 0x0A, 0x04]),
    ('w', [0x00, 0x00, 0x11, 0x11, 0x15, 0x15, 0x0A]),
    ('x', [0x00, 0x00, 0x11, 0x0A, 0x04, 0x0A, 0x11]),
    ('y', [0x00, 0x00, 0x11, 0x11, 0x0F, 0x01, 0x0E]),
    ('z', [0x00, 0x00, 0x1F, 0x02, 0x04, 0x08, 0x1F]),
    ('-', [0, 0, 0, 0x1F, 0, 0, 0]),
    ('_', [0, 0, 0, 0, 0, 0, 0x1F]),
    ('.', [0, 0, 0, 0, 0, 0x0C, 0x0C]),
    (',', [0, 0, 0, 0, 0x0C, 0x04, 0x08]),
    (':', [0, 0x0C, 0x0C, 0, 0x0C, 0x0C, 0]),
    ('/', [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10]),
    ('>', [0x08, 0x04, 0x02, 0x01, 0x02, 0x04, 0x08]),
    ('(', [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02]),
    (')', [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08]),
    ('#', [0x0A, 0x0A, 0x1F, 0x0A, 0x1F, 0x0A, 0x0A]),
    ('+', [0, 0x04, 0x04, 0x1F, 0x04, 0x04, 0]),
    ('=', [0, 0, 0x1F, 0, 0x1F, 0, 0]),
    ('?', [0x0E, 0x11, 0x01, 0x02, 0x04, 0, 0x04]),
    ('|', [0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04]),
    ('[', [0x0E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0E]),
    (']', [0x0E, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0E]),
];

const HUES: [[f32; 3]; 8] = [
    [1.0, 0.36, 0.36],
    [0.25, 0.8, 1.0],
    [1.0, 0.82, 0.2],
    [0.45, 1.0, 0.45],
    [0.95, 0.45, 1.0],
    [1.0, 0.6, 0.25],
    [0.4, 0.55, 1.0],
    [0.3, 1.0, 0.85],
];

#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub label: String,
    pub rect: [f32; 4],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Arc {
    pub label: String,
    pub points: Vec<(u32, [f32; 2])>,
}

#[derive(Clone, Debug, Default)]
pub struct Overlay {
    pub marks: Vec<Mark>,
    pub boxes: bool,
    pub ids: bool,
    pub grid: bool,
    pub arcs: Vec<Arc>,
    pub frame: Option<u32>,
    pub numbers: bool,
}

fn draw_arcs(canvas: &Canvas, overlay: &Overlay, to_out: impl Fn([f32; 2]) -> (f32, f32), px: f32) {
    let mut placed: Vec<Rect> = Vec::new();
    overlay.arcs.iter().enumerate().for_each(|(i, arc)| {
        let hue = HUES[(i + 3) % HUES.len()];
        let pts: Vec<(u32, (f32, f32))> = arc.points.iter().map(|(f, p)| (*f, to_out(*p))).collect();
        let pen = |rgb: [f32; 3], a: f32, width: f32| {
            let mut paint = color(rgb, a);
            paint.set_anti_alias(true);
            paint.set_style(PaintStyle::Stroke);
            paint.set_stroke_width(width);
            paint
        };
        let (back, front) = (pen([0.0, 0.0, 0.0], 0.6, 3.5 * px), pen(hue, 1.0, 1.5 * px));
        pts.windows(2).for_each(|w| {
            canvas.draw_line(w[0].1, w[1].1, &back);
            canvas.draw_line(w[0].1, w[1].1, &front);
        });
        let mut dot = color(hue, 1.0);
        dot.set_anti_alias(true);
        pts.iter().enumerate().for_each(|(k, (frame, at))| {
            let prev = pts[k.saturating_sub(1)].1;
            let next = pts[(k + 1).min(pts.len() - 1)].1;
            let (dx, dy) = (next.0 - prev.0, next.1 - prev.1);
            let len = (dx * dx + dy * dy).sqrt();
            let (nx, ny) = if len < 1e-3 { (0.0, 1.0) } else { (-dy / len, dx / len) };
            let tick = 6.0 * px;
            let ends = (
                (at.0 - nx * tick, at.1 - ny * tick),
                (at.0 + nx * tick, at.1 + ny * tick),
            );
            canvas.draw_line(ends.0, ends.1, &back);
            canvas.draw_line(ends.0, ends.1, &front);
            let current = overlay.frame == Some(*frame);
            if current {
                canvas.draw_circle(*at, 5.5 * px, &pen([1.0, 1.0, 1.0], 1.0, 2.0 * px));
            }
            canvas.draw_circle(*at, if current { 4.0 } else { 2.5 } * px, &dot);
            if overlay.numbers || current {
                let text = frame.to_string();
                let (tx, ty) = (at.0 + nx * 9.0 * px + 2.0, at.1 + ny * 9.0 * px - 3.5 * px);
                draw_text(canvas, &text, tx + px, ty + px, px, [0.0, 0.0, 0.0]);
                draw_text(canvas, &text, tx, ty, px, hue);
            }
        });
        if let Some((_, first)) = pts.first() {
            let (lw, lh) = (text_width(&arc.label, px) + 4.0 * px + 3.0, 11.0 * px);
            let mut at = Rect::from_xywh(first.0 + 10.0 * px, first.1 - 16.0 * px, lw, lh);
            while placed.iter().any(|p| Rect::intersects2(p, at)) {
                at.offset((0.0, -(lh + 1.0)));
            }
            draw_label(canvas, &arc.label, at.left, at.top, px, hue);
            placed.push(at);
        }
    });
}

fn glyph(c: char) -> [u8; 7] {
    GLYPHS
        .iter()
        .find(|(g, _)| *g == c)
        .or_else(|| GLYPHS.iter().find(|(g, _)| *g == c.to_ascii_uppercase()))
        .map_or([0; 7], |(_, rows)| *rows)
}

fn color(rgb: [f32; 3], a: f32) -> Paint {
    let mut paint = Paint::new(Color4f::new(rgb[0], rgb[1], rgb[2], a), &ColorSpace::new_srgb());
    paint.set_anti_alias(false);
    paint
}

pub fn text_width(text: &str, px: f32) -> f32 {
    text.chars().count() as f32 * 6.0 * px - px
}

pub fn draw_text(canvas: &Canvas, text: &str, x: f32, y: f32, px: f32, rgb: [f32; 3]) {
    let paint = color(rgb, 1.0);
    text.chars().enumerate().for_each(|(i, c)| {
        let ox = x + i as f32 * 6.0 * px;
        glyph(c).iter().enumerate().for_each(|(row, bits)| {
            (0..5).filter(|col| bits & (0x10 >> col) != 0).for_each(|col| {
                canvas.draw_rect(
                    Rect::from_xywh(ox + col as f32 * px, y + row as f32 * px, px, px),
                    &paint,
                );
            });
        });
    });
}

pub fn draw_label(canvas: &Canvas, text: &str, x: f32, y: f32, px: f32, accent: [f32; 3]) {
    let w = text_width(text, px) + 4.0 * px + 3.0;
    let h = 11.0 * px;
    canvas.draw_rect(Rect::from_xywh(x, y, w, h), &color([0.05, 0.06, 0.08], 0.78));
    canvas.draw_rect(Rect::from_xywh(x, y, 3.0, h), &color(accent, 1.0));
    draw_text(canvas, text, x + 3.0 + 2.0 * px, y + 2.0 * px, px, [1.0, 1.0, 1.0]);
}

fn grid_step(scale: f32) -> f32 {
    [8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0, 2048.0]
        .into_iter()
        .find(|s| s * scale >= 70.0)
        .unwrap_or(4096.0)
}

fn draw_grid(canvas: &Canvas, origin: (f32, f32), scale: f32, size: (f32, f32), px: f32) {
    let step = grid_step(scale);
    let mut line = color([1.0, 1.0, 1.0], 0.35);
    line.set_style(PaintStyle::Stroke);
    line.set_stroke_width(1.0);
    let mut shade = color([0.0, 0.0, 0.0], 0.35);
    shade.set_style(PaintStyle::Stroke);
    shade.set_stroke_width(1.0);
    let first = |o: f32| (o / step).ceil() * step;
    let xs = std::iter::successors(Some(first(origin.0)), |v| Some(v + step))
        .take_while(|v| (v - origin.0) * scale <= size.0);
    xs.for_each(|v| {
        let x = ((v - origin.0) * scale).round() + 0.5;
        canvas.draw_line((x + 1.0, 0.0), (x + 1.0, size.1), &shade);
        canvas.draw_line((x, 0.0), (x, size.1), &line);
        draw_label(canvas, &format!("{v:.0}"), x + 2.0, 2.0, px, [1.0, 1.0, 1.0]);
    });
    let ys = std::iter::successors(Some(first(origin.1)), |v| Some(v + step))
        .take_while(|v| (v - origin.1) * scale <= size.1);
    ys.for_each(|v| {
        let y = ((v - origin.1) * scale).round() + 0.5;
        canvas.draw_line((0.0, y + 1.0), (size.0, y + 1.0), &shade);
        canvas.draw_line((0.0, y), (size.0, y), &line);
        draw_label(canvas, &format!("{v:.0}"), 2.0, y + 2.0, px, [1.0, 1.0, 1.0]);
    });
}

pub fn apply(image: &skia_safe::Image, overlay: &Overlay, view: &View) -> Result<Frame> {
    let (w, h) = (image.width(), image.height());
    let info = crate::f16_info(w as u32, h as u32);
    let mut surface = surfaces::raster(&info, None, None).ok_or_else(|| anyhow!("cannot allocate {w}x{h}"))?;
    let canvas = surface.canvas();
    canvas.draw_image(image, (0, 0), None);
    let origin = view.region.map_or((0.0, 0.0), |[x, y, _, _]| (x as f32, y as f32));
    let s = view.scale;
    let px = if w.max(h) >= 1400 { 2.0 } else { 1.0 };
    if overlay.grid {
        draw_grid(canvas, origin, s, (w as f32, h as f32), px);
    }
    let to_out = |r: [f32; 4]| [(r[0] - origin.0) * s, (r[1] - origin.1) * s, r[2] * s, r[3] * s];
    if overlay.boxes {
        overlay.marks.iter().enumerate().for_each(|(i, m)| {
            let [x, y, bw, bh] = to_out(m.rect);
            let mut back = color([0.0, 0.0, 0.0], 0.55);
            back.set_style(PaintStyle::Stroke);
            back.set_stroke_width(3.0);
            let mut front = color(HUES[i % HUES.len()], 1.0);
            front.set_style(PaintStyle::Stroke);
            front.set_stroke_width(1.5);
            let rect = Rect::from_xywh(x, y, bw, bh);
            canvas.draw_rect(rect, &back);
            canvas.draw_rect(rect, &front);
        });
    }
    draw_arcs(canvas, overlay, |p| ((p[0] - origin.0) * s, (p[1] - origin.1) * s), px);
    if overlay.ids {
        let mut placed: Vec<Rect> = Vec::new();
        overlay.marks.iter().enumerate().for_each(|(i, m)| {
            let [x, y, _, _] = to_out(m.rect);
            let lw = text_width(&m.label, px) + 4.0 * px + 3.0;
            let lh = 11.0 * px;
            let mut at = Rect::from_xywh(
                x.clamp(0.0, (w as f32 - lw).max(0.0)),
                y.clamp(0.0, (h as f32 - lh).max(0.0)),
                lw,
                lh,
            );
            while placed.iter().any(|p| Rect::intersects2(p, at)) && at.bottom < h as f32 - lh {
                at.offset((0.0, lh + 1.0));
            }
            draw_label(canvas, &m.label, at.left, at.top, px, HUES[i % HUES.len()]);
            placed.push(at);
        });
    }
    read_rgba8(&surface.image_snapshot())
}

fn layer_marks(layer: &Layer, frame: u32, depth: usize, out: &mut Vec<Mark>) -> anyhow::Result<()> {
    if !layer.visible {
        return Ok(());
    }
    match layer.kind() {
        Kind::Vector => layer.content_at(frame).map_or(Ok(()), |cel| {
            node_marks(&cel.nodes, &Matrix::new_identity(), depth, out)
        }),
        Kind::Group => layer
            .children()
            .iter()
            .try_for_each(|c| layer_marks(c, frame, depth, out)),
        Kind::Paint | Kind::Adjust => Ok(()),
    }
}

fn node_marks(nodes: &[Node], parent: &Matrix, depth: usize, out: &mut Vec<Mark>) -> anyhow::Result<()> {
    nodes.iter().filter(|n| n.visible).try_for_each(|n| {
        if depth > 1 && !n.children().is_empty() {
            let m = Matrix::concat(parent, &vector::node_matrix(n)?);
            return node_marks(n.children(), &m, depth - 1, out);
        }
        if let Some(r) = vector::node_bounds(n, parent)? {
            let label = if n.name.is_empty() {
                n.id.clone()
            } else {
                format!("{} {}", n.id, n.name)
            };
            out.push(Mark {
                label,
                rect: vector::to_box(r),
            });
        }
        Ok(())
    })
}

pub fn vector_marks(doc: &Document, frame: u32, depth: usize) -> Result<Vec<Mark>> {
    let mut out = Vec::new();
    doc.layers
        .iter()
        .try_for_each(|l| layer_marks(l, frame, depth, &mut out))?;
    Ok(out)
}

pub fn scoped_marks(doc: &Document, id: &str, frame: u32, depth: usize) -> Result<Vec<Mark>> {
    let mut out = Vec::new();
    match doc.item(id) {
        Some(klad_core::doc::Item::Layer(layer)) => layer_marks(layer, frame, depth, &mut out)?,
        Some(klad_core::doc::Item::Node(..)) => {
            let (node, matrix) = node_world(doc, id)?.ok_or_else(|| anyhow!("no node {id}"))?;
            if node.children().is_empty() {
                node_marks(std::slice::from_ref(node), &matrix, 1, &mut out)?;
            } else {
                let inner = Matrix::concat(&matrix, &vector::node_matrix(node)?);
                node_marks(node.children(), &inner, depth, &mut out)?;
            }
        }
        _ => {}
    }
    Ok(out)
}

fn find_node<'a>(nodes: &'a [Node], id: &str, parent: &Matrix) -> Result<Option<(&'a Node, Matrix)>> {
    for n in nodes {
        if n.id == id {
            return Ok(Some((n, *parent)));
        }
        if !n.children().is_empty() {
            let m = Matrix::concat(parent, &vector::node_matrix(n)?);
            if let Some(found) = find_node(n.children(), id, &m)? {
                return Ok(Some(found));
            }
        }
    }
    Ok(None)
}

pub fn node_world<'a>(doc: &'a Document, id: &str) -> Result<Option<(&'a Node, Matrix)>> {
    for layer in doc.all_layers() {
        for cel in layer.cels() {
            if let Some(found) = find_node(&cel.nodes, id, &Matrix::new_identity())? {
                return Ok(Some(found));
            }
        }
    }
    Ok(None)
}

fn node_hits(node: &Node, parent: &Matrix, point: (f32, f32)) -> Result<bool> {
    if !node.visible {
        return Ok(false);
    }
    let m = Matrix::concat(parent, &vector::node_matrix(node)?);
    let Some(inverse) = m.invert() else {
        return Ok(false);
    };
    let local = inverse.map_point(point);
    if let Some(clip) = &node.clip
        && !skia_safe::Path::from_svg(clip).is_some_and(|p| p.contains(local))
    {
        return Ok(false);
    }
    if !node.children().is_empty() {
        return vector::instances(&node.shape).iter().try_fold(false, |hit, at| {
            let placed = Matrix::concat(&m, at);
            node.children()
                .iter()
                .rev()
                .try_fold(hit, |hit, child| Ok(hit || node_hits(child, &placed, point)?))
        });
    }
    let Some(path) = vector::shape_path(&node.shape)? else {
        return Ok(false);
    };
    let filled = !matches!(&node.fill, Some(klad_core::doc::Paint::Solid(c)) if c == "none");
    if filled && path.contains(local) {
        return Ok(true);
    }
    let inside = |r: Rect| local.x >= r.left && local.x <= r.right && local.y >= r.top && local.y <= r.bottom;
    Ok(node.stroke.as_ref().is_some_and(|s| {
        let pad = s.width / 2.0 + 2.0;
        let bounds = path.compute_tight_bounds();
        inside(bounds.with_outset((pad, pad))) && !inside(bounds.with_inset((pad, pad)))
    }))
}

fn layer_hits(layer: &Layer, frame: u32, point: (f32, f32), out: &mut Vec<(String, String)>) -> Result<()> {
    if !layer.visible || layer.locked {
        return Ok(());
    }
    match layer.kind() {
        Kind::Group => layer
            .children()
            .iter()
            .rev()
            .try_for_each(|child| layer_hits(child, frame, point, out)),
        Kind::Vector => {
            let Some(cel) = layer.content_at(frame) else {
                return Ok(());
            };
            cel.nodes.iter().rev().try_for_each(|node| {
                if node_hits(node, &Matrix::new_identity(), point)? {
                    out.push((node.id.clone(), layer.id.clone()));
                }
                Ok(())
            })
        }
        Kind::Paint | Kind::Adjust => Ok(()),
    }
}

pub fn hits(doc: &Document, frame: u32, point: (f32, f32)) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    doc.layers
        .iter()
        .rev()
        .try_for_each(|layer| layer_hits(layer, frame, point, &mut out))?;
    Ok(out)
}

fn leaf_hit(node: &Node, parent: &Matrix, point: (f32, f32)) -> Result<Option<String>> {
    if !node.visible {
        return Ok(None);
    }
    if node.children().is_empty() || node.shape.generates() {
        return Ok(node_hits(node, parent, point)?.then(|| node.id.clone()));
    }
    let m = Matrix::concat(parent, &vector::node_matrix(node)?);
    if let Some(clip) = &node.clip {
        let local = m.invert().map(|inverse| inverse.map_point(point));
        if !local.is_some_and(|l| skia_safe::Path::from_svg(clip).is_some_and(|p| p.contains(l))) {
            return Ok(None);
        }
    }
    node.children().iter().rev().try_fold(None, |found, child| match found {
        Some(id) => Ok(Some(id)),
        None => leaf_hit(child, &m, point),
    })
}

pub fn leaf_hits(doc: &Document, frame: u32, point: (f32, f32)) -> Result<Vec<String>> {
    let layers: Vec<&Layer> = doc.all_layers();
    let mut out = Vec::new();
    layers
        .iter()
        .rev()
        .filter(|l| l.kind() == Kind::Vector && !l.locked && visible_path(doc, &l.id))
        .try_for_each(|layer| {
            let Some(cel) = layer.content_at(frame) else {
                return Ok::<_, anyhow::Error>(());
            };
            cel.nodes.iter().rev().try_for_each(|node| {
                if let Some(id) = leaf_hit(node, &Matrix::new_identity(), point)? {
                    out.push(id);
                }
                Ok(())
            })
        })?;
    Ok(out)
}

fn visible_path(doc: &Document, id: &str) -> bool {
    match doc.locate(id) {
        Some(klad_core::doc::Loc::Layer(path)) => (1..=path.len()).all(|n| {
            let layer = doc.layer_at(&path[..n]);
            layer.visible && !layer.locked
        }),
        _ => false,
    }
}

pub fn local_to_canvas(doc: &Document, id: &str) -> Result<Option<[f32; 6]>> {
    let Some((node, parent)) = node_world(doc, id)? else {
        return Ok(None);
    };
    let m = Matrix::concat(&parent, &vector::node_matrix(node)?);
    Ok(Some([
        m.scale_x(),
        m.skew_x(),
        m.skew_y(),
        m.scale_y(),
        m.translate_x(),
        m.translate_y(),
    ]))
}

pub fn hit(doc: &Document, frame: u32, point: (f32, f32)) -> Result<Option<(String, String)>> {
    Ok(hits(doc, frame, point)?.into_iter().next())
}

pub fn node_world_box(doc: &Document, id: &str) -> Result<Option<[f32; 4]>> {
    Ok(match node_world(doc, id)? {
        Some((node, matrix)) => vector::node_bounds(node, &matrix)?.map(vector::to_box),
        None => None,
    })
}

pub fn side_by_side(frames: &[(&Frame, &str)]) -> Result<Frame> {
    let gap = 16;
    let band = 26;
    let w: i32 = frames.iter().map(|(f, _)| f.width as i32).sum::<i32>() + gap * (frames.len() as i32 - 1).max(0);
    let h: i32 = frames.iter().map(|(f, _)| f.height as i32).max().unwrap_or(1) + band;
    let mut surface =
        surfaces::raster(&crate::f16_info(w as u32, h as u32), None, None).ok_or_else(|| anyhow!("cannot allocate"))?;
    let canvas = surface.canvas();
    canvas.clear(Color4f::new(0.12, 0.13, 0.15, 1.0));
    frames.iter().try_fold(0i32, |x, (frame, label)| {
        let info = ImageInfo::new(
            (frame.width as i32, frame.height as i32),
            ColorType::RGBA8888,
            AlphaType::Unpremul,
            ColorSpace::new_srgb(),
        );
        let image = images::raster_from_data(&info, Data::new_copy(&frame.rgba), frame.width as usize * 4)
            .ok_or_else(|| anyhow!("cannot wrap frame"))?;
        canvas.draw_image(&image, (x as f32, band as f32), None);
        draw_text(canvas, label, x as f32 + 6.0, 6.0, 2.0, [1.0, 1.0, 1.0]);
        Ok::<_, anyhow::Error>(x + frame.width as i32 + gap)
    })?;
    read_rgba8(&surface.image_snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rasters, compose_view};
    use klad_core::doc::Shape;

    fn doc() -> Document {
        let mut doc = Document::new(400, 200);
        doc.canvas.background = Some("#808080".into());
        let mut sun = Node::new(
            "n1",
            Shape::Ellipse {
                cx: 100.0,
                cy: 100.0,
                rx: 40.0,
                ry: 40.0,
            },
        )
        .filled("#ffcc00");
        sun.name = "sun".into();
        let mut group = Node::new(
            "g1",
            Shape::Group {
                children: vec![
                    Node::new(
                        "n2",
                        Shape::Rect {
                            x: 0.0,
                            y: 0.0,
                            w: 50.0,
                            h: 20.0,
                            radius: 0.0,
                        },
                    )
                    .filled("#00ff00"),
                ],
            },
        );
        group.transform.x = 250.0;
        group.transform.y = 120.0;
        doc.layers.push(Layer::vector("l1", "shapes", "c1", vec![sun, group]));
        doc
    }

    #[test]
    fn marks_follow_depth_and_scope() {
        let d = doc();
        let top = vector_marks(&d, 0, 1).unwrap();
        assert_eq!(
            top.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(),
            ["n1 sun", "g1"]
        );
        let deep = vector_marks(&d, 0, 2).unwrap();
        assert_eq!(deep[1].label, "n2");
        assert!((deep[1].rect[0] - 250.0).abs() < 0.01 && (deep[1].rect[1] - 120.0).abs() < 0.01);
        assert_eq!(scoped_marks(&d, "g1", 0, 1).unwrap()[0].label, "n2");
        assert_eq!(node_world_box(&d, "n2").unwrap().unwrap(), [250.0, 120.0, 50.0, 20.0]);
    }

    #[test]
    fn hits_pick_the_topmost_shape() {
        let d = doc();
        assert_eq!(hit(&d, 0, (100.0, 100.0)).unwrap(), Some(("n1".into(), "l1".into())));
        assert_eq!(hit(&d, 0, (260.0, 130.0)).unwrap(), Some(("g1".into(), "l1".into())));
        assert_eq!(hit(&d, 0, (350.0, 20.0)).unwrap(), None);
        let mut stacked = d.clone();
        let cover = Node::new(
            "cover",
            klad_core::doc::Shape::Rect {
                x: 0.0,
                y: 0.0,
                w: 400.0,
                h: 200.0,
                radius: 0.0,
            },
        );
        stacked.layers[0].cels_mut().unwrap()[0].nodes.push(cover);
        let ids: Vec<String> = hits(&stacked, 0, (100.0, 100.0))
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(ids, ["cover", "n1"]);
        stacked.layers[0].locked = true;
        assert!(hits(&stacked, 0, (100.0, 100.0)).unwrap().is_empty());
    }

    #[test]
    fn direct_selection_reaches_into_groups() {
        let d = doc();
        assert_eq!(leaf_hits(&d, 0, (260.0, 130.0)).unwrap(), ["n2"]);
        assert_eq!(leaf_hits(&d, 0, (100.0, 100.0)).unwrap(), ["n1"]);
        assert!(leaf_hits(&d, 0, (350.0, 20.0)).unwrap().is_empty());
        let m = local_to_canvas(&d, "n2").unwrap().unwrap();
        assert_eq!(m, [1.0, 0.0, 0.0, 1.0, 250.0, 120.0]);
        let mut hidden = d.clone();
        hidden.layers[0].visible = false;
        assert!(leaf_hits(&hidden, 0, (100.0, 100.0)).unwrap().is_empty());
    }

    #[test]
    fn overlays_draw_boxes_labels_and_grid() {
        let d = doc();
        let view = View::scaled(1.0);
        let plain = compose_view(&d, 0, &Rasters::new(), view, None, None).unwrap();
        let overlay = Overlay {
            marks: vector_marks(&d, 0, 1).unwrap(),
            boxes: true,
            ids: true,
            grid: true,
            ..Overlay::default()
        };
        let marked = compose_view(&d, 0, &Rasters::new(), view, Some(&overlay), None).unwrap();
        assert_eq!((marked.width, marked.height), (plain.width, plain.height));
        assert_ne!(marked.pixel(60, 60), plain.pixel(60, 60));
        assert_eq!(marked.pixel(100, 150), plain.pixel(100, 150));
        let pair = side_by_side(&[(&plain, "before"), (&marked, "after")]).unwrap();
        assert_eq!(pair.width, 816);
        assert!(text_width("ab", 1.0) > 0.0);
    }
}
