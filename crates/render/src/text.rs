use crate::assets;
use anyhow::{Result, anyhow};
use harfrust::{BufferFlags, ShapeOptions, ShaperData, ShaperInstance, UnicodeBuffer, Variation};
use klad_core::generators::{Align, Text};
use skia_safe::{Path, PathBuilder, Point};
use skrifa::instance::{Location, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::types::Tag;
use skrifa::{FontRef, GlyphId, MetadataProvider};

const WGHT: Tag = Tag::new(b"wght");

struct Outline<'a> {
    path: &'a mut PathBuilder,
    x: f32,
    y: f32,
    scale: f32,
}

impl Outline<'_> {
    fn at(&self, x: f32, y: f32) -> Point {
        Point::new(self.x + x * self.scale, self.y - y * self.scale)
    }
}

impl OutlinePen for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.path.move_to(p);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.path.line_to(p);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (a, b) = (self.at(x1, y1), self.at(x, y));
        self.path.quad_to(a, b);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (a, b, c) = (self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        self.path.cubic_to(a, b, c);
    }

    fn close(&mut self) {
        self.path.close();
    }
}

pub fn parse(bytes: &[u8]) -> Result<FontRef<'_>> {
    FontRef::from_index(bytes, 0).map_err(|e| anyhow!("{e}"))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub min: f32,
    pub default: f32,
    pub max: f32,
}

pub fn weight_axis(bytes: &[u8]) -> Option<Axis> {
    let font = parse(bytes).ok()?;
    font.axes().iter().find(|a| a.tag() == WGHT).map(|a| Axis {
        min: a.min_value(),
        default: a.default_value(),
        max: a.max_value(),
    })
}

fn variations(font: &FontRef, weight: Option<f32>) -> Vec<Variation> {
    let axis = font.axes().iter().find(|a| a.tag() == WGHT);
    match (weight, axis) {
        (Some(w), Some(a)) => vec![Variation {
            tag: WGHT,
            value: w.clamp(a.min_value(), a.max_value()),
        }],
        _ => Vec::new(),
    }
}

struct Glyph {
    id: GlyphId,
    x: f32,
    dx: f32,
    dy: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub start: usize,
    pub baseline: f32,
    pub left: f32,
    pub width: f32,
    pub carets: Vec<(usize, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub lines: Vec<Line>,
    pub ascent: f32,
    pub descent: f32,
}

impl Layout {
    pub fn caret(&self, at: usize) -> Option<(f32, f32, f32)> {
        self.lines
            .iter()
            .rev()
            .find(|l| l.start <= at)
            .and_then(|l| l.carets.iter().find(|(i, _)| *i == at).map(|(_, x)| (*x, l.baseline)))
            .map(|(x, baseline)| (x, baseline - self.ascent, baseline + self.descent))
    }

