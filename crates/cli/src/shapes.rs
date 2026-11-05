use anyhow::{Context, Result, bail};
use klad_core::bundle::Bundle;
use klad_core::color;
use klad_core::doc::Document;
use klad_core::ops::{
    FillSpec, FoliageSpec, HatchSpec, InkSpec, LightSpec, Op, PaintSpec, ScatterSpec, StrokeSpec, apply_all,
};
use klad_render::form::{self, Form, FormRules, Lighting, Shadow};
use klad_render::shapes::{self, HatchRules, InkRules, Outline, P, Run, ScatterRules};
use serde_json::json;

#[allow(clippy::too_many_arguments)]
fn stroke(
    layer: &str,
    frame: u32,
    brush: &str,
    color: &str,
    size: f32,
    opacity: f32,
    taper: Option<[f32; 2]>,
    seed: u32,
    run: &Run,
    part: &Option<String>,
) -> Result<Op> {
    let mut value = json!({
        "layer": layer,
        "frame": frame,
        "brush": brush,
        "color": color,
        "size": size,
        "opacity": opacity,
        "points": run,
        "seed": seed,
    });
    if let Some(taper) = taper {
        value["taper"] = json!(taper);
    }
    if let Some(part) = part {
        value["part"] = json!(part);
    }
    let spec: StrokeSpec = serde_json::from_value(value)?;
    Ok(Op::Stroke(spec))
}

fn handed(run: &Run, taper: Option<[f32; 2]>, seed: u32, strength: f32, bow: bool) -> (Run, f32, Option<[f32; 2]>) {
    let mut rng = klad_core::rng::Rng::new(seed as u64 * 2_654_435_761 + 101);
    let h = shapes::hand(run, &mut rng, strength, bow);
    let taper = match (taper, strength > 0.0) {
        (Some([a, b]), true) if a == 0.0 && b == 0.0 => Some(h.taper),
        (None, true) => Some(h.taper),
        (given, _) => given,
    };
    (h.run, h.width, taper)
}

fn closed(outlines: &[Outline]) -> Vec<Vec<P>> {
    outlines
        .iter()
        .filter(|o| o.closed && o.points.len() >= 3)
        .map(|o| o.points.clone())
        .collect()
}

fn areas(doc: &Document, within: &[String], polygon: &Option<Vec<[f32; 2]>>, what: &str) -> Result<Vec<Vec<P>>> {
    let mut found = closed(&shapes::outlines(doc, within)?);
    found.extend(polygon.clone().filter(|p| p.len() >= 3));
    if found.is_empty() {
        bail!("{what} needs \"in\": closed shapes (rects, ellipses, closed paths, groups of them) or \"polygon\"");
    }
    Ok(found)
}

fn ink(doc: &Document, spec: &InkSpec) -> Result<Vec<Op>> {
    let outlines = match (&spec.path, spec.from.is_empty()) {
        (Some(d), _) => shapes::path_outlines(d)?,
        (None, false) => shapes::outlines(doc, &spec.from)?,
        (None, true) => bail!("ink needs \"from\": the ids of vector shapes, or \"path\": SVG path data"),
    };
    let rules = InkRules {
        weight: spec.weight,
        light: spec.light,
        break_deg: spec.break_deg,
        gap: spec.gap,
        wobble: spec.wobble,
        seed: spec.seed,
    };
    shapes::ink(&outlines, &rules)
        .iter()
        .enumerate()
        .map(|(i, run)| {
            let seed = spec.seed.wrapping_mul(1009).wrapping_add(i as u32);
            let (run, width, taper) = handed(run, spec.taper, seed, spec.hand, false);
            stroke(
                &spec.layer,
                spec.frame,
                &spec.brush,
                &spec.color,
                spec.weight * width,
                spec.opacity,
                taper,
                seed,
                &run,
                &spec.part,
            )
        })
        .collect()
}

fn fill(polygon: Vec<P>, outside: Option<Vec<P>>, base: &FillSpec, opacity: f32) -> Op {
    Op::Fill(FillSpec {
        at: None,
        polygon: Some(polygon),
        shape: None,
        outside,
        opacity,
        ..base.clone()
    })
}

