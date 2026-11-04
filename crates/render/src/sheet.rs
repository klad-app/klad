use crate::overlay::{draw_text, text_width};
use crate::{Frame, f16_info, read_rgba8};
use anyhow::{Result, anyhow, bail};
use klad_core::doc::{Document, Layer};
use klad_core::timing::{Cell, cells, sheet_layers};
use skia_safe::{
    AlphaType, Color4f, ColorSpace, ColorType, Data, ImageInfo, Paint, PaintStyle, Rect, images, surfaces,
};

const MARGIN: f32 = 16.0;
const GAP: f32 = 12.0;
const TITLE_PX: f32 = 2.0;
const LINE: f32 = 10.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    pub from: u32,
    pub to: u32,
    pub hold: bool,
    pub tokens: Vec<String>,
}

impl Spot {
    pub fn title(&self) -> String {
        let range = match self.from == self.to {
            true => self.from.to_string(),
            false => format!("{}-{}", self.from, self.to),
        };
        match self.hold {
            true => format!("{range} hold"),
            false => range,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub layers: Vec<String>,
    pub spots: Vec<Spot>,
}

fn token(cell: &Cell) -> String {
    match cell {
        Cell::Drawing { cel, start: true } => cel.clone(),
        Cell::Drawing { cel, start: false } => format!("|{cel}"),
        Cell::Link {
            cel,
            source,
            start: true,
        } => format!("{cel}>{source}"),
        Cell::Link { cel, source, .. } => format!("|{cel}>{source}"),
        Cell::Blank => ".".into(),
    }
}

pub fn range(doc: &Document, spec: &str) -> Result<(u32, u32)> {
    let last = doc.timeline.frames.saturating_sub(1);
    if spec == "all" {
        return Ok((0, last));
    }
    if let Some(clip) = doc.timeline.clips.iter().find(|c| c.name == spec) {
        return Ok((clip.from, clip.to.min(last)));
    }
    let parsed: Option<(u32, u32)> = match spec.split_once('-') {
        Some((a, b)) => a.trim().parse().ok().zip(b.trim().parse().ok()),
        None => spec.trim().parse().ok().map(|f| (f, f)),
    };
    let Some((from, to)) = parsed else {
        let clips: Vec<&str> = doc.timeline.clips.iter().map(|c| c.name.as_str()).collect();
        bail!(
            "--sheet takes all, a range like 0-11 or a clip name{}",
            match clips.is_empty() {
                true => " (this document has no clips)".to_string(),
                false => format!(" ({})", clips.join(", ")),
            }
        );
    };
    if from > to || to > last {
        bail!("frames {from}-{to} are not inside the timeline (0-{last})");
    }
    Ok((from, to))
}

fn shown(layers: &[&Layer], frame: u32) -> Vec<Option<String>> {
    layers
        .iter()
        .map(|l| l.content_at(frame).map(|c| c.id.clone()))
        .collect()
}

pub fn plan(doc: &Document, from: u32, to: u32, drawings: bool) -> Plan {
    let layers: Vec<&Layer> = sheet_layers(doc).into_iter().filter(|l| !l.cels().is_empty()).collect();
    let grid: Vec<Vec<Cell>> = layers.iter().map(|l| cells(l, to + 1)).collect();
    let holds = |f: u32| f > 0 && shown(&layers, f) == shown(&layers, f - 1);
    let tokens = |f: u32| grid.iter().map(|c| token(&c[f as usize])).collect::<Vec<_>>();
    let spots = match drawings {
        false => (from..=to)
            .map(|f| Spot {
                from: f,
                to: f,
                hold: holds(f),
                tokens: tokens(f),
            })
            .collect(),
        true => (from..=to).fold(Vec::<Spot>::new(), |mut out, f| {
            match out.last_mut() {
                Some(spot) if holds(f) => spot.to = f,
                _ => out.push(Spot {
                    from: f,
                    to: f,
                    hold: false,
                    tokens: tokens(f),
                }),
            }
            out
        }),
    };
    Plan {
        layers: layers.iter().map(|l| format!("{} {}", l.id, l.name)).collect(),
        spots,
    }
}

fn wrap(tokens: &[String], chars: usize) -> Vec<String> {
    tokens.iter().fold(Vec::<String>::new(), |mut lines, t| {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 2 + t.chars().count() <= chars => {
                line.push_str("  ");
                line.push_str(t);
            }
            _ => lines.push(t.clone()),
        }
        lines
    })
}

fn paint(rgb: [f32; 3]) -> Paint {
    let mut p = Paint::new(Color4f::new(rgb[0], rgb[1], rgb[2], 1.0), &ColorSpace::new_srgb());
    p.set_anti_alias(false);
    p
}

pub fn columns_for(count: usize) -> usize {
    ((count as f64).sqrt().ceil() as usize).max(1)
}

pub fn draw(plan: &Plan, images: &[&Frame], columns: usize, heading: &str) -> Result<Frame> {
    let first = images
        .first()
        .ok_or_else(|| anyhow!("a sheet needs at least one frame"))?;
    if images.len() != plan.spots.len() {
        bail!("{} images for {} sheet cells", images.len(), plan.spots.len());
    }
    let (iw, ih) = (first.width as f32, first.height as f32);
    let columns = columns.clamp(1, images.len());
    let rows = images.len().div_ceil(columns);
    let chars = (((iw - 4.0) / 6.0).floor() as usize).max(4);
    let lines: Vec<Vec<String>> = plan.spots.iter().map(|s| wrap(&s.tokens, chars)).collect();
    let label = 6.0 + 7.0 * TITLE_PX + 4.0 + lines.iter().map(Vec::len).max().unwrap_or(0) as f32 * LINE;
    let legend = [
        format!("layers, top first: {}", plan.layers.join(", ")),
        "cN starts a drawing   |cN holds it   cA>cB shows cB again (a link)   . blank   hold: same picture as the frame before"
            .to_string(),
    ];
    let header = 7.0 * TITLE_PX + 8.0 + legend.len() as f32 * LINE + 8.0;
    let width = (MARGIN * 2.0 + columns as f32 * iw + (columns - 1) as f32 * GAP)
        .max(MARGIN * 2.0 + legend.iter().map(|l| text_width(l, 1.0)).fold(0.0, f32::max))
        .ceil();
    let height = (MARGIN * 2.0 + header + rows as f32 * (ih + label) + (rows - 1) as f32 * GAP).ceil();
    let mut surface = surfaces::raster(&f16_info(width as u32, height as u32), None, None)
        .ok_or_else(|| anyhow!("cannot allocate a {width}x{height} sheet"))?;
    let canvas = surface.canvas();
    canvas.clear(Color4f::new(0.12, 0.13, 0.15, 1.0));
    draw_text(canvas, heading, MARGIN, MARGIN, TITLE_PX, [1.0, 1.0, 1.0]);
    legend.iter().enumerate().for_each(|(i, line)| {
        let y = MARGIN + 7.0 * TITLE_PX + 8.0 + i as f32 * LINE;
        draw_text(canvas, line, MARGIN, y, 1.0, [0.7, 0.72, 0.76]);
    });
    let mut border = paint([0.32, 0.34, 0.38]);
    border.set_style(PaintStyle::Stroke);
    border.set_stroke_width(1.0);
    images
        .iter()
        .zip(&plan.spots)
        .zip(&lines)
        .enumerate()
        .try_for_each(|(i, ((frame, spot), lines))| {
            let x = MARGIN + (i % columns) as f32 * (iw + GAP);
            let y = MARGIN + header + (i / columns) as f32 * (ih + label + GAP);
            let info = ImageInfo::new(
                (frame.width as i32, frame.height as i32),
                ColorType::RGBA8888,
                AlphaType::Unpremul,
                ColorSpace::new_srgb(),
            );
            let image = images::raster_from_data(&info, Data::new_copy(&frame.rgba), frame.width as usize * 4)
                .ok_or_else(|| anyhow!("cannot wrap frame {}", spot.from))?;
            canvas.draw_image(&image, (x, y), None);
            canvas.draw_rect(Rect::from_xywh(x - 0.5, y - 0.5, iw + 1.0, ih + 1.0), &border);
            let title = match spot.hold {
                true => [0.55, 0.57, 0.6],
                false => [1.0, 1.0, 1.0],
            };
            draw_text(canvas, &spot.title(), x + 2.0, y + ih + 6.0, TITLE_PX, title);
            lines.iter().enumerate().for_each(|(k, line)| {
                let ty = y + ih + 6.0 + 7.0 * TITLE_PX + 4.0 + k as f32 * LINE;
                draw_text(canvas, line, x + 2.0, ty, 1.0, [0.78, 0.8, 0.84]);
            });
            Ok::<_, anyhow::Error>(())
        })?;
    read_rgba8(&surface.image_snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::{Cel, Clip};

    fn doc() -> Document {
        let mut doc = Document::new(64, 32);
        doc.timeline.frames = 8;
        doc.timeline.clips.push(Clip::new("run", 2, 5, true));
        let mut ball = Layer::paint("l1", "ball", "c1");
        *ball.cels_mut().unwrap() = vec![
            Cel {
                hold: Some(2),
                ..Cel::at("c1", 0)
            },
            Cel {
                hold: Some(1),
                ..Cel::at("c2", 2)
            },
            Cel::at("c3", 3),
            Cel {
                link: Some("c1".into()),
                ..Cel::at("c4", 6)
            },
        ];
        let floor = Layer::paint("l2", "floor", "c9");
        doc.layers = vec![floor, ball];
        doc
    }

    #[test]
    fn frames_get_tokens_and_hold_marks() {
        let doc = doc();
        let plan = plan(&doc, 0, 7, false);
        assert_eq!(plan.layers, ["l1 ball", "l2 floor"]);
        let row = |f: usize| (plan.spots[f].title(), plan.spots[f].tokens.join(" "));
        assert_eq!(row(0), ("0".into(), "c1 c9".into()));
        assert_eq!(row(1), ("1 hold".into(), "|c1 |c9".into()));
        assert_eq!(row(2), ("2".into(), "c2 |c9".into()));
        assert_eq!(row(4), ("4 hold".into(), "|c3 |c9".into()));
        assert_eq!(row(6), ("6".into(), "c4>c1 |c9".into()));
        assert_eq!(row(7), ("7 hold".into(), "|c4>c1 |c9".into()));
    }

    #[test]
    fn drawings_merge_frames_that_look_the_same() {
        let doc = doc();
        let plan = plan(&doc, 0, 7, true);
        let titles: Vec<String> = plan.spots.iter().map(Spot::title).collect();
        assert_eq!(titles, ["0-1", "2", "3-5", "6-7"]);
        assert_eq!(plan.spots[3].tokens, ["c4>c1", "|c9"]);
    }

    #[test]
    fn ranges_take_all_spans_frames_and_clips() {
        let doc = doc();
        assert_eq!(range(&doc, "all").unwrap(), (0, 7));
        assert_eq!(range(&doc, "2-4").unwrap(), (2, 4));
        assert_eq!(range(&doc, "5").unwrap(), (5, 5));
        assert_eq!(range(&doc, "run").unwrap(), (2, 5));
        assert!(range(&doc, "3-9").is_err());
        assert!(range(&doc, "walk").unwrap_err().to_string().contains("run"));
    }

    #[test]
    fn a_sheet_lays_cells_out_in_a_grid() {
        let doc = doc();
        let plan = plan(&doc, 0, 4, false);
        let frame = Frame {
            width: 40,
            height: 20,
            rgba: [200u8, 40, 40, 255].repeat(800),
        };
        let images: Vec<&Frame> = plan.spots.iter().map(|_| &frame).collect();
        let sheet = draw(&plan, &images, columns_for(images.len()), "test").unwrap();
        assert_eq!(columns_for(5), 3);
        let header = 7.0 * TITLE_PX + 8.0 + 2.0 * LINE + 8.0;
        let (x, y) = (MARGIN as u32 + 41 + GAP as u32 + 5, (MARGIN + header) as u32 + 5);
        assert_eq!(sheet.pixel(x, y), [200, 40, 40, 255]);
        assert!(sheet.height > 2 * 20 + header as u32);
        assert!(draw(&plan, &images[..2], 2, "x").is_err());
    }
}
