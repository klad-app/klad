use anyhow::{Context, Result, anyhow};
use klad_core::color::parse_hex;
use klad_core::doc::{Cap, Join, Node, Paint, Shape, StrokeStyle, Transform};
use klad_core::look::Rays;
use skia_safe::gradient::{Colors, Gradient, Interpolation, shaders};
use skia_safe::{
    Canvas, Color4f, ColorSpace, Data, FilterMode, IRect, Matrix, MipmapMode, PaintCap, PaintJoin, PaintStyle, Path,
    PathEffect, RRect, Rect, SamplingOptions, TileMode, canvas::SaveLayerRec, image_filters, images,
};

use rayon::prelude::*;

const SHARP_SIGMA: f32 = 6.0;
const MAX_FACTOR: i32 = 32;

fn color(hex: &str) -> Result<Color4f> {
    let [r, g, b, a] = parse_hex(hex)?;
    Ok(Color4f::new(r, g, b, a))
}

pub(crate) fn shader_paint(paint: &Paint) -> Result<Option<skia_safe::Paint>> {
    let mut out = skia_safe::Paint::default();
    out.set_anti_alias(true);
    match paint {
        Paint::Solid(hex) if hex == "none" => return Ok(None),
        Paint::Solid(hex) => {
            out.set_color4f(color(hex)?, &ColorSpace::new_srgb());
        }
        Paint::Linear { linear } => {
            let (colors, stops) = stops(&linear.stops)?;
            let gradient = Gradient::new(
                Colors::new(&colors, Some(&stops), TileMode::Clamp, ColorSpace::new_srgb()),
                Interpolation::default(),
            );
            let shader = shaders::linear_gradient(
                ((linear.from[0], linear.from[1]), (linear.to[0], linear.to[1])),
                &gradient,
                None,
            )
            .ok_or_else(|| {
                anyhow!(
                    "cannot build a linear gradient from {:?} to {:?}",
                    linear.from,
                    linear.to
                )
            })?;
            out.set_shader(shader);
        }
        Paint::Radial { radial } => {
            let (colors, stops) = stops(&radial.stops)?;
            let gradient = Gradient::new(
                Colors::new(&colors, Some(&stops), TileMode::Clamp, ColorSpace::new_srgb()),
                Interpolation::default(),
            );
            let shader =
                shaders::radial_gradient(((radial.center[0], radial.center[1]), radial.radius), &gradient, None)
                    .ok_or_else(|| anyhow!("cannot build a radial gradient of radius {}", radial.radius))?;
            out.set_shader(shader);
        }
    }
    Ok(Some(out))
}

fn stops(stops: &[klad_core::doc::Stop]) -> Result<(Vec<Color4f>, Vec<f32>)> {
    let colors = stops.iter().map(|s| color(&s.1)).collect::<Result<Vec<_>>>()?;
    Ok((colors, stops.iter().map(|s| s.0).collect()))
}

fn stroke_paint(stroke: &StrokeStyle) -> Result<Option<skia_safe::Paint>> {
    let Some(mut paint) = shader_paint(&stroke.paint)? else {
        return Ok(None);
    };
    paint.set_style(PaintStyle::Stroke);
    paint.set_stroke_width(stroke.width);
    paint.set_stroke_cap(match stroke.cap {
        Cap::Butt => PaintCap::Butt,
        Cap::Round => PaintCap::Round,
        Cap::Square => PaintCap::Square,
    });
    paint.set_stroke_join(match stroke.join {
        Join::Miter => PaintJoin::Miter,
        Join::Round => PaintJoin::Round,
        Join::Bevel => PaintJoin::Bevel,
    });
    if !stroke.dash.is_empty() {
        let intervals: Vec<f32> = if stroke.dash.len() % 2 == 1 {
            stroke.dash.iter().chain(&stroke.dash).copied().collect()
        } else {
            stroke.dash.clone()
        };
        paint.set_path_effect(PathEffect::dash(&intervals, 0.0));
    }
    Ok(Some(paint))
}