fn light(doc: &Document, spec: &LightSpec) -> Result<Vec<Op>> {
    if spec.shade.is_none() && spec.rim.is_none() {
        bail!("light needs \"shade\": a layer for the shade, \"rim\": a layer for the rim light, or both");
    }
    let outlines = shapes::outlines(doc, &spec.from)?;
    if closed(&outlines).is_empty() {
        bail!("light needs closed shapes in \"from\"");
    }
    let base = |layer: &str, color: &str| FillSpec {
        frame: spec.frame,
        color: color.to_string(),
        ..FillSpec::seeded(layer, [0.0, 0.0])
    };
    let groups: Vec<(Vec<Outline>, P)> = match spec.lamp {
        None => vec![(outlines.clone(), spec.light)],
        Some(at) => outlines
            .iter()
            .filter(|o| o.closed && o.points.len() >= 3)
            .map(|o| {
                let n = o.points.len() as f32;
                let c = o
                    .points
                    .iter()
                    .fold([0.0, 0.0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
                (vec![o.clone()], [at[0] - c[0], at[1] - c[1]])
            })
            .collect(),
    };
    let shade: Vec<Op> = spec
        .shade
        .iter()
        .flat_map(|layer| {
            let base = base(layer, &spec.shade_color);
            groups
                .iter()
                .flat_map(|(group, light)| shapes::shade(group, *light, spec.depth, spec.soft as usize))
                .map(|c| fill(c.polygon, Some(c.outside), &base, spec.shade_opacity * c.share))
                .collect::<Vec<_>>()
        })
        .collect();
    let rim: Vec<Op> = spec
        .rim
        .iter()
        .flat_map(|layer| {
            let base = base(layer, &spec.rim_color);
            groups
                .iter()
                .flat_map(|(group, light)| shapes::rim(group, *light, spec.rim_width))
                .map(|c| fill(c.polygon, Some(c.outside), &base, spec.rim_opacity))
                .collect::<Vec<_>>()
        })
        .collect();
    Ok(shade.into_iter().chain(rim).collect())
}

fn shape_fill(doc: &Document, spec: &FillSpec, id: &str) -> Result<Vec<Op>> {
    let rings = closed(&shapes::outlines(doc, &[id.to_string()])?);
    if rings.is_empty() {
        bail!("{id} has no closed outline to fill");
    }
    Ok(rings
        .into_iter()
        .map(|ring| {
            fill(
                shapes::offset(&ring, spec.expand),
                spec.outside.clone(),
                spec,
                spec.opacity,
            )
        })
        .collect())
}

fn hatch(doc: &Document, spec: &HatchSpec) -> Result<Vec<Op>> {
    let found = areas(doc, &spec.within, &spec.polygon, "hatch")?;
    let rules = HatchRules {
        angle: spec.angle,
        along: spec.along,
        spacing: spec.spacing.max(1.0),
        length: (spec.length[0], spec.length[1]),
        jitter: spec.jitter,
        gap: shapes::HATCH_GAP,
        seed: spec.seed,
    };
    shapes::hatch(&found, &rules)
        .iter()
        .enumerate()
        .map(|(i, run)| {
            let seed = spec.seed.wrapping_mul(7919).wrapping_add(i as u32);
            let (run, width, taper) = handed(run, Some([0.0, 0.0]), seed, spec.hand, true);
            stroke(
                &spec.layer,
                spec.frame,
                &spec.brush,
                &spec.color,
                spec.weight * width,
                spec.opacity,
                taper,
                seed,
                &run,
                &spec.part,
            )
        })
        .collect()
}

fn linear(doc: &Document, text: &str) -> Result<[f32; 3]> {
    Ok(color::to_linear(color::parse_hex(&doc.resolve_color(text)?)?))
}

fn form_rules(doc: &Document, spec: &PaintSpec) -> Result<FormRules> {
    let form = match spec.form.as_str() {
        "round" => Form::Round,
        "flat" => Form::Flat { facing: spec.facing },
        "cylinder" => Form::Cylinder { axis: spec.axis },
        other => bail!("paint forms are round, flat and cylinder, not {other:?}"),
    };
    let light = match spec.lamp {
        Some(at) => Lighting::Lamp { at },
        None => Lighting::Direction {
            toward: [spec.light[0], spec.light[1], spec.elevation],
        },
    };
    let scaled = |text: &str, k: f32| -> Result<[f32; 3]> { Ok(linear(doc, text)?.map(|c| c * k)) };
    let shadow = match &spec.shadow {
        Some(text) => Some(Shadow::of(
            color::parse_hex(&doc.resolve_color(text)?)?,
            spec.shadow_mix,
        )),
        None => None,
    };
    Ok(FormRules {
        form,
        light,
        colour: linear(doc, &spec.color)?,
        key: linear(doc, &spec.key)?,
        fill: scaled(&spec.fill, spec.fill_strength)?,
        bounce: scaled(&spec.bounce, spec.bounce_strength)?,
        core: spec.core,
        weight: spec.weight,
        angle: spec.angle,
        decisive: spec.decisive,
        vary: spec.vary,
        seed: spec.seed,
        spill: doc.layer(&spec.layer).is_some_and(|l| l.clip || l.lock_alpha),
        shadow,
    })
}

fn paint(doc: &Document, spec: &PaintSpec) -> Result<Vec<Op>> {
    let found = areas(doc, &spec.within, &spec.polygon, "paint")?;
    let rules = form_rules(doc, spec)?;
    let bow = matches!(rules.form, Form::Flat { .. });
    form::paint_form(&found, &rules)
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let seed = spec.seed.wrapping_mul(15_485_863).wrapping_add(i as u32);
            let (run, width, taper) = handed(&s.run, Some([0.0, 0.0]), seed, spec.hand * s.hand, bow);
            stroke(
                &spec.layer,
                spec.frame,
                if s.bold { &spec.bold } else { &spec.brush },
                &color::to_hex(color::from_linear(s.rgb, 1.0)),
                s.size * width,
                spec.opacity * s.opacity,
                taper,
                seed,
                &run,
                &spec.part,
            )
        })
        .collect()
}

