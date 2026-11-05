pub mod assets;
pub mod effects;
pub mod export;
pub mod fonts;
pub mod form;
pub mod onion;
pub mod overlay;
pub mod pngwrite;
pub mod shapes;
pub mod sheet;
pub mod sprites;
pub mod text;
pub mod vector;

pub const RENDER_VERSION: u32 = 2;

use anyhow::{Context, Result, anyhow, bail};
use effects::{Img, Space};
use klad_brush::{Raster, TILE};
use klad_core::color::parse_hex;
use klad_core::doc::{Blend, Document, Kind, Layer, LayerContent};
use klad_core::fill::FillRef;
use klad_core::look::{self, Adjustment, Effect};
use klad_core::rng::{FNV_OFFSET, fnv};
use onion::Onions;
use rayon::prelude::*;
use skia_safe::{
    AlphaType, BlendMode, Canvas, Color4f, ColorSpace, ColorType, Data, FilterMode, IRect, Image, ImageInfo,
    MipmapMode, Paint, Pixmap, Rect, SamplingOptions, canvas::SaveLayerRec, image::CachingHint, images, png_encoder,
    surfaces,
};
use std::collections::BTreeMap;

pub type Rasters = BTreeMap<String, Raster>;

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Frame {
    pub fn hash(&self) -> u64 {
        fnv(FNV_OFFSET, &self.rgba)
    }

    pub fn png(&mut self) -> Result<Vec<u8>> {
        let info = rgba8_info(self.width, self.height);
        let pixmap =
            Pixmap::new(&info, &mut self.rgba, self.width as usize * 4).ok_or_else(|| anyhow!("bad pixmap"))?;
        let mut options = png_encoder::Options::default();
        options.z_lib_level = 2;
        options.filter_flags = png_encoder::FilterFlag::SUB;
        let mut out = Vec::new();
        if !png_encoder::encode(&pixmap, &mut out, &options) {
            return Err(anyhow!("PNG encoding failed"));
        }
        Ok(out)
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * self.width + x) * 4) as usize;
        [self.rgba[at], self.rgba[at + 1], self.rgba[at + 2], self.rgba[at + 3]]
    }
}

pub(crate) fn f16_info(width: u32, height: u32) -> ImageInfo {
    ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBAF16,
        AlphaType::Premul,
        ColorSpace::new_srgb(),
    )
}

pub(crate) fn rgba8_info(width: u32, height: u32) -> ImageInfo {
    ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Unpremul,
        ColorSpace::new_srgb(),
    )
}

fn blend_mode(blend: Blend) -> BlendMode {
    match blend {
        Blend::Normal => BlendMode::SrcOver,
        Blend::Multiply => BlendMode::Multiply,
        Blend::Screen => BlendMode::Screen,
        Blend::Overlay => BlendMode::Overlay,
        Blend::Add => BlendMode::Plus,
        Blend::ColorDodge => BlendMode::ColorDodge,
        Blend::SoftLight => BlendMode::SoftLight,
        Blend::HardLight => BlendMode::HardLight,
        Blend::Lighten => BlendMode::Lighten,
        Blend::Darken => BlendMode::Darken,
        Blend::Color => BlendMode::Color,
        Blend::Luminosity => BlendMode::Luminosity,
    }
}

pub(crate) fn background(hex: Option<&str>) -> Result<Color4f> {
    match hex {
        Some(hex) if hex != "none" => {
            let [r, g, b, a] = parse_hex(hex)?;
            Ok(Color4f::new(r, g, b, a))
        }
        _ => Ok(Color4f::new(0.0, 0.0, 0.0, 0.0)),
    }
}

fn tile_image(tile: &[half::f16]) -> Result<Image> {
    let bytes: &[u8] = bytemuck::cast_slice(tile);
    images::raster_from_data(&f16_info(TILE as u32, TILE as u32), Data::new_copy(bytes), TILE * 8)
        .ok_or_else(|| anyhow!("cannot wrap tile"))
}

fn draw_data(canvas: &Canvas, tile: &[half::f16], index: usize, cols: usize) -> Result<()> {
    let (tx, ty) = (index % cols, index / cols);
    canvas.draw_image(tile_image(tile)?, ((tx * TILE) as f32, (ty * TILE) as f32), None);
    Ok(())
}

fn draw_tile(canvas: &Canvas, raster: &Raster, index: usize) -> Result<()> {
    raster
        .at(index)
        .map_or(Ok(()), |tile| draw_data(canvas, tile, index, raster.cols))
}

fn tiles_in(cols: usize, rows: usize, area: IRect) -> Vec<usize> {
    let t = TILE as i32;
    if area.is_empty() || cols == 0 || rows == 0 {
        return Vec::new();
    }
    let (x0, y0) = (area.left.div_euclid(t).max(0), area.top.div_euclid(t).max(0));
    let x1 = (area.right - 1).div_euclid(t).min(cols as i32 - 1);
    let y1 = (area.bottom - 1).div_euclid(t).min(rows as i32 - 1);
    (y0..=y1)
        .flat_map(|ty| (x0..=x1).map(move |tx| ty as usize * cols + tx as usize))
        .collect()
}

pub fn raster_image(raster: &Raster) -> Result<Image> {
    let (w, h) = (raster.width as usize, raster.height as usize);
    let mut pixels = vec![half::f16::ZERO; w * h * 4];
    raster.occupied().for_each(|(index, tile)| {
        let (x0, y0) = ((index % raster.cols) * TILE, (index / raster.cols) * TILE);
        let cols = TILE.min(w - x0);
        (0..TILE.min(h - y0)).for_each(|row| {
            let to = ((y0 + row) * w + x0) * 4;
            pixels[to..to + cols * 4].copy_from_slice(&tile[row * TILE * 4..row * TILE * 4 + cols * 4]);
        });
    });
    images::raster_from_data(
        &f16_info(raster.width, raster.height),
        Data::new_copy(bytemuck::cast_slice(&pixels)),
        w * 8,
    )
    .ok_or_else(|| anyhow!("cannot wrap a {w}x{h} raster"))
}

fn draw_raster(canvas: &Canvas, raster: &Raster, ctx: Ctx) -> Result<()> {
    if ctx.detail != 1.0 {
        canvas.save();
        canvas.scale((1.0 / ctx.detail, 1.0 / ctx.detail));
        let drawn = draw_raster(
            canvas,
            raster,
            Ctx {
                detail: 1.0,
                sampling: None,
                ..ctx
            },
        );
        canvas.restore();
        return drawn;
    }
    match (ctx.area, ctx.sampling) {
        (Some(area), _) => tiles_in(raster.cols, raster.rows, area)
            .into_iter()
            .try_for_each(|i| draw_tile(canvas, raster, i)),
        (None, Some(sampling)) => {
            if raster.count() > 0 {
                let image = raster_image(raster)?;
                let image = match sampling.use_cubic || sampling.mipmap == MipmapMode::None {
                    true => image,
                    false => image
                        .with_default_mipmaps()
                        .ok_or_else(|| anyhow!("cannot build mipmaps"))?,
                };
                canvas.draw_image_with_sampling_options(image, (0, 0), sampling, None);
            }
            Ok(())
        }
        (None, None) => raster
            .occupied()
            .try_for_each(|(index, _)| draw_tile(canvas, raster, index)),
    }
}

pub type SwapTiles = BTreeMap<usize, Option<Vec<half::f16>>>;

#[derive(Clone, Copy)]
pub struct Swap<'a> {
    pub cel: &'a str,
    pub cols: usize,
    pub rows: usize,
    pub tiles: &'a SwapTiles,
}