pub fn shape_path(shape: &Shape) -> Result<Option<Path>> {
    Ok(match shape {
        Shape::Rect { x, y, w, h, radius } => {
            let rect = Rect::from_xywh(*x, *y, *w, *h);
            Some(if *radius > 0.0 {
                Path::rrect(RRect::new_rect_xy(rect, *radius, *radius), None)
            } else {
                Path::rect(rect, None)
            })
        }
        Shape::Ellipse { cx, cy, rx, ry } => {
            Some(Path::oval(Rect::from_ltrb(cx - rx, cy - ry, cx + rx, cy + ry), None))
        }
        Shape::Path { d } => Some(Path::from_svg(d).ok_or_else(|| anyhow!("bad path data {d:?}"))?),
        Shape::Group { .. } | Shape::Scatter(_) | Shape::Repeat(_) | Shape::Mirror(_) => None,
        Shape::Image(p) => Some(Path::rect(Rect::from_xywh(p.x, p.y, p.w, p.h), None)),
        Shape::Text(t) => Some(crate::text::text_path(t)?),
        Shape::Rays(rays) => {
            let mut builder = skia_safe::PathBuilder::new();
            rays.beams().iter().for_each(|beam| {
                builder.add_polygon(&beam.quad.map(|p| skia_safe::Point::new(p[0], p[1])), true);
            });
            Some(builder.detach())
        }
    })
}

const RAY_STOPS: usize = 12;

fn draw_rays(canvas: &Canvas, node: &Node, rays: &Rays) -> Result<()> {
    let c = match &node.fill {
        Some(Paint::Solid(hex)) if hex != "none" => parse_hex(hex)?,
        _ => [1.0, 0.97, 0.86, 1.0],
    };
    let falloff = rays.falloff.max(0.05);
    let stops: Vec<f32> = (0..=RAY_STOPS).map(|i| i as f32 / RAY_STOPS as f32).collect();
    rays.beams().iter().try_for_each(|beam| {
        let colors: Vec<Color4f> = stops
            .iter()
            .map(|t| Color4f::new(c[0], c[1], c[2], c[3] * beam.strength * libm::powf(1.0 - t, falloff)))
            .collect();
        let gradient = Gradient::new(
            Colors::new(&colors, Some(&stops), TileMode::Clamp, ColorSpace::new_srgb()),
            Interpolation::default(),
        );
        let shader = shaders::radial_gradient(((rays.x, rays.y), beam.length.max(1.0)), &gradient, None)
            .ok_or_else(|| anyhow!("cannot shade a ray of length {}", beam.length))?;
        let mut paint = skia_safe::Paint::default();
        paint.set_anti_alias(true);
        paint.set_shader(shader);
        let path = Path::polygon(&beam.quad.map(|p| skia_safe::Point::new(p[0], p[1])), true, None, None);
        canvas.draw_path(&path, &paint);
        Ok(())
    })
}

fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(Rect::from_ltrb(
            a.left.min(b.left),
            a.top.min(b.top),
            a.right.max(b.right),
            a.bottom.max(b.bottom),
        )),
        (a, b) => a.or(b),
    }
}

pub fn local_bounds(node: &Node) -> Result<Option<Rect>> {
    let own = shape_bounds(node)?;
    match &node.clip {
        Some(clip) => {
            let Some(path) = Path::from_svg(clip) else {
                return Err(anyhow!("bad clip path {clip:?}"));
            };
            let c = path.compute_tight_bounds();
            Ok(own.and_then(|r| {
                let mut r = r;
                r.intersect(c).then_some(r)
            }))
        }
        None => Ok(own),
    }
}

pub fn instances(shape: &Shape) -> Vec<Matrix> {
    let list = match shape {
        Shape::Scatter(s) => s.instances(),
        Shape::Repeat(r) => r.instances(),
        Shape::Mirror(m) => m.instances(),
        _ => vec![klad_core::generators::IDENTITY],
    };
    list.iter()
        .map(|a| Matrix::new_all(a[0], a[2], a[4], a[1], a[3], a[5], 0.0, 0.0, 1.0))
        .collect()
}

fn shape_bounds(node: &Node) -> Result<Option<Rect>> {
    match &node.shape {
        shape if shape.generates() || matches!(shape, Shape::Group { .. }) => {
            instances(shape).iter().try_fold(None, |acc, m| {
                shape
                    .children()
                    .iter()
                    .try_fold(acc, |acc, child| Ok(union(acc, node_bounds(child, m)?)))
            })
        }
        shape => Ok(shape_path(shape)?.map(|p| {
            let rect = p.compute_tight_bounds();
            let pad = node.stroke.as_ref().map_or(0.0, |s| s.width / 2.0) + node.blur * 2.0;
            rect.with_outset((pad, pad))
        })),
    }
}

