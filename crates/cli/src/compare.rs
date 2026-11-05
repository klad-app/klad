use crate::agent::{Look, render_look};
use anyhow::{Context, Result, bail};
use klad_core::bundle::Bundle;
use klad_core::color;
use klad_render::View;
use serde_json::json;
use std::path::{Path, PathBuf};

type Lab = [f32; 3];

#[derive(Clone)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub lab: Vec<Lab>,
}

impl Image {
    pub fn from_rgba(w: usize, h: usize, rgba: &[u8]) -> Self {
        let lab = rgba
            .chunks(4)
            .map(|p| {
                let a = p[3] as f32 / 255.0;
                let c = |v: u8| v as f32 / 255.0 * a + (1.0 - a);
                color::to_oklab([c(p[0]), c(p[1]), c(p[2]), 1.0])
            })
            .collect();
        Self { w, h, lab }
    }

    fn resized(&self, w: usize, h: usize) -> Self {
        if (w, h) == (self.w, self.h) {
            return self.clone();
        }
        let (sx, sy) = (self.w as f32 / w as f32, self.h as f32 / h as f32);
        let at = |x: usize, y: usize| self.lab[y.min(self.h - 1) * self.w + x.min(self.w - 1)];
        let lab = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                let (fx, fy) = (
                    ((x as f32 + 0.5) * sx - 0.5).max(0.0),
                    ((y as f32 + 0.5) * sy - 0.5).max(0.0),
                );
                let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
                let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
                let mix = |a: Lab, b: Lab, t: f32| -> Lab { std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t) };
                mix(
                    mix(at(x0, y0), at(x0 + 1, y0), tx),
                    mix(at(x0, y0 + 1), at(x0 + 1, y0 + 1), tx),
                    ty,
                )
            })
            .collect();
        Self { w, h, lab }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub value: [f32; 3],
    pub chroma: f32,
    pub texture: f32,
    pub sharpness: f32,
    pub mean: Lab,
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    match sorted.len() {
        0 => 0.0,
        n => sorted[((n - 1) as f32 * p).round() as usize],
    }
}

pub fn stats(img: &Image, rect: [usize; 4]) -> Stats {
    let [x0, y0, w, h] = rect;
    let (x1, y1) = ((x0 + w).min(img.w), (y0 + h).min(img.h));
    let at = |x: usize, y: usize| img.lab[y * img.w + x];
    let cells: Vec<(usize, usize)> = (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).collect();
    let n = cells.len().max(1) as f32;
    let mut values: Vec<f32> = cells.iter().map(|&(x, y)| at(x, y)[0] * 100.0).collect();
    values.sort_by(f32::total_cmp);
    let mean: Lab = cells.iter().fold([0.0; 3], |m, &(x, y)| {
        let v = at(x, y);
        std::array::from_fn(|i| m[i] + v[i] / n)
    });
    let chroma = cells
        .iter()
        .map(|&(x, y)| {
            let v = at(x, y);
            v[1].hypot(v[2]) * 100.0
        })
        .sum::<f32>()
        / n;
    let inner: Vec<(usize, usize)> = cells
        .iter()
        .copied()
        .filter(|&(x, y)| x > 0 && y > 0 && x + 1 < img.w && y + 1 < img.h)
        .collect();
    let l = |x: usize, y: usize| at(x, y)[0] * 100.0;
    let texture = inner
        .iter()
        .map(|&(x, y)| (4.0 * l(x, y) - l(x - 1, y) - l(x + 1, y) - l(x, y - 1) - l(x, y + 1)).abs())
        .sum::<f32>()
        / inner.len().max(1) as f32;
    let mut edges: Vec<f32> = inner
        .iter()
        .map(|&(x, y)| (l(x + 1, y) - l(x - 1, y)).hypot(l(x, y + 1) - l(x, y - 1)) * 0.5)
        .collect();
    edges.sort_by(f32::total_cmp);
    Stats {
        value: [
            percentile(&values, 0.05),
            percentile(&values, 0.5),
            percentile(&values, 0.95),
        ],
        chroma,
        texture,
        sharpness: percentile(&edges, 0.99),
        mean,
    }
}

fn ratio(a: f32, b: f32) -> f32 {
    a / b.max(1e-3)
}