#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub frame: u32,
    pub rasters: &'a Rasters,
    pub area: Option<IRect>,
    pub swap: Option<Swap<'a>>,
    pub sampling: Option<SamplingOptions>,
    pub onions: Option<&'a Onions>,
    pub flat: bool,
    pub doc: Option<&'a Document>,
    pub references: bool,
    pub detail: f32,
}

impl<'a> Ctx<'a> {
    pub fn full(frame: u32, rasters: &'a Rasters) -> Self {
        Self {
            frame,
            rasters,
            area: None,
            swap: None,
            sampling: None,
            onions: None,
            flat: true,
            doc: None,
            references: false,
            detail: 1.0,
        }
    }

    fn nested(self) -> Self {
        Self { flat: false, ..self }
    }

    fn effects(self, layer: &Layer) -> std::borrow::Cow<'_, [Effect]> {
        match self.doc {
            Some(doc) => doc.effects_of(layer),
            None => std::borrow::Cow::Borrowed(&layer.effects),
        }
    }

    fn shows(self, layer: &Layer) -> bool {
        layer.visible && (self.references || !layer.reference)
    }
}

fn layer_paint(opacity: f32, blend: Blend) -> Paint {
    let mut paint = Paint::default();
    paint.set_alpha_f(opacity);
    paint.set_blend_mode(blend_mode(blend));
    paint
}

fn layer_paint_isolated(canvas: &Canvas, opacity: f32, blend: Blend) {
    canvas.save_layer(&SaveLayerRec::default().paint(&layer_paint(opacity, blend)));
}

fn mode_layer(canvas: &Canvas, mode: BlendMode) {
    let mut paint = Paint::default();
    paint.set_blend_mode(mode);
    canvas.save_layer(&SaveLayerRec::default().paint(&paint));
}

fn clip_groups<'a>(layers: impl IntoIterator<Item = &'a Layer>) -> Vec<(&'a Layer, Vec<&'a Layer>)> {
    layers.into_iter().fold(Vec::new(), |mut out, layer| {
        match out.last_mut() {
            Some((_, clipped)) if layer.clip => clipped.push(layer),
            _ => out.push((layer, Vec::new())),
        }
        out
    })
}

pub fn draw_layers(canvas: &Canvas, layers: &[Layer], ctx: Ctx) -> Result<()> {
    let groups = clip_groups(layers);
    let reads = groups.iter().any(|(base, _)| ctx.shows(base) && base.reads_under());
    let draw = |c: &Canvas, x: Ctx| {
        groups
            .iter()
            .try_for_each(|(base, clipped)| draw_clipped(c, base, clipped, x))
    };
    match reads && !ctx.flat {
        true => effected(canvas, ctx, &[], None, 1.0, Blend::Normal, &draw),
        false => draw(canvas, ctx),
    }
}

fn draw_onions<'a>(canvas: &Canvas, layers: impl Iterator<Item = &'a Layer>, ctx: Ctx) -> Result<()> {
    let Some(onions) = ctx.onions else {
        return Ok(());
    };
    layers
        .filter_map(|l| onions.get(&l.id))
        .try_for_each(|r| draw_raster(canvas, r, ctx))
}

fn draw_clipped(canvas: &Canvas, base: &Layer, clipped: &[&Layer], ctx: Ctx) -> Result<()> {
    if !ctx.shows(base) {
        return Ok(());
    }
    if base.reads_under() {
        return match ctx.flat {
            true => adjust_under(canvas, base, ctx),
            false => Ok(()),
        };
    }
    let shown: Vec<&Layer> = clipped.iter().copied().filter(|l| ctx.shows(l)).collect();
    draw_onions(canvas, std::iter::once(base).chain(shown.iter().copied()), ctx)?;
    let effects = ctx.effects(base);
    if !effects.is_empty() || shown.iter().any(|l| l.reads_under()) {
        return effected(
            canvas,
            ctx,
            &effects,
            Some(base),
            base.keyed(ctx.frame).0,
            base.blend,
            &|c, x| draw_unit(c, base, &shown, x),
        )
        .with_context(|| format!("layer {}", base.id));
    }
    if shown.is_empty() {
        return draw_layer(canvas, base, ctx);
    }
    let inner = ctx.nested();
    layer_paint_isolated(canvas, base.keyed(ctx.frame).0, base.blend);
    let drawn = draw_content(canvas, base, inner)
        .and_then(|_| shown.iter().try_for_each(|layer| draw_clip(canvas, base, layer, inner)));
    canvas.restore();
    drawn
}

pub(crate) fn draw_clipped_alone(canvas: &Canvas, base: &Layer, layer: &Layer, ctx: Ctx) -> Result<()> {
    layer_paint_isolated(canvas, layer.keyed(ctx.frame).0, Blend::Normal);
    let drawn = draw_styled(canvas, layer, ctx).and_then(|_| {
        mode_layer(canvas, BlendMode::DstIn);
        let base_drawn = draw_content(canvas, base, ctx.nested());
        canvas.restore();
        base_drawn
    });
    canvas.restore();
    drawn.with_context(|| format!("layer {} clipped to {}", layer.id, base.id))
}

fn draw_clip(canvas: &Canvas, base: &Layer, layer: &Layer, ctx: Ctx) -> Result<()> {
    layer_paint_isolated(canvas, layer.keyed(ctx.frame).0, layer.blend);
    let drawn = draw_styled(canvas, layer, ctx).and_then(|_| {
        mode_layer(canvas, BlendMode::DstIn);
        let base_drawn = draw_content(canvas, base, ctx);
        canvas.restore();
        base_drawn
    });
    canvas.restore();
    drawn.with_context(|| format!("layer {} clipped to {}", layer.id, base.id))
}

fn draw_unit(canvas: &Canvas, base: &Layer, shown: &[&Layer], ctx: Ctx) -> Result<()> {
    draw_content(canvas, base, ctx.nested())?;
    shown.iter().try_for_each(|layer| match layer.reads_under() {
        true => adjust_under(canvas, layer, ctx),
        false => draw_clip(canvas, base, layer, ctx.nested()),
    })
}

fn draw_styled(canvas: &Canvas, layer: &Layer, ctx: Ctx) -> Result<()> {
    let effects = ctx.effects(layer);
    match effects.is_empty() {
        true => draw_content(canvas, layer, ctx),
        false => effected(canvas, ctx, &effects, Some(layer), 1.0, Blend::Normal, &|c, x| {
            draw_content(c, layer, x)
        }),
    }
}

pub(crate) fn draw_layer(canvas: &Canvas, layer: &Layer, ctx: Ctx) -> Result<()> {
    if layer.reference && !ctx.references {
        return Ok(());
    }
    if layer.reads_under() {
        return match ctx.flat {
            true => adjust_under(canvas, layer, ctx),
            false => Ok(()),
        };
    }
    let effects = ctx.effects(layer);
    let opacity = layer.keyed(ctx.frame).0;
    if !effects.is_empty() {
        return effected(canvas, ctx, &effects, Some(layer), opacity, layer.blend, &|c, x| {
            draw_content(c, layer, x)
        })
        .with_context(|| format!("layer {}", layer.id));
    }
    let isolate = opacity < 1.0 || layer.blend != Blend::Normal || layer.mask_at(ctx.frame).is_some();
    if isolate {
        layer_paint_isolated(canvas, opacity, layer.blend);
    }
    let drawn = draw_content(canvas, layer, ctx);
    if isolate {
        canvas.restore();
    }
    drawn.with_context(|| format!("layer {}", layer.id))
}

fn device_space(canvas: &Canvas) -> Space {
    let matrix = canvas.local_to_device_as_3x3();
    Space {
        scale: matrix.scale_x().abs().max(1e-6),
        tx: matrix.translate_x(),
        ty: matrix.translate_y(),
    }
}