pub fn node_matrix(node: &Node) -> Result<Matrix> {
    let Transform {
        x,
        y,
        rotate,
        sx,
        sy,
        pivot,
    } = node.transform;
    if node.transform.is_identity() {
        return Ok(Matrix::new_identity());
    }
    let [px, py] = match pivot {
        Some(p) => p,
        None => local_bounds(node)?.map_or([0.0, 0.0], |r| [r.center_x(), r.center_y()]),
    };
    let mut m = Matrix::translate((x + px, y + py));
    m.pre_rotate(rotate, None);
    m.pre_scale((sx, sy), None);
    m.pre_translate((-px, -py));
    Ok(m)
}

pub fn node_bounds(node: &Node, parent: &Matrix) -> Result<Option<Rect>> {
    let matrix = Matrix::concat(parent, &node_matrix(node)?);
    Ok(local_bounds(node)?.map(|r| matrix.map_rect(r).0))
}

pub fn nodes_bounds(nodes: &[Node]) -> Result<Option<Rect>> {
    nodes
        .iter()
        .filter(|n| n.visible)
        .try_fold(None, |acc, n| Ok(union(acc, node_bounds(n, &Matrix::new_identity())?)))
}

pub fn draw_nodes(canvas: &Canvas, nodes: &[Node]) -> Result<()> {
    nodes
        .iter()
        .filter(|n| n.visible)
        .try_for_each(|n| draw_node(canvas, n))
}

pub fn node_reach(node: &Node) -> Result<Option<Rect>> {
    fn pad(node: &Node) -> Result<f32> {
        let own = node.stroke.as_ref().map_or(0.0, |s| s.width * 4.0) + node.blur * 4.0 + 4.0;
        let grow = instances(&node.shape)
            .iter()
            .map(Matrix::max_scale)
            .fold(1.0f32, f32::max);
        node.shape.children().iter().try_fold(own, |most, child| {
            Ok(most.max(pad(child)? * grow * node_matrix(child)?.max_scale().max(1.0)))
        })
    }
    let Some(local) = local_bounds(node)? else {
        return Ok(None);
    };
    let pad = pad(node)?;
    Ok(Some(
        node_matrix(node)?
            .map_rect(local.with_outset((pad, pad)))
            .0
            .with_outset((4.0, 4.0)),
    ))
}

pub(crate) fn draw_node(canvas: &Canvas, node: &Node) -> Result<()> {
    let context = || format!("node {}", node.id);
    let matrix = node_matrix(node).with_context(context)?;
    let depth = canvas.save();
    canvas.concat(&matrix);
    let bounds = local_bounds(node)
        .with_context(context)?
        .map(|r| r.with_outset((node.blur * 3.0 + 1.0, node.blur * 3.0 + 1.0)));
    let drawn = match bounds.and_then(|b| downsampling(canvas, node, b)) {
        Some(plan) => draw_downsampled(canvas, node, bounds, plan),
        None => {
            if node.blur > 0.0 || node.opacity < 1.0 {
                blurred_layer(canvas, node, bounds, node.opacity);
            }
            draw_content(canvas, node)
        }
    };
    canvas.restore_to_count(depth);
    drawn.with_context(context)
}

fn blurred_layer(canvas: &Canvas, node: &Node, bounds: Option<Rect>, opacity: f32) {
    let mut paint = skia_safe::Paint::default();
    paint.set_alpha_f(opacity);
    if node.blur > 0.0 {
        paint.set_image_filter(image_filters::blur((node.blur, node.blur), TileMode::Decal, None, None));
    }
    let rec = SaveLayerRec::default().paint(&paint);
    match &bounds {
        Some(b) => canvas.save_layer(&rec.bounds(b)),
        None => canvas.save_layer(&rec),
    };
}

