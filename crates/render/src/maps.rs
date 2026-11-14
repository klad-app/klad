use crate::export::{self, Options, Target, f16_pixels};
use crate::{Frame, Rasters};
use anyhow::{Result, bail};
use klad_brush::TILE;
use klad_brush::tiles::Heights;
use klad_core::color::parse_hex;
use klad_core::doc::{Blend, Document, Kind, Layer, LayerContent};
use klad_core::maps::{Light, LightKind, Maps, Role, Surface};
use rayon::prelude::*;
use skia_safe::AlphaType;

const BODY_PX: f32 = 3.0;
const FAR: f32 = 1e20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Map {
    Normal,
    Height,
    Specular,
    Emission,
}

impl Map {
    pub const ALL: [Map; 4] = [Map::Normal, Map::Height, Map::Specular, Map::Emission];

    pub fn name(self) -> &'static str {
        match self {
            Map::Normal => "normal",
            Map::Height => "height",
            Map::Specular => "specular",
            Map::Emission => "emission",
        }
    }

    pub fn parse(text: &str) -> Result<Map> {
        Map::ALL
            .into_iter()
            .find(|m| m.name() == text)
            .ok_or_else(|| anyhow::anyhow!("no {text} map: the maps are normal, height, specular and emission"))
    }
}

pub struct MapSet {
    pub width: usize,
    pub height: usize,
    pub cover: Vec<f32>,
    pub normal: Vec<[f32; 3]>,
    pub rise: Vec<f32>,
    pub specular: Vec<f32>,
    pub emission: Vec<[f32; 4]>,
}

impl MapSet {
    pub fn frame(&self, map: Map) -> Frame {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let top = self.rise.iter().copied().fold(0.0f32, f32::max).max(1e-6);
        let rgba = (0..self.width * self.height)
            .into_par_iter()
            .flat_map_iter(|i| {
                let a = byte(self.cover[i]);
                match map {
                    Map::Normal => {
                        let [x, y, z] = self.normal[i];
                        [byte(x * 0.5 + 0.5), byte(y * 0.5 + 0.5), byte(z * 0.5 + 0.5), a]
                    }
                    Map::Height => {
                        let v = byte(self.rise[i] / top);
                        [v, v, v, a]
                    }
                    Map::Specular => {
                        let v = byte(self.specular[i]);
                        [v, v, v, a]
                    }
                    Map::Emission => {
                        let [r, g, b, e] = self.emission[i];
                        match e > 0.0 {
                            true => [byte(r / e), byte(g / e), byte(b / e), byte(e)],
                            false => [0, 0, 0, 0],
                        }
                    }
                }
            })
            .collect();
        Frame {
            width: self.width as u32,
            height: self.height as u32,
            rgba,
        }
    }
}

struct Owner<'a> {
    surface: Surface,
    layers: Vec<&'a Layer>,
    alpha: Vec<f32>,
    weight: Vec<f32>,
}

fn is_surface(layer: &Layer) -> bool {
    layer.visible
        && !layer.reference
        && !layer.clip
        && layer.map.is_none()
        && layer.blend == Blend::Normal
        && !matches!(layer.kind(), Kind::Adjust | Kind::Bones)
}

fn options(doc: &Document, scale: f32) -> Options {
    Options {
        scale,
        background: false,
        references: false,
        detail: export::detail_of(doc, scale),
    }
}

fn alone(doc: &Document, frame: u32, rasters: &Rasters, layer: &Layer, scale: f32) -> Result<Vec<[f32; 4]>> {
    let mut shown = layer.clone();
    shown.map = None;
    shown.visible = true;
    let image = export::render(doc, frame, rasters, &Target::Layer(&shown), options(doc, scale))?;
    Ok(f16_pixels(&image, AlphaType::Premul)?
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p.map(|v| v.to_f32()))
        .collect())
}

fn painted<'a>(layer: &'a Layer, frame: u32, out: &mut Vec<&'a str>) {
    if !layer.visible || layer.reference || layer.map.is_some() {
        return;
    }
    match &layer.content {
        LayerContent::Paint { .. } => out.extend(layer.content_at(frame).map(|c| c.id.as_str())),
        LayerContent::Group { children } => children.iter().for_each(|c| painted(c, frame, out)),
        _ => {}
    }
}