fn canvas_rect(window: IRect, space: Space) -> IRect {
    let to = |v: i32, t: f32| (v as f32 - t) / space.scale;
    IRect::from_ltrb(
        to(window.left, space.tx).floor() as i32,
        to(window.top, space.ty).floor() as i32,
        to(window.right, space.tx).ceil() as i32,
        to(window.bottom, space.ty).ceil() as i32,
    )
}

fn f32_info(width: u32, height: u32) -> ImageInfo {
    ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBAF32,
        AlphaType::Premul,
        ColorSpace::new_srgb(),
    )
}

fn read_img(surface: &mut skia_safe::Surface, window: IRect) -> Result<Img> {
    let (w, h) = (window.width() as usize, window.height() as usize);
    let mut px = vec![[0.0f32; 4]; w * h];
    if !surface.read_pixels(
        &f32_info(w as u32, h as u32),
        bytemuck::cast_slice_mut(&mut px),
        w * 16,
        (0, 0),
    ) {
        bail!("cannot read {w}x{h} pixels for effects");
    }
    Ok(Img {
        x: window.left,
        y: window.top,
        w,
        h,
        px,
    })
}

fn img_image(img: &Img) -> Result<Image> {
    images::raster_from_data(
        &f32_info(img.w as u32, img.h as u32),
        Data::new_copy(bytemuck::cast_slice(&img.px)),
        img.w * 16,
    )
    .ok_or_else(|| anyhow!("cannot wrap {}x{} pixels", img.w, img.h))
}

fn offscreen(window: IRect, canvas: &Canvas, ctx: Ctx, draw: &dyn Fn(&Canvas, Ctx) -> Result<()>) -> Result<Img> {
    let (w, h) = (window.width().max(1) as u32, window.height().max(1) as u32);
    let mut surface =
        surfaces::raster(&f16_info(w, h), None, None).ok_or_else(|| anyhow!("cannot allocate {w}x{h} pixels"))?;
    let inner = surface.canvas();
    inner.translate((-window.left as f32, -window.top as f32));
    inner.concat(&canvas.local_to_device_as_3x3());
    let area = ctx.area.map(|_| canvas_rect(window, device_space(canvas)));
    draw(
        inner,
        Ctx {
            area,
            flat: true,
            ..ctx
        },
    )?;
    read_img(&mut surface, window)
}

fn find<'a>(layers: &'a [Layer], id: &str) -> Option<&'a Layer> {
    layers
        .iter()
        .find_map(|l| (l.id == id).then_some(l).or_else(|| find(l.children(), id)))
}

fn painted_cels(layer: &Layer, ctx: Ctx) -> Vec<String> {
    if !ctx.shows(layer) {
        return Vec::new();
    }
    match &layer.content {
        LayerContent::Paint { .. } => layer.content_at(ctx.frame).map(|c| c.id.clone()).into_iter().collect(),
        LayerContent::Group { children } => children.iter().flat_map(|l| painted_cels(l, ctx)).collect(),
        _ => Vec::new(),
    }
}

fn height_at(heights: &klad_brush::tiles::Heights, x: i32, y: i32) -> f32 {
    if x < 0 || y < 0 || x as u32 >= heights.width || y as u32 >= heights.height {
        return 0.0;
    }
    let (x, y) = (x as usize, y as usize);
    heights
        .get(x / TILE, y / TILE)
        .map_or(0.0, |t| t[(y % TILE) * TILE + x % TILE].to_f32())
}

fn sampled_height(heights: &klad_brush::tiles::Heights, cx: f32, cy: f32) -> f32 {
    let (fx, fy) = (cx - 0.5, cy - 0.5);
    let (x, y) = (fx.floor(), fy.floor());
    let (ax, ay) = (fx - x, fy - y);
    let (x, y) = (x as i32, y as i32);
    let top = height_at(heights, x, y) * (1.0 - ax) + height_at(heights, x + 1, y) * ax;
    let bottom = height_at(heights, x, y + 1) * (1.0 - ax) + height_at(heights, x + 1, y + 1) * ax;
    top * (1.0 - ay) + bottom * ay
}

fn height_img(window: IRect, space: Space, layer: &Layer, ctx: Ctx) -> Option<Img> {
    let maps: Vec<&klad_brush::tiles::Heights> = painted_cels(layer, ctx)
        .iter()
        .filter_map(|id| ctx.rasters.get(id).and_then(Raster::heights))
        .collect();
    if maps.is_empty() {
        return None;
    }
    let (w, h) = (window.width().max(0) as usize, window.height().max(0) as usize);
    let taps = ((ctx.detail / space.scale).ceil() as usize).clamp(1, 4);
    let step = ctx.detail / (space.scale * taps as f32);
    let px = (0..h)
        .into_par_iter()
        .flat_map_iter(|row| {
            let maps = &maps;
            (0..w).map(move |col| {
                let (gx, gy) = (window.left + col as i32, window.top + row as i32);
                let (cx, cy) = (
                    (gx as f32 - space.tx) / space.scale * ctx.detail,
                    (gy as f32 - space.ty) / space.scale * ctx.detail,
                );
                let sum: f32 = (0..taps * taps)
                    .map(|k| {
                        let (sx, sy) = (
                            cx + (k % taps) as f32 * step + step * 0.5,
                            cy + (k / taps) as f32 * step + step * 0.5,
                        );
                        maps.iter().map(|m| sampled_height(m, sx, sy)).sum::<f32>()
                    })
                    .sum();
                [sum / (taps * taps) as f32, 0.0, 0.0, 0.0]
            })
        })
        .collect();
    Some(Img {
        x: window.left,
        y: window.top,
        w,
        h,
        px,
    })
}

#[allow(clippy::too_many_arguments)]
fn effected(
    canvas: &Canvas,
    ctx: Ctx,
    list: &[Effect],
    owner: Option<&Layer>,
    opacity: f32,
    blend: Blend,
    draw: &dyn Fn(&Canvas, Ctx) -> Result<()>,
) -> Result<()> {
    let Some(device) = canvas.device_clip_bounds().filter(|d| !d.is_empty()) else {
        return Ok(());
    };
    let space = device_space(canvas);
    let m = effects::margin(list, space.scale);
    let window = device.with_outset((m, m));
    let img = offscreen(window, canvas, ctx, draw)?;
    let by = |id: &str| -> Result<Option<Img>> {
        let Some(layer) = find(ctx.doc.map_or(&[][..], |d| &d.layers[..]), id) else {
            bail!("the displacement layer {id} is gone");
        };
        offscreen(window, canvas, ctx, &|c, x| draw_content(c, layer, x.nested())).map(Some)
    };
    let height = || Ok(owner.and_then(|layer| height_img(window, space, layer, ctx)));
    let (passes, image) = match list.is_empty() {
        true => (Vec::new(), img),
        false => effects::run(list, img, space, &by, &height)?,
    };
    canvas.save();
    canvas.reset_matrix();
    let drawn = passes
        .iter()
        .map(|p| (&p.img, p.blend))
        .chain(std::iter::once((&image, blend)))
        .try_for_each(|(img, mode)| {
            let part = match m {
                0 => img_image(img)?,
                _ => img_image(&img.crop(
                    device.left,
                    device.top,
                    device.width() as usize,
                    device.height() as usize,
                ))?,
            };
            canvas.draw_image(
                part,
                (device.left as f32, device.top as f32),
                Some(&layer_paint(opacity, mode)),
            );
            Ok(())
        });
    canvas.restore();
    drawn
}