fn draw_content(canvas: &Canvas, node: &Node) -> Result<()> {
    if let Some(clip) = &node.clip {
        let path = Path::from_svg(clip).ok_or_else(|| anyhow!("bad clip path {clip:?}"))?;
        canvas.clip_path(&path, skia_safe::ClipOp::Intersect, true);
    }
    match &node.shape {
        Shape::Group { children } => draw_nodes(canvas, children),
        Shape::Rays(rays) => draw_rays(canvas, node, rays),
        Shape::Image(p) => {
            let image = crate::assets::image(&p.src)?;
            canvas.draw_image_rect_with_sampling_options(
                &image,
                None,
                Rect::from_xywh(p.x, p.y, p.w, p.h),
                SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear),
                &skia_safe::Paint::default(),
            );
            Ok(())
        }
        shape if shape.generates() => instances(shape).iter().try_for_each(|m| {
            let depth = canvas.save();
            canvas.concat(m);
            let drawn = draw_nodes(canvas, shape.children());
            canvas.restore_to_count(depth);
            drawn
        }),
        shape => {
            let Some(path) = shape_path(shape)? else {
                return Ok(());
            };
            let fill = node.fill.clone().unwrap_or(Paint::Solid("#000000".into()));
            if let Some(paint) = shader_paint(&fill)? {
                canvas.draw_path(&path, &paint);
            }
            if let Some(paint) = node.stroke.as_ref().map(stroke_paint).transpose()?.flatten() {
                canvas.draw_path(&path, &paint);
            }
            Ok(())
        }
    }
}

struct Downsample {
    area: IRect,
    factor: i32,
    reach: f32,
}

fn downsampling(canvas: &Canvas, node: &Node, bounds: Rect) -> Option<Downsample> {
    let ctm = canvas.local_to_device_as_3x3();
    let (low, high) = (ctm.min_scale(), ctm.max_scale());
    if node.blur <= 0.0 || ctm.has_perspective() || low <= 0.0 || high > low * 1.001 {
        return None;
    }
    let reach = node.blur * high;
    let ratio = (reach / SHARP_SIGMA) as u32;
    if ratio < 2 {
        return None;
    }
    let factor = (1i32 << ratio.ilog2()).min(MAX_FACTOR);
    let spread = (reach * 3.0).ceil() as i32 + factor;
    let clip = canvas.device_clip_bounds()?.with_outset((spread, spread));
    let (mapped, _) = ctm.map_rect(bounds);
    let snap_down = |v: f32| (v.floor() as i32).div_euclid(factor) * factor;
    let snap_up = |v: f32| -(((-v).floor() as i32).div_euclid(factor) * factor);
    let area = IRect::from_ltrb(
        snap_down(mapped.left).max(snap_down(clip.left as f32)),
        snap_down(mapped.top).max(snap_down(clip.top as f32)),
        snap_up(mapped.right).min(snap_up(clip.right as f32)),
        snap_up(mapped.bottom).min(snap_up(clip.bottom as f32)),
    );
    (!area.is_empty()).then_some(Downsample { area, factor, reach })
}

fn draw_downsampled(canvas: &Canvas, node: &Node, bounds: Option<Rect>, plan: Downsample) -> Result<()> {
    let Downsample { area, factor, reach } = plan;
    let (width, height) = ((area.width() / factor) as usize, (area.height() / factor) as usize);
    let info = crate::f16_info(width as u32, height as u32);
    let Some(mut surface) = canvas.new_surface(&info, None) else {
        blurred_layer(canvas, node, bounds, node.opacity);
        return draw_content(canvas, node);
    };
    let small = surface.canvas();
    small.clear(Color4f::new(0.0, 0.0, 0.0, 0.0));
    let scale = 1.0 / factor as f32;
    small.scale((scale, scale));
    small.translate((-area.left as f32, -area.top as f32));
    small.concat(&canvas.local_to_device_as_3x3());
    draw_content(small, node)?;
    let mut pixels = vec![half::f16::ZERO; width * height * 4];
    if !surface.read_pixels(&info, bytemuck::cast_slice_mut(&mut pixels), width * 8, (0, 0)) {
        return Err(anyhow!("cannot read the downsampled layer"));
    }
    let k = factor as f32;
    let sigma = (reach * reach - k * k / 4.0).max(1.0).sqrt() / k;
    let blurred = gaussian_blur(
        &pixels.iter().map(|v| v.to_f32()).collect::<Vec<_>>(),
        width,
        height,
        sigma,
    );
    let packed: Vec<half::f16> = blurred.iter().map(|v| half::f16::from_f32(*v)).collect();
    let image = images::raster_from_data(&info, Data::new_copy(bytemuck::cast_slice(&packed)), width * 8)
        .ok_or_else(|| anyhow!("cannot wrap the blurred layer"))?;
    let placement = Matrix::translate((area.left as f32, area.top as f32)) * Matrix::scale((k, k));
    let shader = image
        .to_shader(
            (TileMode::Decal, TileMode::Decal),
            SamplingOptions::new(FilterMode::Linear, MipmapMode::None),
            &placement,
        )
        .ok_or_else(|| anyhow!("cannot sample the blurred layer"))?;
    let mut paint = skia_safe::Paint::default();
    paint.set_shader(shader);
    paint.set_alpha_f(node.opacity);
    canvas.save();
    canvas.reset_matrix();
    canvas.draw_rect(Rect::from(area), &paint);
    canvas.restore();
    Ok(())
}