fn height_at(heights: &Heights, x: i32, y: i32) -> f32 {
    if x < 0 || y < 0 || x as u32 >= heights.width || y as u32 >= heights.height {
        return 0.0;
    }
    let (x, y) = (x as usize, y as usize);
    heights
        .get(x / TILE, y / TILE)
        .map_or(0.0, |t| t[(y % TILE) * TILE + x % TILE].to_f32())
}

fn sampled(heights: &Heights, cx: f32, cy: f32) -> f32 {
    let (fx, fy) = (cx - 0.5, cy - 0.5);
    let (x, y) = (fx.floor(), fy.floor());
    let (ax, ay) = (fx - x, fy - y);
    let (x, y) = (x as i32, y as i32);
    let top = height_at(heights, x, y) * (1.0 - ax) + height_at(heights, x + 1, y) * ax;
    let bottom = height_at(heights, x, y + 1) * (1.0 - ax) + height_at(heights, x + 1, y + 1) * ax;
    top * (1.0 - ay) + bottom * ay
}

fn distance_1d(f: &[f32], out: &mut [f32]) {
    let n = f.len();
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f32; n + 1];
    let mut k = 0usize;
    z[0] = -FAR;
    z[1] = FAR;
    (1..n).for_each(|q| {
        let meet = |k: usize| {
            let p = v[k];
            ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32)
        };
        let mut s = meet(k);
        while s <= z[k] && k > 0 {
            k -= 1;
            s = meet(k);
        }
        if s <= z[k] {
            v[0] = q;
            z[0] = -FAR;
            z[1] = FAR;
            k = 0;
            return;
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = FAR;
    });
    let mut k = 0usize;
    (0..n).for_each(|q| {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        let d = q as f32 - p as f32;
        out[q] = (d * d + f[p]).min(FAR);
    });
}

fn transpose(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    (0..w * h).map(|i| src[(i % h) * w + i / h]).collect()
}

fn rows(grid: &mut [f32], w: usize) {
    grid.par_chunks_mut(w).for_each(|row| {
        let f = row.to_vec();
        distance_1d(&f, row);
    });
}

pub fn edge_distance(alpha: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut grid: Vec<f32> = alpha.iter().map(|a| if *a < 0.5 { 0.0 } else { FAR }).collect();
    if grid.iter().all(|v| *v >= FAR) {
        return vec![FAR; w * h];
    }
    rows(&mut grid, w);
    let mut turned = transpose(&grid, w, h);
    rows(&mut turned, h);
    transpose(&turned, h, w).iter().map(|d| libm::sqrtf(*d)).collect()
}

fn soften(grid: &[f32], w: usize, h: usize) -> Vec<f32> {
    let across: Vec<f32> = (0..w * h)
        .into_par_iter()
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let at = |x: usize| grid[y * w + x];
            (at(x.saturating_sub(1)) + 2.0 * at(x) + at((x + 1).min(w - 1))) * 0.25
        })
        .collect();
    (0..w * h)
        .into_par_iter()
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let at = |y: usize| across[y * w + x];
            (at(y.saturating_sub(1)) + 2.0 * at(y) + at((y + 1).min(h - 1))) * 0.25
        })
        .collect()
}

fn box_line(line: &[f32], reach: usize) -> Vec<f32> {
    let n = line.len();
    let sums: Vec<f32> = std::iter::once(0.0)
        .chain(line.iter().scan(0.0f32, |acc, v| {
            *acc += v;
            Some(*acc)
        }))
        .collect();
    (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(reach), (i + reach + 1).min(n));
            (sums[b] - sums[a]) / (b - a) as f32
        })
        .collect()
}

fn smooth(grid: &[f32], w: usize, h: usize, reach: usize) -> Vec<f32> {
    let across: Vec<f32> = grid.par_chunks(w).flat_map_iter(|row| box_line(row, reach)).collect();
    let turned = transpose(&across, w, h);
    let down: Vec<f32> = turned.par_chunks(h).flat_map_iter(|col| box_line(col, reach)).collect();
    transpose(&down, h, w)
}