fn adjust_under(canvas: &Canvas, layer: &Layer, ctx: Ctx) -> Result<()> {
    let Some(adjust) = layer.adjustment() else {
        return Ok(());
    };
    let prepared = adjust.prepare().with_context(|| format!("layer {}", layer.id))?;
    let size = canvas.base_layer_size();
    let (w, h) = (size.width.max(0) as usize, size.height.max(0) as usize);
    if w == 0 || h == 0 {
        return Ok(());
    }
    let info = f32_info(w as u32, h as u32);
    let mut pixels = vec![[0.0f32; 4]; w * h];
    if !canvas.read_pixels(&info, bytemuck::cast_slice_mut(&mut pixels), w * 16, (0, 0)) {
        bail!("cannot read the pixels under adjustment layer {}", layer.id);
    }
    let mask = match layer.mask_at(ctx.frame) {
        Some((mask, cel)) => {
            let window = IRect::from_wh(w as i32, h as i32);
            let cel = layer_cel(mask, cel);
            let img = offscreen(window, canvas, ctx, &|c, x| draw_paint_cel(c, &cel, x))?;
            Some(img.px.iter().map(|p| p[3]).collect::<Vec<f32>>())
        }
        None => None,
    };
    let (opacity, blend) = (layer.opacity, layer.blend);
    pixels.par_iter_mut().enumerate().for_each(|(i, px)| {
        let a = px[3];
        let k = opacity * (1.0 - mask.as_ref().map_or(0.0, |m| m[i]));
        if a <= 0.0 || k <= 0.0 {
            return;
        }
        let un = [px[0] / a, px[1] / a, px[2] / a];
        let mixed = look::blend_rgb(blend, un, prepared.apply(un));
        (0..3).for_each(|c| px[c] = (un[c] + (mixed[c] - un[c]) * k) * a);
    });
    if !canvas.write_pixels(&info, bytemuck::cast_slice(&pixels), w * 16, (0, 0)) {
        bail!("cannot write the pixels under adjustment layer {}", layer.id);
    }
    Ok(())
}

fn draw_paint_cel(canvas: &Canvas, cel: &str, ctx: Ctx) -> Result<()> {
    match ctx.swap {
        Some(swap) if swap.cel == cel => {
            let raster = ctx.rasters.get(cel);
            let indices = match ctx.area {
                Some(area) => tiles_in(swap.cols, swap.rows, area),
                None => (0..swap.cols * swap.rows).collect(),
            };
            indices.into_iter().try_for_each(|i| match swap.tiles.get(&i) {
                Some(Some(tile)) => draw_data(canvas, tile, i, swap.cols),
                Some(None) => Ok(()),
                None => raster.map_or(Ok(()), |r| draw_tile(canvas, r, i)),
            })
        }
        _ => ctx.rasters.get(cel).map_or(Ok(()), |r| draw_raster(canvas, r, ctx)),
    }
}

fn draw_content(canvas: &Canvas, layer: &Layer, ctx: Ctx) -> Result<()> {
    let Some(moved) = layer.keyed(ctx.frame).1 else {
        return draw_unmoved(canvas, layer, ctx);
    };
    let pivot = ctx.doc.map_or((0.0, 0.0), |d| {
        (d.canvas.width as f32 / 2.0, d.canvas.height as f32 / 2.0)
    });
    canvas.save();
    canvas.translate((moved.x + pivot.0, moved.y + pivot.1));
    canvas.rotate(moved.rotate, None);
    canvas.scale((moved.sx, moved.sy));
    canvas.translate((-pivot.0, -pivot.1));
    let whole = Ctx {
        area: None,
        sampling: Some(
            ctx.sampling
                .unwrap_or(SamplingOptions::new(FilterMode::Linear, MipmapMode::None)),
        ),
        ..ctx
    };
    let drawn = draw_unmoved(canvas, layer, whole);
    canvas.restore();
    drawn
}

fn draw_unmoved(canvas: &Canvas, layer: &Layer, ctx: Ctx) -> Result<()> {
    match &layer.content {
        LayerContent::Paint { .. } => layer
            .content_at(ctx.frame)
            .map_or(Ok(()), |cel| draw_paint_cel(canvas, &cel.id, ctx)),
        LayerContent::Vector { .. } => {
            layer
                .content_at(ctx.frame)
                .map_or(Ok(()), |cel| match ctx.rasters.get(&cel.id) {
                    Some(r) => draw_raster(canvas, r, ctx),
                    None => vector::draw_nodes(canvas, &klad_core::doc::nodes_at(&cel.nodes, ctx.frame)),
                })
        }
        LayerContent::Group { children } => draw_layers(canvas, children, ctx.nested()),
        LayerContent::Adjust {
            adjust: Adjustment::Fill(fill),
        } => {
            if let Some(paint) = vector::shader_paint(&fill.paint)? {
                canvas.draw_paint(&paint);
            }
            Ok(())
        }
        LayerContent::Adjust { .. } => Ok(()),
    }?;
    if let Some((mask, cel)) = layer.mask_at(ctx.frame) {
        mode_layer(canvas, BlendMode::DstOut);
        let drawn = draw_paint_cel(canvas, &layer_cel(mask, cel), ctx);
        canvas.restore();
        drawn.with_context(|| format!("mask {}", mask.id))?;
    }
    Ok(())
}

fn layer_cel(layer: &Layer, cel: &klad_core::doc::Cel) -> String {
    layer.content_of(cel).id.clone()
}

pub struct Lifted {
    pub rect: [f32; 4],
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<half::f16>,
}

pub fn rasterize_region(nodes: &[klad_core::doc::Node], rect: [f32; 4], max_side: u32, scale: f32) -> Result<Lifted> {
    let [x, y, w, h] = rect;
    let scale = (max_side as f32 / w.max(h)).min(scale.clamp(0.05, 1.0));
    let (width, height) = (((w * scale).ceil() as u32).max(1), ((h * scale).ceil() as u32).max(1));
    let info = f16_info(width, height);
    let mut surface = surfaces::raster(&info, None, None).ok_or_else(|| anyhow!("cannot allocate {width}x{height}"))?;
    let canvas = surface.canvas();
    canvas.scale((scale, scale));
    canvas.translate((-x, -y));
    vector::draw_nodes(canvas, nodes)?;
    let mut pixels = vec![half::f16::ZERO; width as usize * height as usize * 4];
    if !surface.read_pixels(&info, bytemuck::cast_slice_mut(&mut pixels), width as usize * 8, (0, 0)) {
        return Err(anyhow!("cannot read pixels"));
    }
    Ok(Lifted {
        rect: [x, y, width as f32 / scale, height as f32 / scale],
        width,
        height,
        pixels,
    })
}

pub fn rasterize_nodes(nodes: &[klad_core::doc::Node], width: u32, height: u32) -> Result<Raster> {
    rasterize_nodes_at(nodes, width, height, 1.0)
}