fn gaussian_blur(pixels: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let radius = (sigma * 3.0).ceil() as isize;
    let weights: Vec<f32> = (-radius..=radius)
        .map(|i| libm::expf(-((i * i) as f32) / (2.0 * sigma * sigma)))
        .collect();
    let total: f32 = weights.iter().sum();
    let kernel: Vec<f32> = weights.iter().map(|w| w / total).collect();
    let (quads, _) = pixels.as_chunks::<4>();
    let rows = blur_rows_transposed(quads, width, &kernel);
    blur_rows_transposed(&rows, height, &kernel)
        .into_iter()
        .flatten()
        .collect()
}

fn blur_rows_transposed(src: &[[f32; 4]], width: usize, kernel: &[f32]) -> Vec<[f32; 4]> {
    let reach = kernel.len() / 2;
    let rows: Vec<Vec<[f32; 4]>> = src
        .par_chunks_exact(width)
        .map(|row| {
            (0..width)
                .map(|x| {
                    let lo = x.saturating_sub(reach);
                    let hi = (x + reach).min(width - 1);
                    let taps = &kernel[lo + reach - x..=hi + reach - x];
                    row[lo..=hi].iter().zip(taps).fold([0.0f32; 4], |a, (p, w)| {
                        [a[0] + p[0] * w, a[1] + p[1] * w, a[2] + p[2] * w, a[3] + p[3] * w]
                    })
                })
                .collect()
        })
        .collect();
    (0..width).flat_map(|x| rows.iter().map(move |row| row[x])).collect()
}

pub fn pivot_pins(doc: &klad_core::doc::Document, id: &str) -> Result<Vec<klad_core::ops::Op>> {
    let Some(klad_core::doc::Loc::Node(layer, cel, path)) = doc.locate(id) else {
        return Ok(Vec::new());
    };
    let mut list: &[Node] = &doc.layer_at(&layer).cels()[cel].nodes;
    let mut out = Vec::new();
    for &i in &path {
        let node = &list[i];
        if !node.transform.is_identity()
            && node.transform.pivot.is_none()
            && let Some(r) = local_bounds(node)?
        {
            out.push(klad_core::ops::Op::Set {
                id: node.id.clone(),
                props: serde_json::Map::from_iter([(
                    "transform.pivot".to_string(),
                    serde_json::json!([r.center_x(), r.center_y()]),
                )]),
            });
        }
        list = node.children();
    }
    Ok(out)
}

pub fn pinned(doc: &klad_core::doc::Document, ops: &[klad_core::ops::Op]) -> Result<Vec<klad_core::ops::Op>> {
    ops.iter().try_fold(Vec::new(), |mut out, op| {
        if let klad_core::ops::Op::Path { id, .. } = op {
            out.extend(pivot_pins(doc, id)?);
        }
        out.push(op.clone());
        Ok(out)
    })
}

pub fn to_box(rect: Rect) -> [f32; 4] {
    [rect.left, rect.top, rect.width(), rect.height()]
}

pub fn node_box(node: &Node) -> Result<Option<[f32; 4]>> {
    Ok(node_bounds(node, &Matrix::new_identity())?.map(to_box))
}

pub fn nodes_box(nodes: &[Node]) -> Result<Option<[f32; 4]>> {
    Ok(nodes_bounds(nodes)?.map(to_box))
}

#[cfg(test)]
mod tests {
    use super::*;
    use skia_safe::surfaces;

    fn pixels(surface: &mut skia_safe::Surface) -> Vec<f32> {
        let info = surface.image_info();
        let mut out = vec![half::f16::ZERO; (info.width() * info.height() * 4) as usize];
        assert!(surface.read_pixels(
            &info,
            bytemuck::cast_slice_mut(&mut out),
            info.width() as usize * 8,
            (0, 0)
        ));
        out.iter().map(|v| v.to_f32()).collect()
    }