fn normals(rise: &[f32], w: usize, h: usize, strength: f32) -> Vec<[f32; 3]> {
    (0..w * h)
        .into_par_iter()
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let at = |x: usize, y: usize| rise[y * w + x];
            let dx = (at((x + 1).min(w - 1), y) - at(x.saturating_sub(1), y)) * 0.5;
            let dy = (at(x, (y + 1).min(h - 1)) - at(x, y.saturating_sub(1))) * 0.5;
            unit([-strength * dx, strength * dy, 1.0])
        })
        .collect()
}

fn unit(v: [f32; 3]) -> [f32; 3] {
    let n = libm::sqrtf(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
    match n > 1e-9 {
        true => v.map(|c| c / n),
        false => [0.0, 0.0, 1.0],
    }
}

fn luminance(p: [f32; 4]) -> f32 {
    match p[3] > 0.0 {
        true => (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]) / p[3],
        false => 0.0,
    }
}

fn top_of<'a>(doc: &'a Document, id: &str) -> Option<&'a Layer> {
    doc.layers.iter().find(|top| {
        let mut all = vec![*top];
        klad_core::doc::walk_children(top, &mut all);
        all.iter().any(|l| l.id == id)
    })
}

fn owner_of(doc: &Document, layer: &Layer) -> Option<String> {
    let top = top_of(doc, &layer.id)?;
    let unit = match top.id == layer.id && layer.clip {
        true => {
            let at = doc.layers.iter().position(|l| l.id == layer.id)?;
            doc.layers[..at].iter().rev().find(|l| !l.clip)?
        }
        false => top,
    };
    unit.surface.as_ref().map(|_| unit.id.clone())
}

fn map_layers(doc: &Document, role: Role) -> Vec<&Layer> {
    let mut all = Vec::new();
    doc.layers.iter().for_each(|l| {
        all.push(l);
        klad_core::doc::walk_children(l, &mut all);
    });
    all.into_iter()
        .filter(|l| l.map == Some(role) && l.visible && !l.reference)
        .collect()
}

pub fn settings(doc: &Document) -> Maps {
    doc.maps.clone().unwrap_or_default()
}

pub fn size(doc: &Document, scale: f32) -> (usize, usize) {
    (
        ((doc.canvas.width as f32 * scale).round() as usize).max(1),
        ((doc.canvas.height as f32 * scale).round() as usize).max(1),
    )
}