pub fn rasterize_nodes_at(nodes: &[klad_core::doc::Node], width: u32, height: u32, detail: f32) -> Result<Raster> {
    let (width, height) = (
        (width as f32 * detail).ceil() as u32,
        (height as f32 * detail).ceil() as u32,
    );
    let info = f16_info(width, height);
    let mut surface = surfaces::raster(&info, None, None).ok_or_else(|| anyhow!("cannot allocate {width}x{height}"))?;
    surface.canvas().scale((detail, detail));
    vector::draw_nodes(surface.canvas(), nodes)?;
    let (w, h) = (width as usize, height as usize);
    let mut pixels = vec![half::f16::ZERO; w * h * 4];
    if !surface.read_pixels(&info, bytemuck::cast_slice_mut(&mut pixels), w * 8, (0, 0)) {
        return Err(anyhow!("cannot read pixels"));
    }
    let mut raster = Raster::new(width, height);
    (0..raster.rows).for_each(|ty| {
        (0..raster.cols).for_each(|tx| {
            let mut tile = vec![half::f16::ZERO; TILE * TILE * 4];
            let (x0, y0) = (tx * TILE, ty * TILE);
            let cols = TILE.min(w - x0);
            (0..TILE.min(h - y0)).for_each(|row| {
                let from = ((y0 + row) * w + x0) * 4;
                tile[row * TILE * 4..row * TILE * 4 + cols * 4].copy_from_slice(&pixels[from..from + cols * 4]);
            });
            if tile.as_chunks::<4>().0.iter().any(|px| px[3].to_f32() > 0.0) {
                let index = raster.index(tx, ty);
                raster.put(index, tile.into_boxed_slice());
            }
        })
    });
    Ok(raster)
}

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub scale: f32,
    pub region: Option<[i32; 4]>,
    pub references: bool,
}

impl View {
    pub fn scaled(scale: f32) -> Self {
        Self {
            scale,
            region: None,
            references: false,
        }
    }
}

pub fn compose(doc: &Document, frame: u32, rasters: &Rasters, view: View) -> Result<Frame> {
    compose_view(doc, frame, rasters, view, None, None)
}

pub fn compose_view(
    doc: &Document,
    frame: u32,
    rasters: &Rasters,
    view: View,
    overlay: Option<&overlay::Overlay>,
    onions: Option<&Onions>,
) -> Result<Frame> {
    let (width, height) = (doc.canvas.width, doc.canvas.height);
    let mut surface = surfaces::raster(&f16_info(width, height), None, None)
        .ok_or_else(|| anyhow!("cannot allocate {width}x{height}"))?;
    let canvas = surface.canvas();
    canvas.clear(background(doc.canvas.background.as_deref())?);
    let ctx = Ctx {
        onions,
        doc: Some(doc),
        references: view.references,
        ..Ctx::full(frame, rasters)
    };
    draw_layers(canvas, &doc.layers, ctx)?;
    let image = match view.region {
        Some([x, y, w, h]) => surface
            .image_snapshot_with_bounds(IRect::from_xywh(x, y, w, h))
            .ok_or_else(|| anyhow!("region {x},{y} {w}x{h} is outside the canvas"))?,
        None => surface.image_snapshot(),
    };
    let out = if (view.scale - 1.0).abs() < f32::EPSILON {
        image
    } else {
        resample(&image, view.scale)?
    };
    match overlay {
        Some(overlay) => overlay::apply(&out, overlay, &view),
        None => read_rgba8(&out),
    }
}

pub fn compose_image(doc: &Document, frame: u32, rasters: &Rasters) -> Result<Image> {
    let (width, height) = (doc.canvas.width, doc.canvas.height);
    let mut surface = surfaces::raster(&f16_info(width, height), None, None)
        .ok_or_else(|| anyhow!("cannot allocate {width}x{height}"))?;
    let canvas = surface.canvas();
    canvas.clear(background(doc.canvas.background.as_deref())?);
    let ctx = Ctx {
        doc: Some(doc),
        ..Ctx::full(frame, rasters)
    };
    draw_layers(canvas, &doc.layers, ctx)?;
    Ok(surface.image_snapshot())
}

fn resample(image: &Image, scale: f32) -> Result<Image> {
    let (w, h) = (
        ((image.width() as f32 * scale).round() as u32).max(1),
        ((image.height() as f32 * scale).round() as u32).max(1),
    );
    let source = if scale < 1.0 {
        image
            .with_default_mipmaps()
            .ok_or_else(|| anyhow!("cannot build mipmaps"))?
    } else {
        image.clone()
    };
    let mut surface =
        surfaces::raster(&f16_info(w, h), None, None).ok_or_else(|| anyhow!("cannot allocate {w}x{h}"))?;
    let sampling = if scale < 1.0 {
        SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear)
    } else {
        SamplingOptions::new(FilterMode::Nearest, MipmapMode::None)
    };
    surface.canvas().draw_image_rect_with_sampling_options(
        &source,
        None,
        Rect::from_wh(w as f32, h as f32),
        sampling,
        &Paint::default(),
    );
    Ok(surface.image_snapshot())
}

pub fn read_rgba8(image: &Image) -> Result<Frame> {
    let (width, height) = (image.width() as u32, image.height() as u32);
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    if !image.read_pixels(
        &rgba8_info(width, height),
        &mut rgba,
        width as usize * 4,
        (0, 0),
        CachingHint::Allow,
    ) {
        return Err(anyhow!("cannot read pixels"));
    }
    Ok(Frame { width, height, rgba })
}

pub enum TileItem<'a> {
    Pixels {
        tile: &'a [half::f16],
        opacity: f32,
        blend: Blend,
    },
    Layer(&'a Layer),
}

enum Unit<'a> {
    Pixels {
        tile: &'a [half::f16],
        opacity: f32,
        blend: Blend,
    },
    Layers(&'a Layer, Vec<&'a Layer>),
}

fn units<'a>(items: &[TileItem<'a>]) -> Vec<Unit<'a>> {
    items.iter().fold(Vec::new(), |mut out, item| {
        match (item, out.last_mut()) {
            (TileItem::Layer(layer), Some(Unit::Layers(_, clipped))) if layer.clip => clipped.push(layer),
            (TileItem::Layer(layer), _) => out.push(Unit::Layers(layer, Vec::new())),
            (TileItem::Pixels { tile, opacity, blend }, _) => out.push(Unit::Pixels {
                tile,
                opacity: *opacity,
                blend: *blend,
            }),
        }
        out
    })
}

pub struct TileJob<'a> {
    pub frame: u32,
    pub rasters: &'a Rasters,
    pub background: Option<&'a str>,
    pub tx: usize,
    pub ty: usize,
    pub swap: Option<Swap<'a>>,
    pub onions: Option<&'a Onions>,
    pub doc: &'a Document,
    pub references: bool,
}

pub fn flatten(
    doc: &Document,
    frame: u32,
    rasters: &Rasters,
    layers: &[&Layer],
    background: Option<&str>,
) -> Result<Raster> {
    flatten_with(doc, frame, rasters, layers, background, None, false)
}

pub fn flatten_with(
    doc: &Document,
    frame: u32,
    rasters: &Rasters,
    layers: &[&Layer],
    background: Option<&str>,
    onions: Option<&Onions>,
    references: bool,
) -> Result<Raster> {
    let mut out = Raster::new(doc.canvas.width, doc.canvas.height);
    let items: Vec<TileItem> = layers.iter().map(|l| TileItem::Layer(l)).collect();
    let tiles = (0..out.cols * out.rows)
        .into_par_iter()
        .map(|index| {
            let job = TileJob {
                frame,
                rasters,
                background,
                tx: index % out.cols,
                ty: index / out.cols,
                swap: None,
                onions,
                doc,
                references,
            };
            let tile = compose_tile(&items, &job)?;
            let used = tile.as_chunks::<4>().0.iter().any(|px| px[3].to_f32() > 0.0);
            Ok(used.then_some((index, tile)))
        })
        .collect::<Result<Vec<_>>>()?;
    tiles
        .into_iter()
        .flatten()
        .for_each(|(index, tile)| out.put(index, tile.into_boxed_slice()));
    Ok(out)
}