    fn render(nodes: &[Node], size: i32, origin: (f32, f32)) -> Vec<f32> {
        let mut surface = surfaces::raster(&crate::f16_info(size as u32, size as u32), None, None).unwrap();
        surface.canvas().translate((-origin.0, -origin.1));
        draw_nodes(surface.canvas(), nodes).unwrap();
        pixels(&mut surface)
    }

    fn exact(node: &Node, size: i32) -> Vec<f32> {
        let info = crate::f16_info(size as u32, size as u32);
        let mut layer = surfaces::raster(&info, None, None).unwrap();
        let canvas = layer.canvas();
        canvas.concat(&node_matrix(node).unwrap());
        let reach = node.blur * canvas.local_to_device_as_3x3().max_scale();
        draw_content(canvas, node).unwrap();
        let blurred = gaussian_blur(&pixels(&mut layer), size as usize, size as usize, reach);
        blurred
            .iter()
            .map(|v| half::f16::from_f32(v * node.opacity).to_f32())
            .collect()
    }

    fn skia(node: &Node, size: i32) -> Vec<f32> {
        let mut surface = surfaces::raster(&crate::f16_info(size as u32, size as u32), None, None).unwrap();
        let canvas = surface.canvas();
        canvas.concat(&node_matrix(node).unwrap());
        let bounds = local_bounds(node)
            .unwrap()
            .map(|r| r.with_outset((node.blur * 3.0 + 1.0, node.blur * 3.0 + 1.0)));
        blurred_layer(canvas, node, bounds, node.opacity);
        draw_content(canvas, node).unwrap();
        canvas.restore_to_count(1);
        pixels(&mut surface)
    }

    fn mist() -> Vec<Node> {
        let mut band = Node::new(
            "band",
            Shape::Ellipse {
                cx: 256.0,
                cy: 250.0,
                rx: 220.0,
                ry: 40.0,
            },
        );
        band.fill = Some(Paint::Solid("#c8d4e6".into()));
        band.blur = 42.0;
        band.opacity = 0.8;
        let mut tilted = Node::new(
            "tilted",
            Shape::Rect {
                x: 150.0,
                y: 120.0,
                w: 200.0,
                h: 60.0,
                radius: 0.0,
            },
        );
        tilted.fill = Some(Paint::Solid("#40306a".into()));
        tilted.blur = 12.0;
        tilted.transform.rotate = 25.0;
        tilted.transform.sx = 1.5;
        tilted.transform.sy = 1.5;
        vec![band, tilted]
    }

    fn worst(a: &[f32], b: &[f32]) -> (usize, f32) {
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .enumerate()
            .fold((0, 0.0f32), |acc, (i, d)| if d > acc.1 { (i, d) } else { acc })
    }

    fn erf(x: f64) -> f64 {
        let t = 1.0 / (1.0 + 0.5 * x.abs());
        let poly = -1.26551223
            + t * (1.00002368
                + t * (0.37409196
                    + t * (0.09678418
                        + t * (-0.18628806
                            + t * (0.27886807
                                + t * (-1.13520398 + t * (1.48851587 + t * (-0.82215223 + t * 0.17087277))))))));
        let y = 1.0 - t * (-x * x + poly).exp();
        if x >= 0.0 { y } else { -y }
    }