pub fn words(yours: &Stats, reference: &Stats) -> Vec<String> {
    let mut out = Vec::new();
    let dl = (yours.mean[0] - reference.mean[0]) * 100.0;
    if dl.abs() > 2.0 {
        out.push(format!(
            "{} by {:.0} L",
            if dl > 0.0 { "lighter" } else { "darker" },
            dl.abs()
        ));
    }
    let (da, db) = (
        (yours.mean[1] - reference.mean[1]) * 100.0,
        (yours.mean[2] - reference.mean[2]) * 100.0,
    );
    if db.abs() > 1.5 {
        out.push(if db > 0.0 { "yellower" } else { "bluer" }.to_string());
    }
    if da.abs() > 1.5 {
        out.push(if da > 0.0 { "redder" } else { "greener" }.to_string());
    }
    let pct = |r: f32| ((r - 1.0).abs() * 100.0).round();
    let c = ratio(yours.chroma, reference.chroma);
    if (c - 1.0).abs() > 0.1 && (yours.chroma - reference.chroma).abs() > 1.0 {
        out.push(format!(
            "{:.0}% {} saturated",
            pct(c),
            if c > 1.0 { "more" } else { "less" }
        ));
    }
    let t = ratio(yours.texture, reference.texture);
    if (t - 1.0).abs() > 0.2 && (yours.texture - reference.texture).abs() > 0.5 {
        out.push(format!(
            "{:.0}% {}",
            pct(t),
            if t > 1.0 { "busier" } else { "smoother" }
        ));
    }
    let s = ratio(yours.sharpness, reference.sharpness);
    if (s - 1.0).abs() > 0.15 && (yours.sharpness - reference.sharpness).abs() > 1.0 {
        out.push(format!(
            "{:.0}% {} edges",
            pct(s),
            if s > 1.0 { "sharper" } else { "softer" }
        ));
    }
    if out.is_empty() {
        out.push("about the same".into());
    }
    out
}