pub fn fill_reference(doc: &Document, frame: u32, rasters: &Rasters, cel: &str, reference: &FillRef) -> Result<Raster> {
    let (w, h) = (doc.canvas.width, doc.canvas.height);
    match reference {
        FillRef::This => Ok(rasters.get(cel).cloned().unwrap_or_else(|| Raster::new(w, h))),
        FillRef::All => {
            let layers: Vec<&Layer> = doc.layers.iter().collect();
            flatten(doc, frame, rasters, &layers, doc.canvas.background.as_deref())
        }
        FillRef::Layer(id) => {
            let layer = doc
                .layer(id)
                .ok_or_else(|| anyhow!("the fill reference layer {id} is gone"))?;
            if layer.kind() == Kind::Paint {
                return Ok(layer
                    .content_at(frame)
                    .and_then(|c| rasters.get(&c.id))
                    .cloned()
                    .unwrap_or_else(|| Raster::new(w, h)));
            }
            let mut alone = layer.clone();
            alone.visible = true;
            alone.opacity = 1.0;
            alone.blend = Blend::Normal;
            alone.reference = false;
            flatten(doc, frame, rasters, &[&alone], None)
        }
    }
}

pub fn compose_tile(items: &[TileItem], job: &TileJob) -> Result<Vec<half::f16>> {
    let info = f16_info(TILE as u32, TILE as u32);
    let mut surface = surfaces::raster(&info, None, None).ok_or_else(|| anyhow!("cannot allocate a tile"))?;
    let canvas = surface.canvas();
    canvas.clear(background(job.background)?);
    let (ox, oy) = ((job.tx * TILE) as f32, (job.ty * TILE) as f32);
    canvas.translate((-ox, -oy));
    let ctx = Ctx {
        frame: job.frame,
        rasters: job.rasters,
        area: Some(IRect::from_xywh(ox as i32, oy as i32, TILE as i32, TILE as i32)),
        swap: job.swap,
        sampling: None,
        onions: job.onions,
        flat: true,
        doc: Some(job.doc),
        references: job.references,
        detail: 1.0,
    };
    units(items).into_iter().try_for_each(|unit| match unit {
        Unit::Pixels { tile, opacity, blend } => {
            canvas.draw_image(tile_image(tile)?, (ox, oy), Some(&layer_paint(opacity, blend)));
            Ok(())
        }
        Unit::Layers(base, clipped) => draw_clipped(canvas, base, &clipped, ctx),
    })?;
    let mut out = vec![half::f16::ZERO; TILE * TILE * 4];
    if !surface.read_pixels(&info, bytemuck::cast_slice_mut(&mut out), TILE * 8, (0, 0)) {
        return Err(anyhow!("cannot read tile pixels"));
    }
    Ok(out)
}