    #[test]
    fn path_edits_pin_the_pivots_that_would_move() {
        use klad_core::doc::{Document, Layer};
        use klad_core::ops::{apply_all, parse_lines};
        let mut doc = Document::new(400, 400);
        doc.layers.push(Layer::vector("v", "v", "c1", Vec::new()));
        let (doc, _) = apply_all(
            &doc,
            &parse_lines(
                r#"{"op":"add","node":{"id":"g","type":"group","transform":{"rotate":30},"children":[{"id":"p","type":"path","d":"M100 100 L200 100 L150 200 Z","transform":{"sx":2}}]},"in":"v"}"#,
            )
            .unwrap(),
        )
        .unwrap();
        let before = crate::overlay::node_world_box(&doc, "p").unwrap().unwrap();
        let edit = parse_lines(r#"{"op":"path","id":"p","move":2,"to":[150,300]}"#).unwrap();
        let ops = pinned(&doc, &edit).unwrap();
        assert_eq!(ops.len(), 3);
        let (moved, _) = apply_all(&doc, &ops).unwrap();
        let (unpinned, _) = apply_all(&doc, &edit).unwrap();
        let top = |d: &klad_core::doc::Document| crate::overlay::node_world_box(d, "p").unwrap().unwrap()[1];
        assert!((top(&moved) - before[1]).abs() > 1.0 || (top(&unpinned) - before[1]).abs() > 1.0);
        let corner = |d: &klad_core::doc::Document| {
            let (node, parent) = crate::overlay::node_world(d, "p").unwrap().unwrap();
            Matrix::concat(&parent, &node_matrix(node).unwrap()).map_point((100.0, 100.0))
        };
        let (a, b) = (corner(&doc), corner(&moved));
        assert!((a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01, "{a:?} {b:?}");
        let drifted = corner(&unpinned);
        assert!((a.x - drifted.x).abs() + (a.y - drifted.y).abs() > 1.0);
    }

    #[test]
    fn big_blurs_match_an_exact_gaussian() {
        mist().into_iter().for_each(|node| {
            let truth = exact(&node, 512);
            let fast = render(std::slice::from_ref(&node), 512, (0.0, 0.0));
            let (at, error) = worst(&truth, &fast);
            assert!(
                error < 1.5 / 255.0,
                "{} off by {:.2}/255 at ({}, {})",
                node.id,
                error * 255.0,
                at / 4 % 512,
                at / 4 / 512
            );
        });
    }

    #[test]
    fn big_blurs_follow_the_exact_edge_profile_closer_than_skia() {
        [12.0f32, 20.0, 30.0, 45.0].iter().for_each(|&sigma| {
            let mut wall = Node::new(
                "wall",
                Shape::Rect {
                    x: 100.0,
                    y: -2000.0,
                    w: 312.0,
                    h: 5000.0,
                    radius: 0.0,
                },
            );
            wall.fill = Some(Paint::Solid("#ffffff".into()));
            wall.blur = sigma;
            let spread = sigma as f64 * std::f64::consts::SQRT_2;
            let score = |v: &[f32]| {
                (0..512usize)
                    .map(|x| {
                        let c = x as f64 + 0.5;
                        let truth = 0.5 * (erf((c - 100.0) / spread) - erf((c - 412.0) / spread));
                        (v[(256 * 512 + x) * 4 + 3] as f64 - truth).abs() * 255.0
                    })
                    .fold(0.0f64, f64::max)
            };
            let fast = score(&render(std::slice::from_ref(&wall), 512, (0.0, 0.0)));
            let reference = score(&skia(&wall, 512));
            assert!(
                fast < 1.0 && fast < reference,
                "sigma {sigma}: {fast:.2}/255 off, Skia {reference:.2}/255"
            );
        });
    }

    #[test]
    fn stretched_blurs_keep_the_skia_path() {
        let mut node = mist().remove(1);
        node.transform.sy = 0.6;
        let mut surface = surfaces::raster(&crate::f16_info(64, 64), None, None).unwrap();
        surface.canvas().concat(&node_matrix(&node).unwrap());
        assert!(downsampling(surface.canvas(), &node, Rect::from_xywh(0.0, 0.0, 10.0, 10.0)).is_none());
        let fast = render(std::slice::from_ref(&node), 512, (0.0, 0.0));
        assert_eq!(worst(&fast, &skia(&node, 512)).1, 0.0);
    }

    #[test]
    fn big_blurs_line_up_across_tiles() {
        let nodes = mist();
        let whole = render(&nodes, 512, (0.0, 0.0));
        [(0usize, 0usize), (256, 0), (0, 256), (256, 256)]
            .iter()
            .for_each(|&(ox, oy)| {
                let tile = render(&nodes, 256, (ox as f32, oy as f32));
                let crop: Vec<f32> = (0..256usize)
                    .flat_map(|y| {
                        let start = ((y + oy) * 512 + ox) * 4;
                        whole[start..start + 256 * 4].to_vec()
                    })
                    .collect();
                let (at, error) = worst(&tile, &crop);
                assert!(
                    error < 1.0 / 255.0,
                    "tile at ({ox},{oy}) off by {:.2}/255 at ({}, {})",
                    error * 255.0,
                    at / 4 % 256,
                    at / 4 / 256
                );
            });
    }
}