pub fn render(doc: &Document, frame: u32, rasters: &Rasters, scale: f32) -> Result<MapSet> {
    export::check_scale(doc, scale)?;
    let settings = settings(doc);
    let (w, h) = size(doc, scale);
    let n = w * h;
    let units: Vec<&Layer> = doc.layers.iter().filter(|l| is_surface(l)).collect();
    let mut root = Owner {
        surface: settings.surface.clone(),
        layers: Vec::new(),
        alpha: vec![0.0; n],
        weight: vec![0.0; n],
    };
    let mut owned: Vec<(String, Owner)> = Vec::new();
    let mut remaining = vec![1.0f32; n];
    units.iter().rev().try_for_each(|layer| {
        let px = alone(doc, frame, rasters, layer, scale)?;
        let owner = match &layer.surface {
            Some(surface) => {
                owned.push((
                    layer.id.clone(),
                    Owner {
                        surface: surface.clone(),
                        layers: Vec::new(),
                        alpha: vec![0.0; n],
                        weight: vec![0.0; n],
                    },
                ));
                &mut owned.last_mut().expect("just pushed").1
            }
            None => &mut root,
        };
        owner.layers.push(layer);
        owner
            .alpha
            .par_iter_mut()
            .zip(owner.weight.par_iter_mut())
            .zip(remaining.par_iter_mut())
            .zip(px.par_iter())
            .for_each(|(((union, weight), left), p)| {
                let a = p[3].clamp(0.0, 1.0);
                *union = 1.0 - (1.0 - *union) * (1.0 - a);
                *weight += a * *left;
                *left *= 1.0 - a;
            });
        Ok::<_, anyhow::Error>(())
    })?;
    let lifts: Vec<(Option<String>, Vec<[f32; 4]>)> = map_layers(doc, Role::Height)
        .into_iter()
        .map(|l| Ok((owner_of(doc, l), alone(doc, frame, rasters, l, scale)?)))
        .collect::<Result<_>>()?;
    let owners: Vec<(Option<String>, Owner)> = std::iter::once((None, root))
        .chain(owned.into_iter().map(|(id, o)| (Some(id), o)))
        .collect();
    let detail = export::detail_of(doc, scale);
    let rises: Vec<Vec<f32>> = owners
        .iter()
        .map(|(id, owner)| {
            let s = &owner.surface;
            let bevel = s.bevel * scale;
            let rise = match bevel > 0.0 && s.profile != klad_core::maps::Profile::Flat {
                true => {
                    let near: Vec<f32> = edge_distance(&owner.alpha, w, h)
                        .iter()
                        .map(|d| d.min(bevel * 1.5))
                        .collect();
                    let reach = ((bevel * 0.08).round() as usize).clamp(1, 12);
                    smooth(&smooth(&near, w, h, reach), w, h, reach)
                        .par_iter()
                        .map(|d| s.profile.rise(d / bevel) * bevel)
                        .collect()
                }
                false => vec![0.0; n],
            };
            let mut cels = Vec::new();
            owner.layers.iter().for_each(|l| painted(l, frame, &mut cels));
            let maps: Vec<&Heights> = cels
                .iter()
                .filter_map(|c| rasters.get(*c).and_then(|r| r.heights()))
                .collect();
            let body = s.body * BODY_PX * scale;
            let lift = s.range * scale;
            let mine: Vec<&Vec<[f32; 4]>> = lifts.iter().filter(|(o, _)| o == id).map(|(_, px)| px).collect();
            let raw: Vec<f32> = rise
                .into_par_iter()
                .enumerate()
                .map(|(i, r)| {
                    let (x, y) = (i % w, i / w);
                    let (cx, cy) = ((x as f32 + 0.5) / scale * detail, (y as f32 + 0.5) / scale * detail);
                    let paint: f32 = maps.iter().map(|m| sampled(m, cx, cy)).sum();
                    let eased = 1.6 * libm::tanhf(paint / 1.6);
                    let raised: f32 = mine.iter().map(|px| px[i][3] * (2.0 * luminance(px[i]) - 1.0)).sum();
                    (r + eased * body + raised * lift) * owner.alpha[i].clamp(0.0, 1.0).sqrt()
                })
                .collect();
            soften(&soften(&raw, w, h), w, h)
        })
        .collect();
    let mut normal = vec![[0.0f32; 3]; n];
    let mut rise = vec![0.0f32; n];
    owners.iter().zip(&rises).for_each(|((_, owner), r)| {
        let lit = normals(r, w, h, owner.surface.strength);
        normal
            .par_iter_mut()
            .zip(rise.par_iter_mut())
            .enumerate()
            .for_each(|(i, (nm, ri))| {
                let k = owner.weight[i];
                nm.iter_mut().zip(lit[i]).for_each(|(a, b)| *a += k * b);
                *ri += k * r[i];
            });
    });
    let cover: Vec<f32> = remaining.iter().map(|left| 1.0 - left).collect();
    normal.par_iter_mut().zip(&remaining).for_each(|(nm, left)| {
        nm[2] += left;
        *nm = unit(*nm);
    });
    let over = |role: Role| -> Result<Vec<Vec<[f32; 4]>>> {
        map_layers(doc, role)
            .into_iter()
            .map(|l| alone(doc, frame, rasters, l, scale))
            .collect()
    };
    over(Role::Normal)?.iter().for_each(|px| {
        normal.par_iter_mut().zip(px).for_each(|(nm, p)| {
            if p[3] <= 0.0 {
                return;
            }
            let painted = [0, 1, 2].map(|c| (p[c] / p[3]) * 2.0 - 1.0);
            *nm = unit([0, 1, 2].map(|c| nm[c] * (1.0 - p[3]) + painted[c] * p[3]));
        })
    });
    let mut specular = vec![0.0f32; n];
    over(Role::Specular)?.iter().for_each(|px| {
        specular
            .par_iter_mut()
            .zip(px)
            .for_each(|(s, p)| *s = *s * (1.0 - p[3]) + luminance(*p) * p[3]);
    });
    let mut emission = vec![[0.0f32; 4]; n];
    over(Role::Emission)?.iter().for_each(|px| {
        emission
            .par_iter_mut()
            .zip(px)
            .for_each(|(e, p)| *e = [0, 1, 2, 3].map(|c| p[c] + e[c] * (1.0 - p[3])));
    });
    Ok(MapSet {
        width: w,
        height: h,
        cover,
        normal,
        rise,
        specular,
        emission,
    })
}