    pub fn nearest(&self, x: f32, y: f32) -> usize {
        let line = self
            .lines
            .iter()
            .min_by(|a, b| {
                let d = |l: &Line| (l.baseline - (self.ascent - self.descent) / 2.0 - y).abs();
                d(a).total_cmp(&d(b))
            })
            .or(self.lines.first());
        line.and_then(|l| {
            l.carets
                .iter()
                .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
        })
        .map_or(0, |(i, _)| *i)
    }
}

fn shaped(bytes: &[u8], t: &Text, mut each: impl FnMut(&FontRef, &Location, f32, &[Glyph])) -> Result<Layout> {
    let font = parse(bytes).map_err(|e| anyhow!("the font {} cannot be read: {e}", t.font))?;
    let varied = variations(&font, t.weight);
    let location = font.axes().location(varied.iter().map(|v| (v.tag, v.value)));
    let metrics = font.metrics(Size::unscaled(), &location);
    let scale = t.size / metrics.units_per_em as f32;
    let (ascent, descent) = (metrics.ascent * scale, -metrics.descent * scale);
    let data = ShaperData::new(&font);
    let instance = (!varied.is_empty()).then(|| ShaperInstance::from_variations(&font, &varied));
    let shaper = data.shaper(&font).instance(instance.as_ref()).build();
    let mut start = 0;
    let lines = t
        .text
        .split('\n')
        .enumerate()
        .map(|(row, line)| {
            let mut buffer = UnicodeBuffer::new();
            buffer.set_flags(BufferFlags::BEGINNING_OF_TEXT | BufferFlags::END_OF_TEXT);
            buffer.push_str(line);
            buffer.guess_segment_properties();
            let out = shaper.shape(buffer, ShapeOptions::new());
            let count = out.len();
            let mut pen = 0.0;
            let mut clusters: Vec<(usize, f32)> = Vec::new();
            let glyphs: Vec<Glyph> = out
                .glyph_infos()
                .iter()
                .zip(out.glyph_positions())
                .enumerate()
                .map(|(i, (info, pos))| {
                    let cluster = info.cluster as usize;
                    if !clusters.iter().any(|(c, _)| *c == cluster) {
                        clusters.push((cluster, pen));
                    }
                    let glyph = Glyph {
                        id: GlyphId::new(info.glyph_id),
                        x: pen,
                        dx: pos.x_offset as f32 * scale,
                        dy: pos.y_offset as f32 * scale,
                    };
                    pen += pos.x_advance as f32 * scale + if i + 1 < count { t.tracking } else { 0.0 };
                    glyph
                })
                .collect();
            let width = pen;
            let left = match t.align {
                Align::Left => t.x,
                Align::Center => t.x - width / 2.0,
                Align::Right => t.x - width,
            };
            let baseline = t.y + ascent + row as f32 * t.size * t.leading;
            clusters.sort_by_key(|(c, _)| *c);
            clusters.push((line.len(), width));
            let carets = line
                .char_indices()
                .map(|(i, _)| i)
                .chain([line.len()])
                .map(|b| {
                    let k = clusters.partition_point(|(c, _)| *c <= b).saturating_sub(1);
                    let (cs, xs) = clusters[k];
                    let (ce, xe) = clusters.get(k + 1).copied().unwrap_or((line.len(), width));
                    let x = match b == cs || ce <= cs {
                        true => xs,
                        false => {
                            let inside = line[cs..b].chars().count() as f32;
                            let all = line[cs..ce].chars().count().max(1) as f32;
                            xs + (xe - xs) * inside / all
                        }
                    };
                    (start + b, left + x)
                })
                .collect();
            let placed: Vec<Glyph> = glyphs
                .into_iter()
                .map(|g| Glyph {
                    x: left + g.x + g.dx,
                    ..g
                })
                .collect();
            each(&font, &location, baseline, &placed);
            let made = Line {
                start,
                baseline,
                left,
                width,
                carets,
            };
            start += line.len() + 1;
            made
        })
        .collect();
    Ok(Layout { lines, ascent, descent })
}

pub fn layout(t: &Text) -> Result<Layout> {
    layout_in(&assets::font(&t.font)?, t)
}

pub fn layout_in(bytes: &[u8], t: &Text) -> Result<Layout> {
    shaped(bytes, t, |_, _, _, _| {})
}

pub fn text_path(t: &Text) -> Result<Path> {
    path_in(&assets::font(&t.font)?, t)
}

pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub rgba: Vec<u8>,
}

pub fn picture(bytes: &[u8], t: &Text, matrix: [f32; 6], colour: [f32; 4]) -> Result<Option<Picture>> {
    use skia_safe::{AlphaType, ColorType, ImageInfo, Matrix, Paint, RoundOut, surfaces};
    let [a, b, c, d, e, f] = matrix;
    let path = path_in(bytes, t)?.with_transform(&Matrix::new_all(a, c, e, b, d, f, 0.0, 0.0, 1.0));
    let bounds: skia_safe::IRect = path.bounds().round_out();
    let (width, height) = (bounds.width() + 2, bounds.height() + 2);
    if path.is_empty() || width <= 2 || height <= 2 || width > 8192 || height > 8192 {
        return Ok(None);
    }
    let info = ImageInfo::new((width, height), ColorType::RGBA8888, AlphaType::Premul, None);
    let mut surface = surfaces::raster(&info, None, None).ok_or_else(|| anyhow!("cannot hold the text picture"))?;
    let canvas = surface.canvas();
    canvas.translate((1 - bounds.left, 1 - bounds.top));
    let mut paint = Paint::new(
        skia_safe::Color4f::new(colour[0], colour[1], colour[2], colour[3]),
        None,
    );
    paint.set_anti_alias(true);
    canvas.draw_path(&path, &paint);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    if !surface.read_pixels(&info, &mut rgba, width as usize * 4, (0, 0)) {
        return Err(anyhow!("cannot read the text picture"));
    }
    Ok(Some(Picture {
        width: width as u32,
        height: height as u32,
        left: bounds.left - 1,
        top: bounds.top - 1,
        rgba,
    }))
}

pub fn path_in(bytes: &[u8], t: &Text) -> Result<Path> {
    let mut path = PathBuilder::new();
    shaped(bytes, t, |font, location, baseline, glyphs| {
        let scale = t.size / font.metrics(Size::unscaled(), location).units_per_em as f32;
        let outlines = font.outline_glyphs();
        glyphs.iter().for_each(|g| {
            let mut one = PathBuilder::new();
            let mut outline = Outline {
                path: &mut one,
                x: g.x,
                y: baseline - g.dy,
                scale,
            };
            if let Some(glyph) = outlines.get(g.id) {
                let _ = glyph.draw(DrawSettings::unhinted(Size::unscaled(), location), &mut outline);
            }
            let drawn = one.detach();
            let merged = drawn.simplify().and_then(|p| p.as_winding()).unwrap_or(drawn);
            path.add_path(&merged, None);
        });
    })?;
    Ok(path.detach())
}