fn scatter(doc: &Document, spec: &ScatterSpec) -> Result<Vec<Op>> {
    let found = areas(doc, &spec.within, &spec.polygon, "scatter")?;
    let avoid = closed(&shapes::outlines(doc, &spec.avoid)?);
    let mark = shapes::mark_lines(&spec.mark).with_context(|| format!("the mark {:?}", spec.mark))?;
    let rules = ScatterRules {
        count: spec.count,
        spacing: spec.spacing,
        size: (spec.size[0], spec.size[1]),
        turn: (spec.turn[0], spec.turn[1]),
        seed: spec.seed,
    };
    shapes::scatter(&found, &avoid, &mark, &rules)
        .iter()
        .enumerate()
        .map(|(i, run)| {
            let seed = spec.seed.wrapping_mul(104_729).wrapping_add(i as u32);
            let (run, width, taper) = handed(run, Some([0.0, 0.0]), seed, spec.hand, false);
            stroke(
                &spec.layer,
                spec.frame,
                &spec.brush,
                &spec.color,
                spec.weight * width,
                spec.opacity,
                taper,
                seed,
                &run,
                &spec.part,
            )
        })
        .collect()
}

fn inside(polygons: &[Vec<P>], at: P) -> bool {
    polygons.iter().fold(false, |odd, poly| {
        let crossings = (0..poly.len())
            .filter(|&i| {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                (a[1] > at[1]) != (b[1] > at[1]) && at[0] < a[0] + (at[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
            })
            .count();
        odd ^ (crossings % 2 == 1)
    })
}

fn resample(poly: &[Vec<P>], step: f32) -> Vec<(P, P)> {
    poly.iter()
        .flat_map(|points| {
            let n = points.len();
            let mut out = Vec::new();
            let mut carry = 0.0;
            (0..n).for_each(|i| {
                let (a, b) = (points[i], points[(i + 1) % n]);
                let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                if len < 1e-4 {
                    return;
                }
                let t = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
                let mut d = carry;
                while d < len {
                    out.push(([a[0] + t[0] * d, a[1] + t[1] * d], t));
                    d += step;
                }
                carry = d - len;
            });
            out
        })
        .collect()
}

fn mix_lab(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn foliage(doc: &Document, spec: &FoliageSpec) -> Result<Vec<Op>> {
    let found = areas(doc, &spec.within, &None, "foliage")?;
    let colour = |text: &str| -> Result<[f32; 3]> { Ok(color::to_oklab(color::parse_hex(&doc.resolve_color(text)?)?)) };
    let mid = colour(&spec.color)?;
    let given = |c: &Option<String>, shift: [f32; 3]| -> Result<[f32; 3]> {
        match c {
            Some(c) => colour(c),
            None => Ok(std::array::from_fn(|i| mid[i] + shift[i])),
        }
    };
    let lit = given(&spec.lit, [0.15, -0.012, 0.05])?;
    let shade = given(&spec.shade, [-0.2, -0.01, -0.045])?;
    let glow = given(&spec.glow, [0.1, -0.03, 0.09])?;
    let hex = |lab: [f32; 3]| color::to_hex(color::from_oklab(lab, 1.0));
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    found.iter().flatten().for_each(|p| {
        (x0, y0, x1, y1) = (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1]));
    });
    let (cx, cy, rx, ry) = (
        (x0 + x1) / 2.0,
        (y0 + y1) / 2.0,
        ((x1 - x0) / 2.0).max(1.0),
        ((y1 - y0) / 2.0).max(1.0),
    );
    let crown = match &spec.mass {
        Some(id) => {
            let outline = areas(doc, std::slice::from_ref(id), &None, "foliage's mass")?;
            let (mut a0, mut b0, mut a1, mut b1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            outline.iter().flatten().for_each(|p| {
                (a0, b0, a1, b1) = (a0.min(p[0]), b0.min(p[1]), a1.max(p[0]), b1.max(p[1]));
            });
            Some((
                (a0 + a1) / 2.0,
                (b0 + b1) / 2.0,
                ((a1 - a0) / 2.0).max(1.0),
                ((b1 - b0) / 2.0).max(1.0),
            ))
        }
        None => None,
    };
    let bulge = |p: P, (cx, cy, rx, ry): (f32, f32, f32, f32)| -> [f32; 3] {
        let (u, v) = ((p[0] - cx) / rx, (p[1] - cy) / ry);
        let flat = (u * u + v * v).min(1.0);
        let n = [u * 0.9, v * 0.9, (1.0 - flat).max(0.0).sqrt() + 0.15];
        let d = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        [n[0] / d, n[1] / d, n[2] / d]
    };
    let blended = |p: P, own: [f32; 3]| -> [f32; 3] {
        match crown {
            Some(c) => {
                let big = bulge(p, c);
                let n: [f32; 3] = std::array::from_fn(|i| own[i] * 0.45 + big[i] * 0.55);
                let d = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-4);
                [n[0] / d, n[1] / d, n[2] / d]
            }
            None => own,
        }
    };
    let flat = (spec.light[0].powi(2) + spec.light[1].powi(2)).sqrt().max(1e-4);
    let sun = [spec.light[0] / flat * 0.8, spec.light[1] / flat * 0.8, 0.6];
    let towards = |p: P| -> [f32; 3] {
        match spec.lamp {
            Some([lx, ly, lz]) => {
                let v = [lx - p[0], ly - p[1], lz.max(1.0)];
                let d = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-4);
                [v[0] / d, v[1] / d, v[2] / d]
            }
            None => sun,
        }
    };
    let tone_at = |p: P, normal: [f32; 3]| -> f32 {
        let l = towards(p);
        let lambert = normal[0] * l[0] + normal[1] * l[1] + normal[2] * l[2];
        let under = normal[1].max(0.0) * 0.4;
        (0.42 + 0.7 * lambert - under).clamp(0.0, 1.0)
    };
    let mut rng = klad_core::rng::Rng::new(spec.seed as u64 * 7_919 + 13);
    let mut leaves: Vec<(f32, Op)> = Vec::new();
    let mut leaf = |rng: &mut klad_core::rng::Rng, at: P, out: P, size: f32, tone: f32, shine: bool| -> Result<()> {
        let stepped = (tone * 3.0).floor().min(2.0) / 3.0 + 1.0 / 6.0;
        let tone = (stepped + (tone - stepped) * 0.35 + rng.signed() * 0.05 * (0.2 + tone)).clamp(0.0, 1.0);
        let lab = match (shine, tone < 0.5) {
            (true, _) => glow,
            (false, true) => mix_lab(shade, mid, (tone * 2.0).clamp(0.0, 1.0)),
            (false, false) => mix_lab(mid, lit, (tone * 2.0 - 1.0).clamp(0.0, 1.0)),
        };
        let spread = 0.015 + 0.03 * tone;
        let lab = [
            lab[0] + rng.signed() * spread,
            lab[1] + rng.signed() * 0.006,
            lab[2] + rng.signed() * 0.01,
        ];
        let turn = rng.signed() * 0.9;
        let (s, c) = (turn.sin(), turn.cos());
        let d = [out[0] * c - out[1] * s, out[0] * s + out[1] * c];
        let reach = size * (0.3 + 0.2 * (rng.signed() + 1.0));
        let a = [at[0] - d[0] * size * 0.3, at[1] - d[1] * size * 0.3];
        let b = [at[0] + d[0] * reach, at[1] + d[1] * reach];
        let run: Run = vec![[a[0], a[1], 0.8], [b[0], b[1], 1.0]];
        let seed = spec.seed.wrapping_mul(2_750_159).wrapping_add(leaves.len() as u32 * 31);
        let op = stroke(
            &spec.layer,
            spec.frame,
            &spec.brush,
            &hex(lab),
            size,
            1.0,
            Some([0.0, 0.0]),
            seed,
            &run,
            &spec.part,
        )?;
        leaves.push((tone, op));
        Ok(())
    };
    if spec.body {
        let step = spec.leaf * 0.6 / spec.density.max(0.05);
        let (cols, rows) = (((x1 - x0) / step).ceil() as i32, ((y1 - y0) / step).ceil() as i32);
        (0..rows)
            .flat_map(|r| (0..cols).map(move |c| (r, c)))
            .try_for_each(|(r, c)| {
                let p = [
                    x0 + (c as f32 + 0.5 + rng.signed() * 0.45) * step,
                    y0 + (r as f32 + 0.5 + rng.signed() * 0.45) * step,
                ];
                if !inside(&found, p) {
                    return Ok::<_, anyhow::Error>(());
                }
                let (u, v) = ((p[0] - cx) / rx, (p[1] - cy) / ry);
                let normal = blended(p, bulge(p, (cx, cy, rx, ry)));
                let out = {
                    let d = (u * u + v * v).sqrt().max(1e-3);
                    [u / d, v / d]
                };
                let size = spec.leaf * (0.75 + 0.2 * (rng.signed() + 1.0));
                leaf(&mut rng, p, out, size, tone_at(p, normal), false)
            })?;
    }
    let step = spec.leaf * 0.4 / spec.density.max(0.05);
    resample(&found, step).into_iter().try_for_each(|(p, t)| {
        let probe = (spec.leaf * 0.25).max(1.0);
        let mut n = [t[1], -t[0]];
        if inside(&found, [p[0] + n[0] * probe, p[1] + n[1] * probe]) {
            n = [-n[0], -n[1]];
        }
        let normal = blended(p, [n[0] * 0.92, n[1] * 0.92, 0.38]);
        let tone = tone_at(p, normal);
        let up = (-n[1]).max(0.0);
        let keep = (0.5 + 0.5 * tone + 0.25 * up).min(1.0);
        if (rng.signed() + 1.0) / 2.0 > keep {
            return Ok::<_, anyhow::Error>(());
        }
        let size = spec.leaf * (0.75 + 0.25 * (rng.signed() + 1.0));
        let shine = tone < 0.35 && up < 0.5 && rng.signed() > 0.75;
        let at = [p[0] + n[0] * size * 0.05, p[1] + n[1] * size * 0.05];
        leaf(&mut rng, at, n, size, tone, shine)
    })?;
    leaves.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(leaves.into_iter().map(|(_, op)| op).collect())
}