struct Lamp {
    kind: LightKind,
    at: [f32; 2],
    height: f32,
    range: f32,
    rgb: [f32; 3],
    toward: [f32; 3],
}

fn lamps(lights: &[Light], scale: f32) -> Result<Vec<Lamp>> {
    lights
        .iter()
        .map(|l| {
            let [r, g, b, _] = parse_hex(&l.color)?;
            let toward = match l.kind {
                LightKind::Directional => {
                    let len = libm::hypotf(l.at[0], l.at[1]).max(1e-6);
                    let angle = l.height * std::f32::consts::FRAC_PI_2;
                    let (up, along) = (libm::sinf(angle), libm::cosf(angle));
                    unit([l.at[0] / len * along, -l.at[1] / len * along, up])
                }
                LightKind::Point => [0.0, 0.0, 1.0],
            };
            Ok(Lamp {
                kind: l.kind,
                at: [l.at[0] * scale, l.at[1] * scale],
                height: l.height * scale,
                range: l.range * scale,
                rgb: [r, g, b].map(|c| c * l.energy),
                toward,
            })
        })
        .collect()
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub struct Lit {
    set: MapSet,
    colour: Vec<[f32; 4]>,
    ambient: [f32; 3],
    shininess: f32,
    scale: f32,
}

impl Lit {
    pub fn prepare(doc: &Document, frame: u32, rasters: &Rasters, scale: f32) -> Result<Lit> {
        let settings = settings(doc);
        let set = render(doc, frame, rasters, scale)?;
        let image = export::render(
            doc,
            frame,
            rasters,
            &Target::Frame,
            Options {
                background: true,
                ..options(doc, scale)
            },
        )?;
        let colour = f16_pixels(&image, AlphaType::Premul)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p.map(|v| v.to_f32()))
            .collect();
        let [r, g, b, _] = parse_hex(&settings.ambient)?;
        Ok(Lit {
            set,
            colour,
            ambient: [r, g, b],
            shininess: settings.shininess,
            scale,
        })
    }

    pub fn size(&self) -> (usize, usize) {
        (self.set.width, self.set.height)
    }

    fn texel(&self, i: usize, lamps: &[Lamp]) -> [f32; 4] {
        let p = self.colour[i];
        let a = p[3];
        let base = match a > 0.0 {
            true => [p[0] / a, p[1] / a, p[2] / a],
            false => [0.0; 3],
        };
        let normal = self.set.normal[i];
        let specular = self.set.specular[i];
        let at = [(i % self.set.width) as f32 + 0.5, (i / self.set.width) as f32 + 0.5];
        let (light, shine) = lamps.iter().fold((self.ambient, [0.0f32; 3]), |(sum, shine), lamp| {
            let (dir, fade) = match lamp.kind {
                LightKind::Point => {
                    let (dx, dy) = (lamp.at[0] - at[0], lamp.at[1] - at[1]);
                    let reach = (1.0 - libm::hypotf(dx, dy) / lamp.range).clamp(0.0, 1.0);
                    (unit([dx, -dy, lamp.height]), reach * reach)
                }
                LightKind::Directional => (lamp.toward, 1.0),
            };
            let facing = dot(normal, dir).max(0.0);
            let gloss = match specular > 0.0 && facing > 0.0 {
                true => {
                    specular
                        * libm::powf(
                            dot(normal, unit([dir[0], dir[1], dir[2] + 1.0])).max(0.0),
                            self.shininess,
                        )
                }
                false => 0.0,
            };
            (
                [0, 1, 2].map(|c| sum[c] + lamp.rgb[c] * fade * facing),
                [0, 1, 2].map(|c| shine[c] + lamp.rgb[c] * fade * gloss),
            )
        });
        let e = self.set.emission[i];
        let lit = [0, 1, 2].map(|c| (base[c] * light[c] + shine[c]).clamp(0.0, 1.0) * a);
        let alpha = e[3] + a * (1.0 - e[3]);
        match alpha > 0.0 {
            true => {
                let [r, g, b] = [0, 1, 2].map(|c| ((e[c] + lit[c] * (1.0 - e[3])) / alpha).clamp(0.0, 1.0));
                [r, g, b, alpha.clamp(0.0, 1.0)]
            }
            false => [0.0; 4],
        }
    }

    pub fn shade(&self, lights: &[Light]) -> Result<Frame> {
        let lamps = lamps(lights, self.scale)?;
        let byte = |v: f32| (v * 255.0).round() as u8;
        let rgba = (0..self.colour.len())
            .into_par_iter()
            .flat_map_iter(|i| self.texel(i, &lamps).map(byte))
            .collect();
        Ok(Frame {
            width: self.set.width as u32,
            height: self.set.height as u32,
            rgba,
        })
    }
}

