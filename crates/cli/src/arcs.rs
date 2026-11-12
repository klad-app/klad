use anyhow::{Result, anyhow, bail};
use klad_core::bundle::Bundle;
use klad_core::doc::{Cel, Document, Kind, Layer, Loc};
use klad_core::fill::FillSource;
use klad_core::markedit::{bounds_of, parse_address};
use klad_core::stroke::{Mark, Sample};
use klad_core::tween;
use klad_render::overlay;
use serde_json::json;
use std::collections::BTreeMap;

pub const SLACK: f32 = 2.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Track {
    Mark { cel: String, id: u32 },
    Part { layer: Option<String>, name: String },
    Point { x: f32, y: f32 },
}

impl Track {
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim();
        if let Some((cel, id)) = parse_address(text) {
            return Ok(Track::Mark {
                cel: cel.to_string(),
                id,
            });
        }
        if let Some((x, y)) = text.split_once(',') {
            let (x, y) = (x.trim().parse::<f32>(), y.trim().parse::<f32>());
            return match (x, y) {
                (Ok(x), Ok(y)) => Ok(Track::Point { x, y }),
                _ => bail!("{text:?} is not a point x,y"),
            };
        }
        if text.is_empty() {
            bail!("a track is a mark (c4:s12), a part (leaf, or lines:leaf) or a point (x,y)");
        }
        Ok(match text.split_once(':') {
            Some((layer, name)) => Track::Part {
                layer: Some(layer.to_string()),
                name: name.to_string(),
            },
            None => Track::Part {
                layer: None,
                name: text.to_string(),
            },
        })
    }

    pub fn label(&self) -> String {
        match self {
            Track::Mark { cel, id } => format!("{cel}:s{id}"),
            Track::Part { layer: Some(l), name } => format!("{l}:{name}"),
            Track::Part { layer: None, name } => name.clone(),
            Track::Point { x, y } => format!("{x:.0},{y:.0}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub centre: [f32; 2],
    pub bottom: [f32; 2],
    pub sole: [f32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub label: String,
    pub spots: Vec<(u32, Option<Spot>)>,
}

#[derive(Default)]
struct Cache {
    marks: BTreeMap<(String, String), Vec<Mark>>,
}

impl Cache {
    fn marks(&mut self, bundle: &Bundle, layer: &str, cel: &str) -> Result<&Vec<Mark>> {
        let key = (layer.to_string(), cel.to_string());
        if !self.marks.contains_key(&key) {
            let read = bundle.read_marks(layer, cel)?;
            self.marks.insert(key.clone(), read);
        }
        Ok(&self.marks[&key])
    }
}

fn points(mark: &Mark) -> Vec<[f32; 2]> {
    match mark {
        Mark::Stroke(s) => s.samples.iter().map(Sample::pos).map(|(x, y)| [x, y]).collect(),
        Mark::Fill(f) => match &f.source {
            FillSource::Polygon(p) => p.clone(),
            FillSource::Seed { .. } | FillSource::Cells { .. } => f
                .bounds()
                .map(|[x0, y0, x1, y1]| vec![[x0, y0], [x1, y1]])
                .unwrap_or_default(),
        },
    }
}

fn spot_of(marks: &[Mark], ids: &[u32]) -> Option<Spot> {
    let [x0, y0, x1, y1] = bounds_of(marks, ids)?;
    let all: Vec<[f32; 2]> = marks
        .iter()
        .filter(|m| ids.contains(&m.id()))
        .flat_map(points)
        .collect();
    let low = all.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
    let sole: Vec<f32> = all.iter().filter(|p| p[1] >= low - 1.0).map(|p| p[0]).collect();
    let contact: Vec<f32> = all.iter().filter(|p| p[1] >= low - SLACK).map(|p| p[0]).collect();
    (!sole.is_empty()).then(|| Spot {
        centre: [(x0 + x1) / 2.0, (y0 + y1) / 2.0],
        bottom: [sole.iter().sum::<f32>() / sole.len() as f32, low],
        sole: [
            contact.iter().copied().fold(f32::MAX, f32::min),
            contact.iter().copied().fold(f32::MIN, f32::max),
        ],
    })
}

fn shown(layer: &Layer, frame: u32) -> Option<&Cel> {
    let cel = layer.cel_at(frame)?;
    match &cel.link {
        Some(source) => layer.cels().iter().find(|c| &c.id == source),
        None => Some(cel),
    }
}

fn paint_layers(doc: &Document) -> Vec<&Layer> {
    let mut all: Vec<&Layer> = doc
        .all_layers()
        .into_iter()
        .filter(|l| l.kind() == Kind::Paint && l.visible)
        .collect();
    all.reverse();
    all
}

fn owner_of<'a>(doc: &'a Document, cel: &str) -> Result<&'a Layer> {
    match doc.locate(cel) {
        Some(Loc::Cel(path, _)) => Ok(doc.layer_at(&path)),
        _ => Err(anyhow!("no cel {cel}")),
    }
}

fn nearest_mark(bundle: &Bundle, cache: &mut Cache, frame: u32, at: [f32; 2]) -> Result<Option<(String, u32)>> {
    let mut best: Option<(f32, String, u32)> = None;
    paint_layers(&bundle.doc).into_iter().try_for_each(|layer| {
        let Some(cel) = shown(layer, frame) else {
            return Ok::<_, anyhow::Error>(());
        };
        cache
            .marks(bundle, &layer.id, &cel.id)?
            .iter()
            .filter_map(Mark::as_stroke)
            .for_each(|s| {
                let d = s
                    .samples
                    .iter()
                    .map(|q| {
                        let (x, y) = q.pos();
                        (x - at[0]).hypot(y - at[1])
                    })
                    .fold(f32::MAX, f32::min);
                if best.as_ref().is_none_or(|b| d < b.0) {
                    best = Some((d, layer.id.clone(), s.id));
                }
            });
        Ok(())
    })?;
    Ok(best.map(|(_, layer, id)| (layer, id)))
}

pub fn track(bundle: &Bundle, tracks: &[Track], (from, to): (u32, u32)) -> Result<Vec<Path>> {
    let doc = &bundle.doc;
    let mut cache = Cache::default();
    tracks
        .iter()
        .map(|t| {
            let spots = match t {
                Track::Mark { cel, id } => {
                    let layer = owner_of(doc, cel)?;
                    (from..=to)
                        .map(|f| {
                            let spot = match shown(layer, f) {
                                Some(c) => spot_of(cache.marks(bundle, &layer.id, &c.id)?, &[*id]),
                                None => None,
                            };
                            Ok((f, spot))
                        })
                        .collect::<Result<Vec<_>>>()?
                }
                Track::Part { layer, name } => {
                    let candidates: Vec<&Layer> = match layer {
                        Some(key) => vec![doc.find_layer(key)?],
                        None => paint_layers(doc),
                    };
                    (from..=to)
                        .map(|f| {
                            let found = candidates
                                .iter()
                                .find_map(|l| shown(l, f).and_then(|c| c.parts.get(name).map(|ids| (l, c, ids))));
                            let spot = match found {
                                Some((l, c, ids)) => spot_of(cache.marks(bundle, &l.id, &c.id)?, ids),
                                None => None,
                            };
                            Ok((f, spot))
                        })
                        .collect::<Result<Vec<_>>>()?
                }
                Track::Point { x, y } => {
                    let Some((layer_id, id)) = nearest_mark(bundle, &mut cache, from, [*x, *y])? else {
                        bail!("frame {from} shows no strokes to carry the point {x},{y}");
                    };
                    let layer = doc.layer(&layer_id).ok_or_else(|| anyhow!("no layer {layer_id}"))?;
                    let line_at = |cache: &mut Cache, f: u32| -> Result<Option<Vec<(f32, f32)>>> {
                        Ok(match shown(layer, f) {
                            Some(c) => cache
                                .marks(bundle, &layer.id, &c.id)?
                                .iter()
                                .filter_map(Mark::as_stroke)
                                .find(|s| s.id == id)
                                .map(tween::line_of),
                            None => None,
                        })
                    };
                    let first = line_at(&mut cache, from)?.unwrap_or_default();
                    let anchor = tween::anchor(&[first], (*x, *y));
                    (from..=to)
                        .map(|f| {
                            let line = line_at(&mut cache, f)?;
                            let at =
                                anchor
                                    .as_ref()
                                    .zip(line)
                                    .and_then(|(a, l)| tween::place(a, &[l]))
                                    .map(|(x, y)| Spot {
                                        centre: [x, y],
                                        bottom: [x, y],
                                        sole: [x, x],
                                    });
                            Ok((f, at))
                        })
                        .collect::<Result<Vec<_>>>()?
                }
            };
            Ok(Path {
                label: t.label(),
                spots,
            })
        })
        .collect()
}

fn moved(path: &Path) -> Vec<(u32, f32)> {
    path.spots
        .windows(2)
        .filter_map(|w| match (w[0].1, w[1].1) {
            (Some(a), Some(b)) => Some((w[1].0, (b.centre[0] - a.centre[0]).hypot(b.centre[1] - a.centre[1]))),
            _ => None,
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ground {
    pub under: Vec<(u32, f32)>,
    pub slides: Vec<(u32, u32, f32)>,
    pub touches: Vec<u32>,
}

pub fn ground(path: &Path, y: f32, travel: f32, slack: f32) -> Ground {
    let touching = |s: &Spot| (s.bottom[1] - y).abs() <= slack;
    let changes: Vec<(u32, Spot)> = path.spots.iter().filter_map(|(f, s)| s.map(|s| (*f, s))).fold(
        Vec::new(),
        |mut out: Vec<(u32, Spot)>, (f, s)| {
            if out
                .last()
                .is_none_or(|(_, last)| last.bottom != s.bottom || last.centre != s.centre || last.sole != s.sole)
            {
                out.push((f, s));
            }
            out
        },
    );
    Ground {
        under: path
            .spots
            .iter()
            .filter_map(|(f, s)| s.filter(|s| s.bottom[1] > y + slack).map(|s| (*f, s.bottom[1] - y)))
            .collect(),
        touches: path
            .spots
            .iter()
            .filter(|(_, s)| s.as_ref().is_some_and(touching))
            .map(|(f, _)| *f)
            .collect(),
        slides: changes
            .windows(2)
            .filter_map(|w| {
                let ((fa, a), (fb, b)) = (w[0], w[1]);
                let expected = -travel * (fb - fa) as f32;
                let end = |i: usize| (b.sole[i] - a.sole[i] - expected).abs();
                let off = end(0).min(end(1)).min((b.bottom[0] - a.bottom[0] - expected).abs());
                (touching(&a) && touching(&b) && off > slack).then_some((fa, fb, off))
            })
            .collect(),
    }
}

pub fn swaps(paths: &[Path]) -> Vec<(usize, usize, u32, u32)> {
    (0..paths.len())
        .flat_map(|i| (i + 1..paths.len()).map(move |j| (i, j)))
        .flat_map(|(i, j)| {
            let side: Vec<(u32, f32)> = paths[i]
                .spots
                .iter()
                .zip(&paths[j].spots)
                .filter_map(|((f, a), (_, b))| match (a, b) {
                    (Some(a), Some(b)) if (a.bottom[0] - b.bottom[0]).abs() > SLACK => {
                        Some((*f, (a.bottom[0] - b.bottom[0]).signum()))
                    }
                    _ => None,
                })
                .collect();
            side.windows(2)
                .filter(|w| w[0].1 != w[1].1)
                .map(|w| (i, j, w[0].0, w[1].0))
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn range(doc: &Document, frame: u32, from: Option<u32>, to: Option<u32>) -> (u32, u32) {
    let whole = doc
        .timeline
        .clip_at(frame)
        .map_or((0, doc.timeline.frames.saturating_sub(1)), |c| (c.from, c.to));
    (
        from.unwrap_or(whole.0),
        to.unwrap_or(whole.1).min(doc.timeline.frames.saturating_sub(1)),
    )
}

pub fn arcs_overlay(paths: &[Path]) -> Vec<overlay::Arc> {
    paths
        .iter()
        .map(|p| overlay::Arc {
            label: p.label.clone(),
            points: p.spots.iter().filter_map(|(f, s)| s.map(|s| (*f, s.centre))).collect(),
        })
        .collect()
}

pub fn part_marks(bundle: &Bundle, frame: u32) -> Result<Vec<overlay::Mark>> {
    let mut cache = Cache::default();
    let mut out = Vec::new();
    paint_layers(&bundle.doc).into_iter().try_for_each(|layer| {
        let Some(cel) = shown(layer, frame) else {
            return Ok::<_, anyhow::Error>(());
        };
        let marks = cache.marks(bundle, &layer.id, &cel.id)?;
        cel.parts.iter().for_each(|(name, ids)| {
            if let Some([x0, y0, x1, y1]) = bounds_of(marks, ids) {
                out.push(overlay::Mark {
                    label: format!("{name} ({})", layer.name),
                    rect: [x0, y0, x1 - x0, y1 - y0],
                });
            }
        });
        Ok(())
    })?;
    Ok(out)
}

pub struct Checks {
    pub ground: Vec<f32>,
    pub travel: f32,
    pub slack: f32,
    pub json: bool,
    pub check: bool,
}

pub fn report(bundle: &Bundle, tracks: &[Track], span: (u32, u32), checks: &Checks) -> Result<()> {
    let paths = track(bundle, tracks, span)?;
    let lines: Vec<Option<f32>> = match checks.ground.len() {
        0 => vec![None; paths.len()],
        1 => vec![Some(checks.ground[0]); paths.len()],
        n if n == paths.len() => checks.ground.iter().copied().map(Some).collect(),
        n => bail!(
            "{n} ground lines for {} tracks: give one for all, or one per track",
            paths.len()
        ),
    };
    let grounds: Vec<Option<Ground>> = paths
        .iter()
        .zip(&lines)
        .map(|(p, y)| y.map(|y| ground(p, y, checks.travel, checks.slack)))
        .collect();
    let crossed = swaps(&paths);
    let wrong: Vec<String> = grounds
        .iter()
        .zip(&paths)
        .flat_map(|(g, p)| {
            g.iter().flat_map(move |g| {
                g.under
                    .iter()
                    .map(move |(f, d)| format!("{} under the ground at {f} by {d:.1} px", p.label))
                    .chain(
                        g.slides
                            .iter()
                            .map(move |(a, b, d)| format!("{} slides {d:.1} px from {a} to {b}", p.label)),
                    )
            })
        })
        .chain(crossed.iter().map(|(i, j, a, b)| {
            format!(
                "{} and {} swap sides between {a} and {b}",
                paths[*i].label, paths[*j].label
            )
        }))
        .collect();
    let as_json = checks.json;
    if as_json {
        let tracks: Vec<serde_json::Value> = paths
            .iter()
            .zip(&grounds)
            .map(|(p, g)| {
                let steps: BTreeMap<u32, f32> = moved(p).into_iter().collect();
                json!({
                    "track": p.label,
                    "frames": p.spots.iter().map(|(f, s)| json!({
                        "frame": f,
                        "at": s.map(|s| s.centre),
                        "bottom": s.map(|s| s.bottom),
                        "sole": s.map(|s| s.sole),
                        "moved": steps.get(f),
                    })).collect::<Vec<_>>(),
                    "under": g.as_ref().map(|g| g.under.iter().map(|(f, d)| json!({"frame": f, "by": d})).collect::<Vec<_>>()),
                    "slides": g.as_ref().map(|g| g.slides.iter().map(|(a, b, d)| json!({"from": a, "to": b, "by": d})).collect::<Vec<_>>()),
                    "touches": g.as_ref().map(|g| g.touches.clone()),
                })
            })
            .collect();
        let swaps: Vec<serde_json::Value> = crossed
            .iter()
            .map(|(i, j, a, b)| json!({"tracks": [paths[*i].label, paths[*j].label], "between": [a, b]}))
            .collect();
        println!(
            "{}",
            json!({"from": span.0, "to": span.1, "ground": checks.ground, "travel": checks.travel, "tracks": tracks, "swaps": swaps, "wrong": wrong})
        );
    } else {
        print_paths(&paths, &grounds, &crossed);
    }
    if checks.check && !wrong.is_empty() {
        bail!("the arcs show {}: {}", wrong.len(), wrong.join("; "));
    }
    Ok(())
}

fn print_paths(paths: &[Path], grounds: &[Option<Ground>], crossed: &[(usize, usize, u32, u32)]) {
    paths.iter().zip(grounds).for_each(|(p, g)| {
        println!("{}", p.label);
        let steps: BTreeMap<u32, f32> = moved(p).into_iter().collect();
        p.spots.iter().for_each(|(f, s)| match s {
            Some(s) => println!(
                "  {f:>4}  {:>7.1},{:>7.1}  moved {:>6}  bottom {:.0},{:.0}",
                s.centre[0],
                s.centre[1],
                steps.get(f).map_or("-".to_string(), |d| format!("{d:.1}")),
                s.bottom[0],
                s.bottom[1]
            ),
            None => println!("  {f:>4}  not shown"),
        });
        if let Some(g) = g {
            let frames = |list: Vec<String>| {
                if list.is_empty() {
                    "none".to_string()
                } else {
                    list.join(", ")
                }
            };
            println!(
                "  under the ground: {}",
                frames(g.under.iter().map(|(f, d)| format!("{f} ({d:.1} px)")).collect())
            );
            println!(
                "  slides on the ground: {}",
                frames(
                    g.slides
                        .iter()
                        .map(|(a, b, d)| format!("{a} to {b} ({d:.1} px)"))
                        .collect()
                )
            );
            println!(
                "  on the ground: {}",
                frames(g.touches.iter().map(u32::to_string).collect())
            );
        }
    });
    crossed.iter().for_each(|(i, j, a, b)| {
        println!(
            "{} and {} swap sides between frames {a} and {b}",
            paths[*i].label, paths[*j].label
        );
    });
    if crossed.is_empty() && paths.len() > 1 {
        println!("no tracks swap sides");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(label: &str, at: &[(f32, f32)]) -> Path {
        Path {
            label: label.into(),
            spots: at
                .iter()
                .enumerate()
                .map(|(f, &(x, y))| {
                    (
                        f as u32,
                        Some(Spot {
                            centre: [x, y - 10.0],
                            bottom: [x, y],
                            sole: [x, x],
                        }),
                    )
                })
                .collect(),
        }
    }

    fn walk(name: &str) -> (std::path::PathBuf, Bundle) {
        use klad_core::doc::{Document, Layer};
        let dir = std::env::temp_dir().join(format!("draw-arcs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(400, 300);
        doc.timeline.frames = 4;
        let mut lines = Layer::paint("lines", "lines", "k0");
        lines.cels_mut().unwrap()[0].hold = Some(1);
        doc.layers = vec![lines];
        doc.next_id = 10;
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let foot = |x: f32, bottom: f32, frame: u32, part: &str| {
            format!(
                r#"{{"op":"stroke","layer":"lines","frame":{frame},"size":4,"part":"{part}","path":"M{x} {} L{x} {bottom} L{} {bottom}"}}"#,
                bottom - 60.0,
                x + 20.0
            )
        };
        let near = [(100.0, 250.0), (140.0, 250.0), (180.0, 256.0), (220.0, 250.0)];
        let far = [(200.0, 250.0), (170.0, 250.0), (140.0, 230.0), (110.0, 230.0)];
        let text: Vec<String> = (0..4u32)
            .flat_map(|f| {
                let new = (f > 0).then(|| format!(r#"{{"op":"cel","new":"lines","frame":{f},"hold":1,"id":"k{f}"}}"#));
                new.into_iter().chain([
                    foot(near[f as usize].0, near[f as usize].1, f, "near foot"),
                    foot(far[f as usize].0, far[f as usize].1, f, "far foot"),
                ])
            })
            .collect();
        bundle
            .apply(&klad_core::ops::parse_lines(&text.join("\n")).unwrap())
            .unwrap();
        (dir, bundle)
    }

    #[test]
    fn a_walk_whose_feet_cross_is_caught_with_its_contacts_and_slides() {
        let (dir, bundle) = walk("cross");
        let tracks = [Track::parse("near foot").unwrap(), Track::parse("far foot").unwrap()];
        let paths = track(&bundle, &tracks, (0, 3)).unwrap();
        let sole = paths[0].spots[1].1.unwrap().bottom;
        assert!(
            (sole[0] - 150.0).abs() < 2.0 && (sole[1] - 250.0).abs() < 0.5,
            "{sole:?}"
        );
        assert_eq!(swaps(&paths), [(0, 1, 1, 2)]);
        let near = ground(&paths[0], 250.0, 0.0, SLACK);
        assert_eq!(near.under.iter().map(|u| u.0).collect::<Vec<_>>(), [2]);
        assert_eq!(near.touches, [0, 1, 3]);
        assert_eq!(near.slides.iter().map(|s| (s.0, s.1)).collect::<Vec<_>>(), [(0, 1)]);
        let far = ground(&paths[1], 250.0, 0.0, SLACK);
        assert_eq!(far.touches, [0, 1]);
        assert!(far.under.is_empty());
        let mark = track(&bundle, &[Track::parse("k0:s1").unwrap()], (0, 3)).unwrap();
        assert!(mark[0].spots.iter().all(|(_, s)| s.is_some()));
        let point = track(&bundle, &[Track::parse("100,220").unwrap()], (0, 3)).unwrap();
        let at = |f: usize| point[0].spots[f].1.unwrap().centre;
        assert!(
            (at(0)[0] - 100.0).abs() < 1.0 && (at(0)[1] - 220.0).abs() < 1.0,
            "{:?}",
            at(0)
        );
        assert!(
            (at(1)[0] - 140.0).abs() < 1.0 && (at(1)[1] - 220.0).abs() < 1.0,
            "{:?}",
            at(1)
        );
        let parts = part_marks(&bundle, 2).unwrap();
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().any(|m| m.label.starts_with("near foot")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tracks_parse_as_marks_parts_and_points() {
        assert_eq!(
            Track::parse("c4:s12").unwrap(),
            Track::Mark {
                cel: "c4".into(),
                id: 12
            }
        );
        assert_eq!(Track::parse("320,410").unwrap(), Track::Point { x: 320.0, y: 410.0 });
        assert_eq!(
            Track::parse("lines:near leg").unwrap(),
            Track::Part {
                layer: Some("lines".into()),
                name: "near leg".into()
            }
        );
        assert_eq!(
            Track::parse("leaf").unwrap(),
            Track::Part {
                layer: None,
                name: "leaf".into()
            }
        );
        assert!(Track::parse("3,x").is_err());
    }

    #[test]
    fn the_ground_check_finds_feet_under_it_and_sliding_on_it() {
        let foot = path(
            "foot",
            &[
                (100.0, 600.0),
                (104.0, 600.0),
                (104.5, 601.0),
                (110.0, 590.0),
                (112.0, 607.0),
            ],
        );
        let g = ground(&foot, 600.0, 0.0, SLACK);
        assert_eq!(g.touches, [0, 1, 2]);
        assert_eq!(g.slides.len(), 1);
        assert_eq!((g.slides[0].0, g.slides[0].1), (0, 1));
        let walking = path(
            "foot",
            &[
                (100.0, 600.0),
                (100.0, 600.0),
                (86.0, 600.0),
                (79.0, 601.0),
                (80.0, 600.0),
            ],
        );
        let w = ground(&walking, 600.0, 7.0, SLACK);
        assert_eq!(w.slides.iter().map(|s| (s.0, s.1)).collect::<Vec<_>>(), [(3, 4)]);
        assert_eq!(g.under.len(), 1);
        assert_eq!(g.under[0].0, 4);
        assert!(moved(&foot)[0].1 > 3.9);
    }

    #[test]
    fn a_foot_rolling_from_heel_to_toe_does_not_slide_and_a_pushed_one_does() {
        use klad_core::stroke::Stroke;
        let foot = |heel: (f32, f32), toe: (f32, f32)| -> Vec<Mark> {
            let line: Vec<(f32, f32, f32)> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    (heel.0 + (toe.0 - heel.0) * t, heel.1 + (toe.1 - heel.1) * t, 1.0)
                })
                .collect();
            vec![
                Stroke::new(1, "ink", [0.0, 0.0, 0.0, 1.0], 4.0, 1)
                    .with_points(&line, 0.0)
                    .into(),
            ]
        };
        let at = |marks: Vec<Mark>| spot_of(&marks, &[1]);
        let roll = Path {
            label: "foot".into(),
            spots: [
                foot((100.0, 250.0), (128.0, 238.0)),
                foot((100.0, 250.0), (130.0, 250.0)),
                foot((102.0, 238.0), (130.0, 250.0)),
            ]
            .into_iter()
            .enumerate()
            .map(|(f, m)| (f as u32, at(m)))
            .collect(),
        };
        let bottoms: Vec<f32> = roll.spots.iter().map(|(_, s)| s.unwrap().bottom[0]).collect();
        assert!(
            bottoms[2] - bottoms[0] > 20.0,
            "the lowest point jumps from heel to toe: {bottoms:?}"
        );
        let g = ground(&roll, 250.0, 0.0, SLACK);
        assert_eq!(g.touches, [0, 1, 2]);
        assert!(g.slides.is_empty(), "a roll is not a slide: {:?}", g.slides);
        let pushed = Path {
            label: "foot".into(),
            spots: [
                foot((100.0, 250.0), (130.0, 250.0)),
                foot((110.0, 250.0), (140.0, 250.0)),
            ]
            .into_iter()
            .enumerate()
            .map(|(f, m)| (f as u32, at(m)))
            .collect(),
        };
        let g = ground(&pushed, 250.0, 0.0, SLACK);
        assert_eq!(g.slides.len(), 1);
        assert!((g.slides[0].2 - 10.0).abs() < 0.5, "{:?}", g.slides);
        assert!(
            ground(&pushed, 250.0, -10.0, SLACK).slides.is_empty(),
            "travel accounts for it"
        );
    }

    #[test]
    fn crossing_feet_are_reported_once_per_crossing() {
        let near = path(
            "near",
            &[(100.0, 600.0), (120.0, 600.0), (160.0, 600.0), (180.0, 600.0)],
        );
        let far = path("far", &[(170.0, 600.0), (150.0, 600.0), (130.0, 600.0), (110.0, 600.0)]);
        assert_eq!(swaps(&[near.clone(), far]), [(0, 1, 1, 2)]);
        let apart = path("far", &[(300.0, 600.0), (300.0, 600.0), (300.0, 600.0), (300.0, 600.0)]);
        assert!(swaps(&[near, apart]).is_empty());
    }
}
