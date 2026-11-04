use crate::assets;
use anyhow::{Result, anyhow};
use klad_core::generators::{Align, Text};
use skia_safe::{Path, PathBuilder, Point};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::TableProvider;
use skrifa::raw::tables::kern::SubtableKind;
use skrifa::{FontRef, GlyphId, MetadataProvider};

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

fn kerning(font: &FontRef, left: GlyphId, right: GlyphId) -> f32 {
    font.kern()
        .ok()
        .and_then(|table| {
            table
                .subtables()
                .filter_map(|s| s.ok())
                .filter(|s| s.is_horizontal() && !s.is_variable())
                .find_map(|s| match s.kind().ok()? {
                    SubtableKind::Format0(t) => t.kerning(left, right),
                    SubtableKind::Format2(t) => t.kerning(left, right),
                    SubtableKind::Format3(t) => t.kerning(left, right),
                    SubtableKind::Format1(_) => None,
                })
        })
        .unwrap_or(0) as f32
}

pub fn text_path(t: &Text) -> Result<Path> {
    let bytes = assets::font(&t.font)?;
    let font = parse(&bytes).map_err(|e| anyhow!("the font {} cannot be read: {e}", t.font))?;
    let metrics = font.metrics(Size::unscaled(), LocationRef::default());
    let advances = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let charmap = font.charmap();
    let outlines = font.outline_glyphs();
    let scale = t.size / metrics.units_per_em as f32;
    let ascent = metrics.ascent * scale;
    let mut path = PathBuilder::new();
    t.text.split('\n').enumerate().for_each(|(row, line)| {
        let glyphs: Vec<GlyphId> = line
            .chars()
            .map(|c| charmap.map(c).unwrap_or(GlyphId::NOTDEF))
            .collect();
        let steps: Vec<f32> = glyphs
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let advance = advances.advance_width(*g).unwrap_or(0.0);
                let kern = glyphs.get(i + 1).map_or(0.0, |next| kerning(&font, *g, *next));
                (advance + kern) * scale + if i + 1 < glyphs.len() { t.tracking } else { 0.0 }
            })
            .collect();
        let width: f32 = steps.iter().sum();
        let left = match t.align {
            Align::Left => t.x,
            Align::Center => t.x - width / 2.0,
            Align::Right => t.x - width,
        };
        let baseline = t.y + ascent + row as f32 * t.size * t.leading;
        glyphs.iter().zip(&steps).fold(left, |pen, (g, step)| {
            let mut outline = Outline {
                path: &mut path,
                x: pen,
                y: baseline,
                scale,
            };
            if let Some(glyph) = outlines.get(*g) {
                let _ = glyph.draw(
                    DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
                    &mut outline,
                );
            }
            pen + step
        });
    });
    Ok(path.detach())
}