pub fn lit(doc: &Document, frame: u32, rasters: &Rasters, scale: f32) -> Result<Frame> {
    let lights = settings(doc).lights;
    if lights.is_empty() {
        bail!("no lights to light it with: klad lights <bundle> --add point --at x,y");
    }
    Lit::prepare(doc, frame, rasters, scale)?.shade(&lights)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{View, compose, rasterize_nodes_at};
    use klad_core::doc::{Node, Shape};
    use klad_core::maps::{Light, Profile};

    fn disc(id: &str, cx: f32, cy: f32, r: f32, color: &str) -> Layer {
        Layer::vector(
            id,
            id,
            &format!("c{id}"),
            vec![Node::new(&format!("n{id}"), Shape::Ellipse { cx, cy, rx: r, ry: r }).filled(color)],
        )
    }

    fn rasters(doc: &Document) -> Rasters {
        rasters_at(doc, 1.0)
    }

    fn rasters_at(doc: &Document, detail: f32) -> Rasters {
        doc.layers
            .iter()
            .filter(|l| l.kind() == Kind::Vector)
            .filter_map(|l| l.cels().first())
            .map(|c| {
                let raster = rasterize_nodes_at(&c.nodes, doc.canvas.width, doc.canvas.height, detail).unwrap();
                (c.id.clone(), raster)
            })
            .collect()
    }

    fn doc_of(layers: Vec<Layer>) -> Document {
        let mut doc = Document::new(128, 128);
        doc.layers = layers;
        doc
    }

    fn normal_at(doc: &Document, rasters: &Rasters, x: usize, y: usize) -> [f32; 3] {
        let set = render(doc, 0, rasters, 1.0).unwrap();
        set.normal[y * set.width + x]
    }

    #[test]
    fn a_disc_faces_out_at_its_rim_and_forward_in_the_middle() {
        let doc = doc_of(vec![disc("a", 64.0, 64.0, 40.0, "#c08040")]);
        let r = rasters(&doc);
        let set = render(&doc, 0, &r, 1.0).unwrap();
        let at = |x: usize, y: usize| set.normal[y * set.width + x];
        assert!(at(64, 64)[2] > 0.99, "the middle faces the viewer: {:?}", at(64, 64));
        assert!(at(27, 64)[0] < -0.3, "the left rim faces left: {:?}", at(27, 64));
        assert!(at(101, 64)[0] > 0.3, "the right rim faces right: {:?}", at(101, 64));
        assert!(at(64, 27)[1] > 0.3, "the top rim faces up, green up: {:?}", at(64, 27));
        assert!(at(64, 101)[1] < -0.3, "the bottom rim faces down: {:?}", at(64, 101));
        let frame = set.frame(Map::Normal);
        assert_eq!(frame.pixel(2, 2), [128, 128, 255, 0], "outside: flat and clear");
        assert!(frame.pixel(27, 64)[0] < 100 && frame.pixel(101, 64)[0] > 155);
        assert_eq!(frame.pixel(64, 64)[3], 255);
    }

    #[test]
    fn the_bevel_sets_how_much_of_the_form_rounds() {
        let mut doc = doc_of(vec![disc("a", 64.0, 64.0, 40.0, "#c08040")]);
        let r = rasters(&doc);
        let mut wide = Maps::default();
        wide.surface.bevel = 30.0;
        doc.maps = Some(wide);
        assert!(
            normal_at(&doc, &r, 39, 64)[0] < -0.2,
            "a wide bevel still turns 15 px in"
        );
        let mut narrow = Maps::default();
        narrow.surface.bevel = 4.0;
        doc.maps = Some(narrow);
        assert!(
            normal_at(&doc, &r, 39, 64)[0].abs() < 0.01,
            "a narrow one is flat there"
        );
        let mut flat = Maps::default();
        flat.surface.profile = Profile::Flat;
        doc.maps = Some(flat);
        assert!(normal_at(&doc, &r, 27, 64)[2] > 0.999, "a flat profile has no rim");
    }

    #[test]
    fn a_height_layer_lifts_its_surface_and_never_shows_in_the_colour() {
        let base = doc_of(vec![disc("a", 64.0, 64.0, 40.0, "#c08040")]);
        let mut bump = disc("h", 64.0, 64.0, 10.0, "#ffffff");
        bump.map = Some(Role::Height);
        let doc = doc_of(vec![base.layers[0].clone(), bump]);
        doc.validate().unwrap();
        let r = rasters(&doc);
        assert_eq!(
            compose(&doc, 0, &r, View::scaled(1.0)).unwrap().hash(),
            compose(&base, 0, &rasters(&base), View::scaled(1.0)).unwrap().hash(),
            "a map layer is not drawn in the colour"
        );
        let set = render(&doc, 0, &r, 1.0).unwrap();
        let flat = render(&base, 0, &rasters(&base), 1.0).unwrap();
        let i = 64 * 128 + 64;
        assert!(
            set.rise[i] > flat.rise[i] + 5.0,
            "white raises: {} vs {}",
            set.rise[i],
            flat.rise[i]
        );
        assert!(set.normal[64 * 128 + 54][0] < -0.3, "the bump's left side faces left");
        assert!(flat.normal[64 * 128 + 54][2] > 0.999);
    }

    #[test]
    fn a_top_layer_with_its_own_surface_gets_its_own_rim() {
        let small = disc("b", 64.0, 64.0, 16.0, "#80a0c0");
        let together = doc_of(vec![disc("a", 64.0, 64.0, 50.0, "#c08040"), small.clone()]);
        let r = rasters(&together);
        assert!(
            normal_at(&together, &r, 50, 64)[2] > 0.999,
            "inside one surface the small disc is part of the plateau"
        );
        let mut own = small;
        own.surface = Some(Surface::default());
        let apart = doc_of(vec![together.layers[0].clone(), own]);
        apart.validate().unwrap();
        assert!(
            normal_at(&apart, &r, 50, 64)[0] < -0.3,
            "its own surface has its own rim"
        );
        let mut hidden = apart.clone();
        hidden.layers.swap(0, 1);
        assert!(
            normal_at(&hidden, &r, 50, 64)[2] > 0.999,
            "covered by the big disc, the small one's rim is not seen"
        );
    }

    #[test]
    fn paint_body_raises_ridges_and_its_share_is_a_setting() {
        let mut doc = Document::new(128, 128);
        doc.layers = vec![Layer::paint("p", "p", "cp")];
        let mut raster = klad_brush::Raster::new(128, 128);
        raster.ensure(0).chunks_mut(4).for_each(|px| {
            px.copy_from_slice(&[0.5, 0.5, 0.5, 1.0].map(half::f16::from_f32));
        });
        raster
            .heights_mut()
            .ensure(0)
            .iter_mut()
            .enumerate()
            .for_each(|(i, h)| {
                if (60..68).contains(&(i % TILE)) {
                    *h = half::f16::from_f32(1.0);
                }
            });
        let r: Rasters = [("cp".to_string(), raster)].into_iter().collect();
        let mut maps = Maps::default();
        maps.surface.bevel = 0.0;
        doc.maps = Some(maps.clone());
        assert!(
            normal_at(&doc, &r, 59, 64)[0] < -0.2,
            "the ridge's left side faces left"
        );
        maps.surface.body = 0.0;
        doc.maps = Some(maps);
        assert!(normal_at(&doc, &r, 59, 64)[2] > 0.999, "without body the paint is flat");
    }

    #[test]
    fn normal_specular_and_emission_layers_paint_their_maps() {
        let mut facing = disc("n", 64.0, 64.0, 12.0, "#ff8080");
        facing.map = Some(Role::Normal);
        let mut shiny = disc("s", 30.0, 30.0, 8.0, "#ffffff");
        shiny.map = Some(Role::Specular);
        let mut glow = disc("e", 100.0, 100.0, 8.0, "#ffcc00");
        glow.map = Some(Role::Emission);
        let doc = doc_of(vec![disc("a", 64.0, 64.0, 60.0, "#406080"), facing, shiny, glow]);
        doc.validate().unwrap();
        let set = render(&doc, 0, &rasters(&doc), 1.0).unwrap();
        let n = set.normal[64 * 128 + 64];
        assert!(n[0] > 0.99, "#ff8080 is a normal facing right: {n:?}");
        assert!(set.specular[30 * 128 + 30] > 0.99 && set.specular[64 * 128 + 64] == 0.0);
        assert_eq!(set.frame(Map::Emission).pixel(100, 100), [255, 204, 0, 255]);
        assert_eq!(set.frame(Map::Emission).pixel(64, 64)[3], 0);
    }

    #[test]
    fn the_side_facing_a_light_is_lit() {
        let mut doc = doc_of(vec![disc("a", 64.0, 64.0, 40.0, "#ffffff")]);
        let r = rasters(&doc);
        assert!(lit(&doc, 0, &r, 1.0).is_err_and(|e| e.to_string().contains("no lights")));
        let mut maps = Maps {
            lights: vec![Light {
                at: [0.0, 64.0],
                height: 30.0,
                ..Light::default()
            }],
            ..Maps::default()
        };
        doc.maps = Some(maps.clone());
        let frame = lit(&doc, 0, &r, 1.0).unwrap();
        let (left, right) = (frame.pixel(27, 64)[0], frame.pixel(101, 64)[0]);
        assert!(
            left > right + 40,
            "a lamp on the left lights the left rim: {left} vs {right}"
        );
        maps.lights = vec![Light::directional([1.0, 0.0], 0.3)];
        doc.maps = Some(maps);
        let frame = lit(&doc, 0, &r, 1.0).unwrap();
        assert!(
            frame.pixel(101, 64)[0] > frame.pixel(27, 64)[0] + 40,
            "light from the right"
        );
        let ambient = parse_hex(&Maps::default().ambient).unwrap();
        assert!(
            (frame.pixel(27, 64)[0] as f32 / 255.0 - ambient[0]).abs() < 0.02,
            "the side turned away gets the ambient only"
        );
    }

    #[test]
    fn maps_scale_with_the_export() {
        let doc = doc_of(vec![disc("a", 64.0, 64.0, 40.0, "#c08040")]);
        let r = rasters_at(&doc, 2.0);
        let set = render(&doc, 0, &r, 2.0).unwrap();
        assert_eq!((set.width, set.height), (256, 256));
        assert!(set.normal[128 * 256 + 54][0] < -0.3, "the left rim at twice the size");
        assert!(set.normal[128 * 256 + 128][2] > 0.99);
    }

    #[test]
    fn the_edge_distance_is_euclidean() {
        let (w, h) = (9, 9);
        let alpha: Vec<f32> = (0..w * h)
            .map(|i| {
                if (1..8).contains(&(i % w)) && (1..8).contains(&(i / w)) {
                    1.0
                } else {
                    0.0
                }
            })
            .collect();
        let d = edge_distance(&alpha, w, h);
        assert_eq!(d[0], 0.0);
        assert_eq!(d[4 * w + 4], 4.0, "the middle is 4 px from the frame of empty pixels");
        assert_eq!(d[w + 1], 1.0);
        assert!(
            edge_distance(&[1.0; 16], 4, 4).iter().all(|v| *v >= FAR),
            "nothing outside: no rim"
        );
    }
}