fn shapes_change(op: &Op) -> bool {
    matches!(
        op,
        Op::AddLayer { .. }
            | Op::AddNode { .. }
            | Op::Set { .. }
            | Op::Remove { .. }
            | Op::Move { .. }
            | Op::Path { .. }
            | Op::Palette(_)
    )
}

pub fn expand(bundle: &Bundle, ops: Vec<Op>) -> Result<Vec<Op>> {
    let mut doc = bundle.doc.clone();
    let mut out = Vec::new();
    ops.into_iter().enumerate().try_for_each(|(i, op)| {
        let made = match &op {
            Op::Ink(spec) => Some(ink(&doc, spec)),
            Op::Light(spec) => Some(light(&doc, spec)),
            Op::Hatch(spec) => Some(hatch(&doc, spec)),
            Op::Paint(spec) => Some(paint(&doc, spec)),
            Op::Scatter(spec) => Some(scatter(&doc, spec)),
            Op::Foliage(spec) => Some(foliage(&doc, spec)),
            Op::Fill(spec) => spec.shape.as_ref().map(|id| shape_fill(&doc, spec, id)),
            _ => None,
        };
        match made {
            Some(made) => {
                let made = made.with_context(|| format!("op {}", i + 1))?;
                if made.is_empty() {
                    bail!("op {}: nothing to draw", i + 1);
                }
                out.extend(made);
            }
            None => {
                if shapes_change(&op)
                    && let Ok((next, _)) = apply_all(&doc, std::slice::from_ref(&op))
                {
                    doc = next;
                }
                out.push(op);
            }
        }
        Ok::<_, anyhow::Error>(())
    })?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::Layer;
    use klad_core::ops::parse_lines;
    use klad_core::stroke::Mark;

    fn bundle(name: &str) -> (std::path::PathBuf, Bundle) {
        let dir = std::env::temp_dir().join(format!("draw-shapes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(400, 300);
        doc.layers.push(Layer::paint("l1", "flats", "c1"));
        doc.layers.push(Layer::paint("l2", "shade", "c2"));
        doc.layers.push(Layer::paint("l3", "lines", "c3"));
        doc.layers.push(Layer::vector("l4", "sketch", "c4", Vec::new()));
        doc.next_id = 20;
        (dir.clone(), Bundle::create(&dir, doc).unwrap())
    }

    #[test]
    fn foliage_builds_a_clump_from_leaves_lit_as_a_form_with_a_broken_edge() {
        let (dir, b) = bundle("foliage");
        let ops = parse_lines(
            r##"{"op":"add","node":{"id":"clump","type":"ellipse","cx":200,"cy":150,"rx":110,"ry":80},"in":"l4","frame":0}
{"op":"foliage","layer":"l1","in":["clump"],"color":"#4d7a3b","light":[-0.6,-0.8],"leaf":12,"seed":4}"##,
        )
        .unwrap();
        let made = expand(&b, ops.clone()).unwrap();
        assert_eq!(made, expand(&b, ops).unwrap(), "seeded");
        let leaves: Vec<(f32, f32, f32)> = made
            .iter()
            .filter_map(|o| match o {
                Op::Stroke(s) => {
                    let points = s.points.as_ref()?;
                    let (x, y) = (points[0][0], points[0][1]);
                    let lab = color::to_oklab(color::parse_hex(&s.color).ok()?);
                    assert_eq!(s.brush, "leaves");
                    Some((x, y, lab[0]))
                }
                _ => None,
            })
            .collect();
        assert!(leaves.len() > 200, "{} leaves", leaves.len());
        let mean = |pick: &dyn Fn(f32, f32) -> bool| {
            let chosen: Vec<f32> = leaves.iter().filter(|(x, y, _)| pick(*x, *y)).map(|l| l.2).collect();
            chosen.iter().sum::<f32>() / chosen.len().max(1) as f32
        };
        let lit = mean(&|x, y| x < 170.0 && y < 130.0);
        let dark = mean(&|x, y| x > 230.0 && y > 170.0);
        assert!(lit > dark + 0.08, "the lit side is lighter: {lit} against {dark}");
        let outside = made
            .iter()
            .filter_map(|o| match o {
                Op::Stroke(s) => s.points.as_ref().and_then(|p| p.last().copied()),
                _ => None,
            })
            .filter(|p| ((p[0] - 200.0) / 110.0).powi(2) + ((p[1] - 150.0) / 80.0).powi(2) > 1.0)
            .count();
        assert!(outside > 40, "leaves break the outline ({outside})");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn shapes_added_in_a_batch_are_inked_filled_lit_and_textured_in_the_same_batch() {
        let (dir, mut b) = bundle("batch");
        let ops = parse_lines(
            r##"{"op":"add","node":{"id":"body","type":"ellipse","cx":200,"cy":150,"rx":90,"ry":70},"in":"l4","frame":0}
{"op":"add","node":{"id":"eye","type":"ellipse","cx":230,"cy":130,"rx":12,"ry":12},"in":"l4","frame":0}
{"op":"ink","layer":"l3","from":["body","eye"],"weight":5}
{"op":"fill","layer":"l1","shape":"body","color":"#7fa060"}
{"op":"light","from":["body"],"shade":"l2","light":[-1,-1]}
{"op":"hatch","layer":"l2","in":["body"],"spacing":9}
{"op":"scatter","layer":"l1","in":["body"],"avoid":["eye"],"count":12}"##,
        )
        .unwrap();
        let ops = expand(&b, ops).unwrap();
        let strokes = ops.iter().filter(|o| matches!(o, Op::Stroke(_))).count();
        let fills = ops.iter().filter(|o| matches!(o, Op::Fill(_))).count();
        assert!(strokes > 20, "{strokes} strokes");
        assert_eq!(fills, 1 + 3);
        let mut ops = ops;
        crate::agent::resolve_strokes(&b, &mut ops).unwrap();
        crate::fills::resolve(&b, &mut ops).unwrap();
        b.apply(&ops).unwrap();
        let lines = b.read_marks("l3", "c3").unwrap();
        assert!(lines.iter().filter(|m| matches!(m, Mark::Stroke(_))).count() >= 2);
        let shade = b.read_marks("l2", "c2").unwrap();
        assert!(shade.iter().any(|m| m.as_fill().is_some_and(|f| !f.cuts.is_empty())));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paint_takes_palette_names_from_the_same_batch_and_a_shadow_colour_changes_only_the_shadow_side() {
        let (dir, b) = bundle("shadow");
        let painted = |extra: &str| -> Vec<(f32, String)> {
            let ops = parse_lines(&format!(
                r##"{{"op":"palette","add":"#c0603c","name":"clay"}}
{{"op":"palette","add":"#a898d0","name":"lavender"}}
{{"op":"add","node":{{"id":"bowl","type":"ellipse","cx":200,"cy":150,"rx":90,"ry":70}},"in":"l4","frame":0}}
{{"op":"paint","layer":"l1","in":["bowl"],"color":"clay","light":[-1,0]{extra}}}"##
            ))
            .unwrap();
            expand(&b, ops)
                .unwrap()
                .into_iter()
                .filter_map(|o| match o {
                    Op::Stroke(s) => {
                        let points = s.points.unwrap_or_default();
                        let x = points.iter().map(|q| q[0]).sum::<f32>() / points.len().max(1) as f32;
                        Some((x, s.color))
                    }
                    _ => None,
                })
                .collect()
        };
        let plain = painted("");
        let tinted = painted(r#","shadow":"lavender","shadowMix":0.8"#);
        assert_eq!(plain.len(), tinted.len());
        let lit = |strokes: &[(f32, String)]| -> Vec<String> {
            strokes
                .iter()
                .filter(|(x, _)| *x < 150.0)
                .map(|(_, c)| c.clone())
                .collect()
        };
        assert_eq!(lit(&plain), lit(&tinted));
        let blue = |strokes: &[(f32, String)]| {
            let far: Vec<f32> = strokes
                .iter()
                .filter(|(x, _)| *x > 260.0)
                .map(|(_, c)| {
                    let [r, _, b, _] = color::parse_hex(c).unwrap();
                    b - r
                })
                .collect();
            far.iter().sum::<f32>() / far.len().max(1) as f32
        };
        assert!(
            blue(&tinted) > blue(&plain) + 0.05,
            "{} {}",
            blue(&tinted),
            blue(&plain)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_lamp_shades_each_shape_on_the_side_away_from_it() {
        let (dir, b) = bundle("lamp");
        let ops = parse_lines(
            r##"{"op":"add","node":{"id":"near","type":"ellipse","cx":100,"cy":100,"rx":40,"ry":40},"in":"l4","frame":0}
{"op":"add","node":{"id":"far","type":"ellipse","cx":300,"cy":100,"rx":40,"ry":40},"in":"l4","frame":0}
{"op":"light","from":["near","far"],"lamp":[200,100,40],"shade":"l2","depth":20}"##,
        )
        .unwrap();
        let fills: Vec<Vec<[f32; 2]>> = expand(&b, ops)
            .unwrap()
            .into_iter()
            .filter_map(|o| match o {
                Op::Fill(f) => f.polygon,
                _ => None,
            })
            .collect();
        let middle = |p: &Vec<[f32; 2]>| p.iter().map(|q| q[0]).sum::<f32>() / p.len() as f32;
        assert!(fills.iter().any(|p| middle(p) < 100.0 && middle(p) > 40.0));
        assert!(fills.iter().any(|p| middle(p) > 300.0));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ink_and_hatch_strokes_are_drawn_by_hand_unless_hand_is_zero() {
        let (dir, b) = bundle("hand");
        let run = |hand: f32| -> Vec<StrokeSpec> {
            let text = format!(
                r##"{{"op":"add","node":{{"id":"box","type":"rect","x":0,"y":0,"w":120,"h":120}},"in":"l4","frame":0}}
{{"op":"hatch","layer":"l2","in":["box"],"spacing":12,"hand":{hand}}}"##
            );
            expand(&b, parse_lines(&text).unwrap())
                .unwrap()
                .into_iter()
                .filter_map(|o| match o {
                    Op::Stroke(s) => Some(s),
                    _ => None,
                })
                .collect()
        };
        let (handed, even) = (run(1.0), run(0.0));
        assert!(handed.iter().any(|s| s.size != handed[0].size));
        assert!(even.iter().all(|s| s.size == even[0].size));
        assert!(handed.iter().all(|s| s.taper.is_some_and(|t| t[1] > 0.0)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paint_paints_a_form_with_tones_from_the_light_and_strokes_drawn_by_hand() {
        let (dir, b) = bundle("paint");
        let ops = parse_lines(
            r##"{"op":"add","node":{"id":"rock","type":"ellipse","cx":200,"cy":150,"rx":120,"ry":90},"in":"l4","frame":0}
{"op":"paint","layer":"l2","in":["rock"],"color":"#6b7380","weight":10,"light":[-1,-1]}"##,
        )
        .unwrap();
        let ops = expand(&b, ops).unwrap();
        let strokes: Vec<&StrokeSpec> = ops
            .iter()
            .filter_map(|o| match o {
                Op::Stroke(s) => Some(s),
                _ => None,
            })
            .collect();
        let tones: std::collections::BTreeSet<&str> = strokes.iter().map(|s| s.color.as_str()).collect();
        assert!(tones.len() > 30, "{}", tones.len());
        assert!(strokes.iter().all(|s| s.layer == "l2"));
        assert!(strokes.iter().any(|s| s.brush == "oil") && strokes.iter().any(|s| s.brush == "oil-load"));
        let sizes: Vec<f32> = strokes.iter().map(|s| s.size).collect();
        assert!(sizes.iter().cloned().fold(0.0f32, f32::max) > sizes.iter().cloned().fold(f32::MAX, f32::min) * 3.0);
        let lamp = expand(
            &b,
            parse_lines(
                r##"{"op":"add","node":{"id":"rock","type":"rect","x":0,"y":0,"w":90,"h":90},"in":"l4","frame":0}
{"op":"paint","layer":"l2","in":["rock"],"form":"flat","lamp":[-40,40,60]}"##,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(lamp.iter().filter(|o| matches!(o, Op::Stroke(_))).count() > 10);
        let refused = format!(
            "{:#}",
            expand(
                &b,
                parse_lines(
                    r#"{"op":"add","node":{"id":"rock","type":"rect","x":0,"y":0,"w":90,"h":90},"in":"l4","frame":0}
{"op":"paint","layer":"l2","in":["rock"],"form":"blob"}"#
                )
                .unwrap()
            )
            .unwrap_err()
        );
        assert!(refused.contains("round, flat and cylinder"), "{refused}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unexpanded_and_empty_ops_say_what_they_need() {
        let (dir, b) = bundle("errors");
        let refused = |line: &str| format!("{:#}", expand(&b, parse_lines(line).unwrap()).unwrap_err());
        assert!(refused(r#"{"op":"ink","layer":"l3"}"#).contains("op 1"));
        assert!(refused(r#"{"op":"light","from":["nope"],"shade":"l2"}"#).contains("op 1"));
        let err = format!(
            "{:#}",
            apply_all(
                &b.doc,
                &parse_lines(r#"{"op":"ink","layer":"l3","path":"M0 0 L10 10"}"#).unwrap()
            )
            .unwrap_err()
        );
        assert!(err.contains("klad ink"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