fn difference(a: Lab, b: Lab) -> f32 {
    100.0 * ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

pub fn mean_difference(a: &Image, b: &Image) -> f32 {
    a.lab.iter().zip(&b.lab).map(|(x, y)| difference(*x, *y)).sum::<f32>() / a.lab.len().max(1) as f32
}

fn heat(a: &Image, b: &Image) -> klad_render::assets::Rgba {
    let pixels = a
        .lab
        .iter()
        .zip(&b.lab)
        .flat_map(|(x, y)| {
            let t = (difference(*x, *y) / 20.0).clamp(0.0, 1.0);
            let ramp = [t * 3.0, t * 3.0 - 1.0, t * 3.0 - 2.0].map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
            [ramp[0], ramp[1], ramp[2], 255]
        })
        .collect();
    klad_render::assets::Rgba {
        width: a.w as u32,
        height: a.h as u32,
        pixels,
    }
}

fn hex(lab: Lab) -> String {
    color::to_hex(color::from_oklab(lab, 1.0))
}

fn load(path: &Path, frame: u32, size: Option<(usize, usize)>) -> Result<Image> {
    if path.is_dir() {
        let bundle = Bundle::open(path)?;
        let scale = size.map_or(1.0, |(w, _)| w as f32 / bundle.doc.canvas.width.max(1) as f32);
        let out = std::env::temp_dir().join(format!("klad-compare-{}.png", std::process::id()));
        let rendered = render_look(
            &bundle,
            frame,
            View::scaled(scale),
            &Look::plain(None),
            Some(out.clone()),
            true,
        )?;
        let _ = std::fs::remove_file(&out);
        let f = rendered.frame;
        return Ok(Image::from_rgba(f.width as usize, f.height as usize, &f.rgba));
    }
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let rgba = klad_render::assets::decode(&bytes)?;
    Ok(Image::from_rgba(
        rgba.width as usize,
        rgba.height as usize,
        &rgba.pixels,
    ))
}

pub fn parse_region(text: &str) -> Result<(String, [usize; 4])> {
    let (name, rect) = text.split_once('=').context("a region is name=x,y,w,h")?;
    let n: Vec<usize> = rect.split(',').map(|v| v.trim().parse()).collect::<Result<_, _>>()?;
    match n.as_slice() {
        [x, y, w, h] if *w > 0 && *h > 0 => Ok((name.trim().to_string(), [*x, *y, *w, *h])),
        _ => bail!("region {text:?} is not name=x,y,w,h"),
    }
}

pub fn compare(
    yours: &Path,
    reference: &Path,
    frame: u32,
    regions: &[(String, [usize; 4])],
    heat_to: Option<PathBuf>,
    as_json: bool,
) -> Result<()> {
    let theirs = load(reference, 0, None)?;
    let mine = load(yours, frame, Some((theirs.w, theirs.h)))?.resized(theirs.w, theirs.h);
    let whole = [0, 0, theirs.w, theirs.h];
    let (a, b) = (stats(&mine, whole), stats(&theirs, whole));
    let mean = mean_difference(&mine, &theirs);
    let rows: Vec<(String, Stats, Stats)> = regions
        .iter()
        .map(|(name, rect)| (name.clone(), stats(&mine, *rect), stats(&theirs, *rect)))
        .collect();
    if let Some(path) = &heat_to {
        klad_core::bundle::write_atomic(path, &klad_render::assets::encode_png(&heat(&mine, &theirs))?)?;
    }
    let side = |s: &Stats| json!({"value": s.value, "chroma": s.chroma, "texture": s.texture, "sharpness": s.sharpness, "mean": hex(s.mean)});
    if as_json {
        let regions: Vec<_> = rows
            .iter()
            .map(|(n, x, y)| {
                json!({"name": n, "yours": side(x), "reference": side(y), "difference": difference(x.mean, y.mean), "words": words(x, y)})
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "size": [theirs.w, theirs.h],
                "yours": side(&a),
                "reference": side(&b),
                "difference": mean,
                "words": words(&a, &b),
                "regions": regions,
                "heat": heat_to.as_ref().map(|p| p.display().to_string()),
            }))?
        );
        return Ok(());
    }
    println!("{:<12} {:<22} {:<22}", "", "yours", "reference");
    println!(
        "{:<12} {:<22} {:<22}",
        "value L",
        format!("{:.1} / {:.1} / {:.1}", a.value[0], a.value[1], a.value[2]),
        format!("{:.1} / {:.1} / {:.1}", b.value[0], b.value[1], b.value[2])
    );
    [
        ("chroma", a.chroma, b.chroma),
        ("texture", a.texture, b.texture),
        ("sharpness", a.sharpness, b.sharpness),
    ]
    .iter()
    .for_each(|(n, x, y)| println!("{n:<12} {x:<22.1} {y:<22.1}"));
    println!("mean difference {mean:.1} (OKLab x100): {}", words(&a, &b).join(", "));
    if !rows.is_empty() {
        println!(
            "{:<14} {:<9} {:<10} {:>5}  what differs",
            "region", "yours", "reference", "dE"
        );
        rows.iter().for_each(|(n, x, y)| {
            println!(
                "{n:<14} {:<9} {:<10} {:>5.1}  {}",
                hex(x.mean),
                hex(y.mean),
                difference(x.mean, y.mean),
                words(x, y).join(", ")
            )
        });
    }
    if let Some(path) = heat_to {
        println!("{}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 3]) -> Image {
        let rgba: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let c = f(x, y);
                [c[0], c[1], c[2], 255]
            })
            .collect();
        Image::from_rgba(w, h, &rgba)
    }

    fn painting(x: usize, y: usize) -> [u8; 3] {
        let n = ((x * 7 + y * 13) % 11) as u8 * 4;
        match x < 60 {
            true => [180 + n, 120 + n / 2, 60],
            false => [90, 110 + n, 160 + n],
        }
    }

    #[test]
    fn a_picture_against_itself_a_lighter_copy_a_blurred_copy_and_a_recoloured_region() {
        let base = picture(120, 80, painting);
        let whole = [0, 0, 120, 80];
        let s = stats(&base, whole);
        assert_eq!(words(&s, &s), ["about the same"]);
        assert_eq!(mean_difference(&base, &base), 0.0);
        let lighter = picture(120, 80, |x, y| painting(x, y).map(|v| v.saturating_add(30)));
        let said = words(&stats(&lighter, whole), &s);
        assert!(said[0].starts_with("lighter by"), "{said:?}");
        let blurred = picture(120, 80, |x, y| {
            let sum = (0..5)
                .flat_map(|dy| (0..5).map(move |dx| (dx, dy)))
                .fold([0u32; 3], |acc, (dx, dy)| {
                    let c = painting((x + dx).saturating_sub(2).min(119), (y + dy).saturating_sub(2).min(79));
                    std::array::from_fn(|i| acc[i] + c[i] as u32)
                });
            sum.map(|v| (v / 25) as u8)
        });
        let b = stats(&blurred, whole);
        assert!(b.texture < s.texture * 0.5 && b.sharpness < s.sharpness, "{b:?} {s:?}");
        let said = words(&b, &s).join(", ");
        assert!(said.contains("smoother") && said.contains("softer edges"), "{said}");
        let recoloured = picture(120, 80, |x, y| if x < 60 { [90, 110, 200] } else { painting(x, y) });
        let left = [0, 0, 60, 80];
        let said = words(&stats(&recoloured, left), &stats(&base, left)).join(", ");
        assert!(said.contains("bluer"), "{said}");
        let right = [60, 0, 60, 80];
        assert_eq!(
            words(&stats(&recoloured, right), &stats(&base, right)),
            ["about the same"]
        );
        assert_eq!(
            parse_region("shade=10,20,30,40").unwrap(),
            ("shade".to_string(), [10, 20, 30, 40])
        );
        assert!(parse_region("shade=10,20").is_err());
    }
}