#[cfg(test)]
mod look_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use klad_brush::{BrushSet, replay};
    use klad_core::doc::{Cel, Linear, Node, Paint as Fill, Shape, Stop, StrokeStyle};
    use klad_core::stroke::Stroke;

    fn rect(id: &str, x: f32, y: f32, w: f32, h: f32, fill: &str) -> Node {
        Node::new(
            id,
            Shape::Rect {
                x,
                y,
                w,
                h,
                radius: 0.0,
            },
        )
        .filled(fill)
    }

    fn close(a: [u8; 4], b: [u8; 4]) -> bool {
        a.iter().zip(&b).all(|(x, y)| x.abs_diff(*y) <= 2)
    }

    #[test]
    fn vector_and_paint_layers_compose_in_order() {
        let mut doc = Document::new(64, 64);
        doc.layers.push(Layer::vector(
            "bg",
            "bg",
            "c1",
            vec![rect("n1", 0.0, 0.0, 64.0, 64.0, "#0000ff")],
        ));
        doc.layers.push(Layer::paint("ink", "ink", "c2"));
        let stroke = Stroke::new(1, "ink", [1.0, 0.0, 0.0, 1.0], 12.0, 1).with_points(&[(32.0, 32.0, 1.0)], 0.0);
        let raster = replay(64, 64, &[stroke.into()], &BrushSet::presets()).unwrap().raster;
        let rasters = Rasters::from([("c2".to_string(), raster)]);
        let mut frame = compose(&doc, 0, &rasters, View::scaled(1.0)).unwrap();
        assert_eq!(frame.pixel(32, 32), [255, 0, 0, 255]);
        assert_eq!(frame.pixel(2, 2), [0, 0, 255, 255]);
        assert_eq!(compose(&doc, 0, &rasters, View::scaled(0.5)).unwrap().width, 32);
        assert!(frame.png().unwrap().starts_with(&[0x89, b'P', b'N', b'G']));
        let zoomed = compose(
            &doc,
            0,
            &rasters,
            View {
                scale: 2.0,
                region: Some([28, 28, 8, 8]),
                references: false,
            },
        )
        .unwrap();
        assert_eq!((zoomed.width, zoomed.height), (16, 16));
        assert_eq!(zoomed.pixel(8, 8), [255, 0, 0, 255]);
    }

    #[test]
    fn groups_isolate_opacity_and_blend() {
        let mut doc = Document::new(8, 8);
        doc.layers.push(Layer::vector(
            "a",
            "a",
            "c1",
            vec![rect("n1", 0.0, 0.0, 8.0, 8.0, "#ffffff")],
        ));
        let black = Layer::vector("b", "b", "c2", vec![rect("n2", 0.0, 0.0, 8.0, 8.0, "#000000")]);
        let mut group = Layer::group("g", "g", vec![black]);
        group.opacity = 0.5;
        doc.layers.push(group);
        let grey = compose(&doc, 0, &Rasters::new(), View::scaled(1.0))
            .unwrap()
            .pixel(4, 4);
        assert!((126..=129).contains(&grey[0]), "{grey:?}");
        doc.layers[1].opacity = 1.0;
        doc.layers[1].blend = Blend::Multiply;
        assert_eq!(
            compose(&doc, 0, &Rasters::new(), View::scaled(1.0))
                .unwrap()
                .pixel(4, 4),
            [0, 0, 0, 255]
        );
        doc.layers[1].visible = false;
        assert_eq!(
            compose(&doc, 0, &Rasters::new(), View::scaled(1.0))
                .unwrap()
                .pixel(4, 4),
            [255, 255, 255, 255]
        );
    }

    #[test]
    fn frames_pick_their_cels() {
        let mut doc = Document::new(4, 4);
        doc.timeline.frames = 3;
        let mut layer = Layer::vector("v", "v", "c1", vec![rect("n1", 0.0, 0.0, 4.0, 4.0, "#ff0000")]);
        if let Some(cels) = layer.cels_mut() {
            cels[0].hold = Some(1);
            let mut second = Cel::at("c2", 1);
            second.nodes = vec![rect("n2", 0.0, 0.0, 4.0, 4.0, "#00ff00")];
            second.hold = Some(1);
            cels.push(second);
            cels.push(Cel {
                link: Some("c1".into()),
                ..Cel::at("c3", 2)
            });
        }
        doc.layers.push(layer);
        let at = |f| {
            compose(&doc, f, &Rasters::new(), View::scaled(1.0))
                .unwrap()
                .pixel(1, 1)
        };
        assert_eq!(
            [at(0), at(1), at(2)],
            [[255, 0, 0, 255], [0, 255, 0, 255], [255, 0, 0, 255]]
        );
    }

    #[test]
    fn transforms_gradients_and_strokes_render() {
        let mut doc = Document::new(100, 100);
        let mut bar = rect("bar", 40.0, 0.0, 20.0, 100.0, "#000000");
        bar.transform.rotate = 90.0;
        let mut sky = rect("sky", 0.0, 0.0, 100.0, 100.0, "#000000");
        sky.fill = Some(Fill::Linear {
            linear: Linear {
                from: [0.0, 0.0],
                to: [0.0, 100.0],
                stops: vec![Stop(0.0, "#ff0000".into()), Stop(1.0, "#0000ff".into())],
            },
        });
        let mut ring = Node::new(
            "ring",
            Shape::Ellipse {
                cx: 80.0,
                cy: 80.0,
                rx: 10.0,
                ry: 10.0,
            },
        );
        ring.fill = Some(Fill::Solid("none".into()));
        ring.stroke = Some(StrokeStyle {
            paint: Fill::Solid("#00ff00".into()),
            width: 4.0,
            cap: Default::default(),
            join: Default::default(),
            dash: Vec::new(),
        });
        doc.layers.push(Layer::vector("v", "v", "c1", vec![sky, bar, ring]));
        let frame = compose(&doc, 0, &Rasters::new(), View::scaled(1.0)).unwrap();
        let top = frame.pixel(5, 2);
        let bottom = frame.pixel(5, 97);
        assert!(top[0] > 240 && top[2] < 15, "{top:?}");
        assert!(bottom[2] > 240 && bottom[0] < 15, "{bottom:?}");
        assert_eq!(frame.pixel(5, 50), [0, 0, 0, 255]);
        assert_ne!(frame.pixel(50, 5), [0, 0, 0, 255]);
        assert!(close(frame.pixel(90, 80), [0, 255, 0, 255]));
        assert_ne!(frame.pixel(80, 80)[1], 255);
        let bounds = vector::node_bounds(&doc.layers[0].cels()[0].nodes[1], &skia_safe::Matrix::new_identity())
            .unwrap()
            .unwrap();
        assert!((bounds.left - 0.0).abs() < 0.01 && (bounds.width() - 100.0).abs() < 0.01);
    }

    #[test]
    fn blur_softens_edges() {
        let mut doc = Document::new(100, 40);
        let mut soft = rect("soft", 50.0, 0.0, 50.0, 40.0, "#000000");
        soft.blur = 6.0;
        doc.layers.push(Layer::vector(
            "v",
            "v",
            "c1",
            vec![rect("hard", 0.0, 0.0, 50.0, 40.0, "#000000"), soft],
        ));
        let frame = compose(&doc, 0, &Rasters::new(), View::scaled(1.0)).unwrap();
        assert_eq!(frame.pixel(25, 20)[3], 255);
        assert_eq!(frame.pixel(75, 20)[3], 255);
        let edge = frame.pixel(98, 20)[3];
        assert!(edge > 20 && edge < 200, "{edge}");
        assert_eq!(frame.pixel(48, 2)[3], 255);
    }

    #[test]
    fn clip_paths_cut_children() {
        let mut doc = Document::new(100, 100);
        let mut g = Node::new(
            "g",
            Shape::Group {
                children: vec![rect("r", 0.0, 0.0, 100.0, 100.0, "#ff0000")],
            },
        );
        g.clip = Some("M0 0 L50 0 L50 100 L0 100 Z".into());
        doc.layers.push(Layer::vector("v", "v", "c1", vec![g]));
        let frame = compose(&doc, 0, &Rasters::new(), View::scaled(1.0)).unwrap();
        assert_eq!(frame.pixel(25, 50), [255, 0, 0, 255]);
        assert_eq!(frame.pixel(75, 50)[3], 0);
        let b = vector::node_box(&doc.layers[0].cels()[0].nodes[0]).unwrap().unwrap();
        assert!((b[2] - 50.0).abs() < 0.01, "{b:?}");
    }

    #[test]
    fn rasterized_vectors_and_swapped_tiles_match() {
        let mut doc = Document::new(600, 300);
        let mut ring = Node::new(
            "ring",
            Shape::Ellipse {
                cx: 300.0,
                cy: 150.0,
                rx: 200.0,
                ry: 100.0,
            },
        )
        .filled("#3366aa");
        ring.blur = 3.0;
        doc.layers.push(Layer::vector("v", "v", "cv", vec![ring]));
        let ink = Layer::paint("p", "p", "cp");
        doc.layers.push(Layer::group("g", "g", vec![ink]));
        let stroke = Stroke::new(1, "ink", [0.9, 0.1, 0.1, 1.0], 40.0, 1)
            .with_points(&[(20.0, 20.0, 1.0), (580.0, 280.0, 1.0)], 5.0);
        let raster = replay(600, 300, &[stroke.into()], &BrushSet::presets()).unwrap().raster;
        let plain = Rasters::from([("cp".to_string(), raster.clone())]);
        let full = compose(&doc, 0, &plain, View::scaled(1.0)).unwrap();
        let mut cached = plain.clone();
        cached.insert(
            "cv".into(),
            rasterize_nodes(&doc.layers[0].cels()[0].nodes, 600, 300).unwrap(),
        );
        let items: Vec<TileItem> = doc.layers.iter().map(TileItem::Layer).collect();
        let empty = SwapTiles::from([(1usize, None)]);
        let tile = |swap| {
            compose_tile(
                &items,
                &TileJob {
                    frame: 0,
                    rasters: &cached,
                    background: None,
                    tx: 1,
                    ty: 0,
                    swap,
                    onions: None,
                    doc: &doc,
                    references: false,
                },
            )
            .unwrap()
        };
        let px = |t: &[half::f16], x: usize, y: usize| -> [u8; 3] {
            let at = (y * TILE + x) * 4;
            let a = t[at + 3].to_f32().max(1e-6);
            [0, 1, 2].map(|c| (t[at + c].to_f32() / a * 255.0).round() as u8)
        };
        let ours = tile(None);
        [(10usize, 150usize), (60, 120), (200, 30)].iter().for_each(|&(x, y)| {
            let reference = full.pixel((256 + x) as u32, y as u32);
            let got = px(&ours, x, y);
            assert!(
                got.iter().zip(&reference[..3]).all(|(a, b)| a.abs_diff(*b) <= 1),
                "({x},{y}) {got:?} vs {reference:?}"
            );
        });
        let without_ink = tile(Some(Swap {
            cel: "cp",
            cols: 3,
            rows: 2,
            tiles: &empty,
        }));
        let ring_only = compose(&doc, 0, &Rasters::new(), View::scaled(1.0)).unwrap();
        let reference = ring_only.pixel(256 + 60, 120);
        assert_eq!(px(&without_ink, 60, 120), [reference[0], reference[1], reference[2]]);
    }

    #[test]
    fn tiles_match_the_full_composite() {
        let mut doc = Document::new(512, 300);
        doc.layers.push(Layer::vector(
            "bg",
            "bg",
            "c1",
            vec![
                Node::new(
                    "n1",
                    Shape::Ellipse {
                        cx: 256.0,
                        cy: 150.0,
                        rx: 200.0,
                        ry: 120.0,
                    },
                )
                .filled("#336699"),
            ],
        ));
        let mut ink = Layer::paint("ink", "ink", "c2");
        ink.opacity = 0.7;
        doc.layers.push(ink);
        let stroke = Stroke::new(1, "ink", [0.9, 0.2, 0.1, 1.0], 30.0, 1)
            .with_points(&[(40.0, 40.0, 1.0), (300.0, 260.0, 0.6), (480.0, 60.0, 0.9)], 5.0);
        let raster = replay(512, 300, &[stroke.into()], &BrushSet::presets()).unwrap().raster;
        let rasters = Rasters::from([("c2".to_string(), raster)]);
        let full = compose(&doc, 0, &rasters, View::scaled(1.0)).unwrap();
        let tile = rasters["c2"].get(1, 0).unwrap();
        let items = [
            TileItem::Layer(&doc.layers[0]),
            TileItem::Pixels {
                tile,
                opacity: 0.7,
                blend: Blend::Normal,
            },
        ];
        let job = TileJob {
            frame: 0,
            rasters: &rasters,
            background: None,
            tx: 1,
            ty: 0,
            swap: None,
            onions: None,
            doc: &doc,
            references: false,
        };
        let composed = compose_tile(&items, &job).unwrap();
        let px = &composed[(100 * TILE + 44) * 4..(100 * TILE + 44) * 4 + 4];
        let a = px[3].to_f32();
        let rgb: Vec<u8> = px[..3].iter().map(|v| (v.to_f32() / a * 255.0).round() as u8).collect();
        let reference = full.pixel(256 + 44, 100);
        assert!(
            rgb.iter().zip(&reference[..3]).all(|(x, y)| x.abs_diff(*y) <= 1),
            "{rgb:?} vs {reference:?}"
        );
    }

    #[test]
    fn onion_skin_sits_under_its_layer_and_tiles_agree() {
        let mut doc = Document::new(256, 64);
        doc.canvas.background = Some("#ffffff".into());
        doc.timeline.frames = 3;
        let mut ball = Layer::paint("ball", "ball", "c1");
        *ball.cels_mut().unwrap() = vec![
            Cel {
                hold: Some(1),
                ..Cel::at("c1", 0)
            },
            Cel {
                hold: Some(1),
                ..Cel::at("c2", 1)
            },
            Cel::at("c3", 2),
        ];
        let mut cover = Layer::paint("top", "top", "c9");
        cover.opacity = 0.5;
        doc.layers = vec![ball, cover];
        let dot = |x: f32| {
            let stroke = Stroke::new(1, "ink", [0.0, 0.0, 0.0, 1.0], 30.0, 1).with_points(&[(x, 32.0, 1.0)], 0.0);
            replay(256, 64, &[stroke.into()], &BrushSet::presets()).unwrap().raster
        };
        let rasters = Rasters::from([
            ("c1".to_string(), dot(40.0)),
            ("c2".to_string(), dot(128.0)),
            ("c3".to_string(), dot(216.0)),
            ("c9".to_string(), dot(40.0)),
        ]);
        let plan = onion::plan(&doc, 1, &onion::animated(&doc), &onion::Onion::default());
        assert_eq!(plan.len(), 1);
        let onions = onion::build(&doc, &plan, &rasters);
        let full = compose_view(&doc, 1, &rasters, View::scaled(1.0), None, Some(&onions)).unwrap();
        let plain = compose(&doc, 1, &rasters, View::scaled(1.0)).unwrap();
        let before = full.pixel(216 - 176, 32);
        let after = full.pixel(216, 32);
        assert!(
            before[0] > before[1] + 25,
            "red before, under the top layer: {before:?}"
        );
        assert!(after[1] > after[0] + 40, "green after: {after:?}");
        assert_eq!(full.pixel(128, 32), plain.pixel(128, 32));
        assert_eq!(plain.pixel(216, 32), [255, 255, 255, 255]);
        let items: Vec<TileItem> = doc.layers.iter().map(TileItem::Layer).collect();
        let tile = compose_tile(
            &items,
            &TileJob {
                frame: 1,
                rasters: &rasters,
                background: Some("#ffffff"),
                tx: 0,
                ty: 0,
                swap: None,
                onions: Some(&onions),
                doc: &doc,
                references: false,
            },
        )
        .unwrap();
        [40usize, 128, 216].iter().for_each(|&x| {
            let at = (32 * TILE + x) * 4;
            let got: Vec<u8> = (0..3).map(|c| (tile[at + c].to_f32() * 255.0).round() as u8).collect();
            let want = full.pixel(x as u32, 32);
            assert!(
                got.iter().zip(&want[..3]).all(|(a, b)| a.abs_diff(*b) <= 1),
                "{x}: {got:?} vs {want:?}"
            );
        });
    }

    #[test]
    fn masks_hide_and_clipped_layers_stay_inside_their_base() {
        let mut doc = Document::new(256, 128);
        doc.canvas.background = Some("#ffffff".into());
        let flat = Layer::vector(
            "flat",
            "flat",
            "c1",
            vec![rect("n1", 40.0, 20.0, 120.0, 80.0, "#d0a060")],
        );
        let mut shade = Layer::paint("shade", "shade", "c2");
        shade.clip = true;
        shade.blend = Blend::Multiply;
        let mut light = Layer::paint("light", "light", "c3");
        light.clip = true;
        light.mask = Some(Box::new(Layer::paint("m", "mask", "c4")));
        doc.layers = vec![flat, shade, light];
        let line = |id: u32, y: f32, color: [f32; 4], size: f32| {
            Stroke::new(id, "ink", color, size, 1).with_points(&[(0.0, y, 1.0), (256.0, y, 1.0)], 5.0)
        };
        let paint = |strokes: Vec<Stroke>| {
            replay(
                256,
                128,
                &strokes.into_iter().map(Into::into).collect::<Vec<_>>(),
                &BrushSet::presets(),
            )
            .unwrap()
            .raster
        };
        let rasters = Rasters::from([
            ("c2".to_string(), paint(vec![line(1, 60.0, [0.2, 0.2, 0.5, 1.0], 30.0)])),
            ("c3".to_string(), paint(vec![line(1, 30.0, [1.0, 1.0, 1.0, 1.0], 12.0)])),
            (
                "c4".to_string(),
                paint(vec![
                    Stroke::new(1, "ink", [0.0, 0.0, 0.0, 1.0], 30.0, 1)
                        .with_points(&[(100.0, 30.0, 1.0), (256.0, 30.0, 1.0)], 5.0),
                ]),
            ),
        ]);
        let frame = compose(&doc, 0, &rasters, View::scaled(1.0)).unwrap();
        let shaded = frame.pixel(100, 60);
        assert!(shaded[2] < 140 && shaded[0] < 60, "{shaded:?}");
        assert_eq!(frame.pixel(10, 60), [255, 255, 255, 255]);
        assert_eq!(frame.pixel(200, 60), [255, 255, 255, 255]);
        assert_eq!(frame.pixel(60, 30), [255, 255, 255, 255]);
        assert_eq!(frame.pixel(10, 30), [255, 255, 255, 255]);
        assert!(
            close(frame.pixel(130, 30), [0xd0, 0xa0, 0x60, 255]),
            "{:?}",
            frame.pixel(130, 30)
        );
        let items: Vec<TileItem> = doc.layers.iter().map(TileItem::Layer).collect();
        let tile = compose_tile(
            &items,
            &TileJob {
                frame: 0,
                rasters: &rasters,
                background: Some("#ffffff"),
                tx: 0,
                ty: 0,
                swap: None,
                onions: None,
                doc: &doc,
                references: false,
            },
        )
        .unwrap();
        [(100usize, 60usize), (60, 30), (130, 30), (200, 60)]
            .iter()
            .for_each(|&(x, y)| {
                let at = (y * TILE + x) * 4;
                let got: Vec<u8> = (0..3).map(|c| (tile[at + c].to_f32() * 255.0).round() as u8).collect();
                let want = frame.pixel(x as u32, y as u32);
                assert!(
                    got.iter().zip(&want[..3]).all(|(a, b)| a.abs_diff(*b) <= 1),
                    "({x},{y}) {got:?} vs {want:?}"
                );
            });
        doc.layers[0].visible = false;
        let hidden = compose(&doc, 0, &rasters, View::scaled(1.0)).unwrap();
        assert_eq!(hidden.pixel(100, 60), [255, 255, 255, 255]);
        doc.layers.remove(0);
        let leading = compose(&doc, 0, &rasters, View::scaled(1.0)).unwrap();
        assert!(leading.pixel(10, 60)[2] < 200, "{:?}", leading.pixel(10, 60));
    }
}
