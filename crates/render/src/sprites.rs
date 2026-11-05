use crate::export::{dither8, f16_pixels, png_of_f16};
use anyhow::{Result, bail};
use klad_core::doc::{Clip, Document};
use klad_core::rng::{FNV_OFFSET, fnv};
use serde_json::{Value, json};
use skia_safe::{AlphaType, Image};

#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    pub name: String,
    pub from: u32,
    pub to: u32,
    pub fps: u32,
    pub looping: bool,
    pub clip: bool,
}

pub fn animations(doc: &Document, spec: &str) -> Result<Vec<Animation>> {
    let t = &doc.timeline;
    let last = t.frames - 1;
    let of_clip = |c: &Clip| Animation {
        name: c.name.clone(),
        from: c.from,
        to: c.to.min(last),
        fps: t.fps_of(c),
        looping: c.looping,
        clip: true,
    };
    let whole = |name: &str, from: u32, to: u32| Animation {
        name: name.to_string(),
        from,
        to,
        fps: t.fps,
        looping: true,
        clip: false,
    };
    if spec == "all" {
        return Ok(match t.clips.is_empty() {
            true => vec![whole("default", 0, last)],
            false => t.clips.iter().map(of_clip).collect(),
        });
    }
    if let Some(c) = t.clip(spec) {
        return Ok(vec![of_clip(c)]);
    }
    let (from, to) = crate::sheet::range(doc, spec)?;
    Ok(vec![whole(spec, from, to)])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub picture: usize,
    pub frame: u32,
    pub frames: u32,
    pub ms: u32,
}

fn ms_at(frames: u32, fps: u32) -> u64 {
    (frames as u64 * 2000 + fps as u64) / (2 * fps as u64)
}

pub fn runs(anim: &Animation, picture: impl Fn(u32) -> usize) -> Vec<Run> {
    let grouped = (anim.from..=anim.to).fold(Vec::<Run>::new(), |mut out, f| {
        let p = picture(f);
        match out.last_mut() {
            Some(run) if run.picture == p => run.frames += 1,
            _ => out.push(Run {
                picture: p,
                frame: f,
                frames: 1,
                ms: 0,
            }),
        }
        out
    });
    grouped
        .into_iter()
        .map(|run| {
            let start = run.frame - anim.from;
            Run {
                ms: (ms_at(start + run.frames, anim.fps) - ms_at(start, anim.fps)) as u32,
                ..run
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Data {
    Rgba8(Vec<u8>),
    Rgba16(Vec<half::f16>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub data: Data,
}

impl Pixels {
    pub fn of(image: &Image, depth: u8) -> Result<Self> {
        let data = match depth {
            8 => Data::Rgba8(dither8(image)?),
            16 => Data::Rgba16(f16_pixels(image, AlphaType::Unpremul)?),
            other => bail!("--depth takes 8 or 16, not {other}"),
        };
        Ok(Self {
            width: image.width() as u32,
            height: image.height() as u32,
            data,
        })
    }

    pub fn blank(width: u32, height: u32, depth: u8) -> Self {
        let n = width as usize * height as usize * 4;
        let data = match depth {
            16 => Data::Rgba16(vec![half::f16::ZERO; n]),
            _ => Data::Rgba8(vec![0; n]),
        };
        Self { width, height, data }
    }

    pub fn depth(&self) -> u8 {
        match self.data {
            Data::Rgba8(_) => 8,
            Data::Rgba16(_) => 16,
        }
    }

    fn bytes(&self) -> &[u8] {
        match &self.data {
            Data::Rgba8(d) => d,
            Data::Rgba16(d) => bytemuck::cast_slice(d),
        }
    }

    fn covered(&self, x: u32, y: u32) -> bool {
        let at = (y as usize * self.width as usize + x as usize) * 4 + 3;
        match &self.data {
            Data::Rgba8(d) => d[at] > 0,
            Data::Rgba16(d) => d[at].to_f32() > 0.0,
        }
    }

    pub fn trim(&self) -> Rect {
        let rows: Vec<u32> = (0..self.height)
            .filter(|&y| (0..self.width).any(|x| self.covered(x, y)))
            .collect();
        let (Some(&top), Some(&bottom)) = (rows.first(), rows.last()) else {
            return Rect { x: 0, y: 0, w: 1, h: 1 };
        };
        let columns = |y: u32| (0..self.width).filter(move |&x| self.covered(x, y));
        let left = rows.iter().filter_map(|&y| columns(y).next()).min().unwrap_or(0);
        let right = rows.iter().filter_map(|&y| columns(y).next_back()).max().unwrap_or(0);
        Rect {
            x: left,
            y: top,
            w: right - left + 1,
            h: bottom - top + 1,
        }
    }

    pub fn hash(&self) -> u64 {
        let dims = [self.width, self.height].map(u32::to_le_bytes).concat();
        fnv(fnv(FNV_OFFSET, &dims), self.bytes())
    }

    pub fn blit(&mut self, from: &Pixels, src: Rect, x: u32, y: u32) {
        let (fw, tw) = (from.width as usize * 4, self.width as usize * 4);
        let row = src.w as usize * 4;
        (0..src.h as usize).for_each(|r| {
            let a = (src.y as usize + r) * fw + src.x as usize * 4;
            let b = (y as usize + r) * tw + x as usize * 4;
            match (&mut self.data, &from.data) {
                (Data::Rgba8(to), Data::Rgba8(d)) => to[b..b + row].copy_from_slice(&d[a..a + row]),
                (Data::Rgba16(to), Data::Rgba16(d)) => to[b..b + row].copy_from_slice(&d[a..a + row]),
                _ => {}
            }
        });
    }

    pub fn crop(&self, rect: Rect) -> Pixels {
        let mut out = Pixels::blank(rect.w, rect.h, self.depth());
        out.blit(self, rect, 0, 0);
        out
    }

    pub fn png(&mut self) -> Result<Vec<u8>> {
        let (w, h) = (self.width, self.height);
        Ok(match &self.data {
            Data::Rgba8(d) => crate::pngwrite::write(w, h, 8, d),
            Data::Rgba16(d) => png_of_f16(w, h, d),
        })
    }
}

pub type Layout = (Vec<Placed>, Vec<(u32, u32)>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub page: usize,
    pub x: u32,
    pub y: u32,
}

fn shelves(sizes: &[(u32, u32)], p: u32, width: u32, max: u32) -> Layout {
    let mut pages = vec![(0u32, 0u32)];
    let (mut x, mut y, mut row) = (p, p, 0);
    let mut placed = Vec::with_capacity(sizes.len());
    for &(w, h) in sizes {
        if x + w + p > width {
            (x, y, row) = (p, y + row + p, 0);
        }
        if y + h + p > max {
            pages.push((0, 0));
            (x, y, row) = (p, p, 0);
        }
        let page = pages.len() - 1;
        placed.push(Placed { page, x, y });
        x += w + p;
        row = row.max(h);
        let size = &mut pages[page];
        *size = (size.0.max(x), size.1.max(y + h + p));
    }
    (placed, pages)
}

pub fn pack(sizes: &[(u32, u32)], padding: u32, max: u32) -> Result<Layout> {
    let p = padding;
    if let Some((i, (w, h))) = sizes
        .iter()
        .enumerate()
        .find(|(_, (w, h))| w + 2 * p > max || h + 2 * p > max)
    {
        bail!(
            "picture {i} is {w}x{h} after trimming and does not fit a {max} px page with {p} px padding; export at a smaller --scale or raise --max"
        );
    }
    let area: u64 = sizes.iter().map(|&(w, h)| (w + p) as u64 * (h + p) as u64).sum();
    let widest = sizes.iter().map(|s| s.0).max().unwrap_or(1) + 2 * p;
    let base = ((area as f64).sqrt().ceil() as u32 + p).max(widest);
    let tries: Vec<u32> = (0..=10)
        .map(|k| (base as f64 * (1.0 + k as f64 / 10.0)).ceil() as u32)
        .map(|w| w.min(max))
        .collect();
    let best = tries
        .iter()
        .map(|&w| shelves(sizes, p, w, max))
        .min_by_key(|(_, pages)| {
            let last = pages.last().copied().unwrap_or_default();
            (pages.len(), last.0.max(last.1), last.0 as u64 * last.1 as u64)
        })
        .unwrap_or_default();
    Ok(best)
}

pub struct Sheet<'a> {
    pub stem: &'a str,
    pub source: (u32, u32),
    pub trims: &'a [Rect],
    pub placed: &'a [Placed],
    pub pages: &'a [(u32, u32)],
    pub images: &'a [String],
    pub animations: &'a [(Animation, Vec<Run>)],
    pub scale: f32,
    pub depth: u8,
}

fn rect_json(r: Rect) -> Value {
    json!({ "x": r.x, "y": r.y, "w": r.w, "h": r.h })
}

pub fn aseprite(sheet: &Sheet) -> Value {
    let (sw, sh) = sheet.source;
    let mut frames: Vec<Value> = Vec::new();
    let mut tags: Vec<Value> = Vec::new();
    sheet.animations.iter().for_each(|(anim, runs)| {
        let first = frames.len();
        frames.extend(runs.iter().map(|run| {
            let (trim, at) = (sheet.trims[run.picture], sheet.placed[run.picture]);
            let prefix = if anim.clip { anim.name.as_str() } else { sheet.stem };
            let mut entry = json!({
                "filename": format!("{prefix} {}", run.frame),
                "frame": rect_json(Rect { x: at.x, y: at.y, ..trim }),
                "rotated": false,
                "trimmed": trim != Rect { x: 0, y: 0, w: sw, h: sh },
                "spriteSourceSize": rect_json(trim),
                "sourceSize": { "w": sw, "h": sh },
                "duration": run.ms,
            });
            if at.page > 0 {
                entry["image"] = json!(sheet.images[at.page]);
            }
            entry
        }));
        if anim.clip && frames.len() > first {
            let mut tag = json!({
                "name": anim.name,
                "from": first,
                "to": frames.len() - 1,
                "direction": "forward",
                "color": "#000000ff",
            });
            if !anim.looping {
                tag["repeat"] = json!("1");
            }
            tags.push(tag);
        }
    });
    let scale = format!("{:.3}", sheet.scale);
    let mut meta = json!({
        "app": "klad",
        "version": env!("CARGO_PKG_VERSION"),
        "image": sheet.images[0],
        "format": if sheet.depth == 16 { "RGBA16161616" } else { "RGBA8888" },
        "size": { "w": sheet.pages[0].0, "h": sheet.pages[0].1 },
        "scale": scale.trim_end_matches('0').trim_end_matches('.'),
        "frameTags": tags,
        "layers": [],
        "slices": [],
    });
    if sheet.pages.len() > 1 {
        meta["images"] = sheet
            .images
            .iter()
            .zip(sheet.pages)
            .map(|(image, (w, h))| json!({ "image": image, "size": { "w": w, "h": h } }))
            .collect();
    }
    json!({ "frames": frames, "meta": meta })
}

pub fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn number(v: f32) -> String {
    let text = format!("{v:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text {
        "-0" => "0".into(),
        t => t.into(),
    }
}

fn float(v: f32) -> String {
    let text = number(v);
    match text.contains('.') {
        true => text,
        false => format!("{text}.0"),
    }
}

fn rect2(r: [f32; 4]) -> String {
    format!("Rect2({})", r.map(number).join(", "))
}

pub struct Texture {
    pub path: String,
    pub uid: Option<String>,
}

pub struct SpriteFrames<'a> {
    pub uid: &'a str,
    pub textures: &'a [Texture],
    pub source: (u32, u32),
    pub trims: &'a [Rect],
    pub placed: &'a [Placed],
    pub animations: &'a [(Animation, Vec<Run>)],
}

pub fn sprite_frames(res: &SpriteFrames) -> String {
    let (sw, sh) = res.source;
    let mut out = format!("[gd_resource type=\"SpriteFrames\" format=3 uid={}]\n", quoted(res.uid));
    res.textures.iter().enumerate().for_each(|(i, t)| {
        let uid = t
            .uid
            .as_deref()
            .map_or(String::new(), |u| format!(" uid={}", quoted(u)));
        out += &format!(
            "\n[ext_resource type=\"Texture2D\"{uid} path={} id=\"{}_sheet\"]",
            quoted(&t.path),
            i + 1
        );
    });
    out += "\n";
    res.trims.iter().zip(res.placed).enumerate().for_each(|(i, (trim, at))| {
        out += &format!(
            "\n[sub_resource type=\"AtlasTexture\" id=\"AtlasTexture_{i}\"]\natlas = ExtResource(\"{}_sheet\")\nregion = {}\n",
            at.page + 1,
            rect2([at.x, at.y, trim.w, trim.h].map(|v| v as f32))
        );
        let margin = [trim.x, trim.y, sw - trim.w, sh - trim.h];
        if margin != [0; 4] {
            out += &format!("margin = {}\n", rect2(margin.map(|v| v as f32)));
        }
    });
    let animations: Vec<String> = res
        .animations
        .iter()
        .map(|(anim, runs)| {
            let frames: Vec<String> = runs
                .iter()
                .map(|run| {
                    format!(
                        "{{\n\"duration\": {},\n\"texture\": SubResource(\"AtlasTexture_{}\")\n}}",
                        float(run.frames as f32),
                        run.picture
                    )
                })
                .collect();
            format!(
                "{{\n\"frames\": [{}],\n\"loop\": {},\n\"name\": &{},\n\"speed\": {}\n}}",
                frames.join(", "),
                anim.looping,
                quoted(&anim.name),
                float(anim.fps as f32)
            )
        })
        .collect();
    out += &format!("\n[resource]\nanimations = [{}]\n", animations.join(", "));
    out
}

pub struct Scene<'a> {
    pub uid: &'a str,
    pub node: &'a str,
    pub frames_path: &'a str,
    pub frames_uid: &'a str,
    pub animation: &'a str,
    pub origin: [f32; 2],
}

pub fn node_name(stem: &str) -> String {
    let name: String = stem
        .chars()
        .map(|c| match c {
            '.' | ':' | '@' | '/' | '"' | '%' => '_',
            c => c,
        })
        .collect();
    match name.trim().is_empty() {
        true => "sprite".into(),
        false => name,
    }
}

pub fn scene(s: &Scene) -> String {
    format!(
        "[gd_scene format=3 uid={}]\n\n[ext_resource type=\"SpriteFrames\" uid={} path={} id=\"1_frames\"]\n\n[node name={} type=\"AnimatedSprite2D\"]\nsprite_frames = ExtResource(\"1_frames\")\nanimation = &{}\nautoplay = {}\ncentered = false\noffset = Vector2({}, {})\n",
        quoted(s.uid),
        quoted(s.frames_uid),
        quoted(s.frames_path),
        quoted(s.node),
        quoted(s.animation),
        quoted(s.animation),
        number(-s.origin[0]),
        number(-s.origin[1])
    )
}

pub fn uid_text(mut id: u64) -> String {
    id &= 0x7fff_ffff_ffff_ffff;
    let mut digits = Vec::new();
    loop {
        let c = (id % 34) as u8;
        digits.push(if c < 25 { b'a' + c } else { b'0' + c - 25 });
        id /= 34;
        if id == 0 {
            break;
        }
    }
    digits.reverse();
    format!("uid://{}", String::from_utf8_lossy(&digits))
}

pub fn uid_for(seed: &str) -> String {
    uid_text(fnv(FNV_OFFSET, seed.as_bytes()))
}

fn uid_in(text: &str) -> Option<String> {
    let at = text.find("uid=\"uid://")? + 5;
    let end = at + text[at..].find('"')?;
    Some(text[at..end].to_string())
}

pub fn header_uid(text: &str) -> Option<String> {
    uid_in(text.lines().next()?)
}

pub fn import_uid(text: &str) -> Option<String> {
    text.lines().find(|l| l.starts_with("uid=")).and_then(uid_in)
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::Clip;

    fn anim(from: u32, to: u32, fps: u32) -> Animation {
        Animation {
            name: "run".into(),
            from,
            to,
            fps,
            looping: true,
            clip: true,
        }
    }

    #[test]
    fn runs_merge_held_frames_and_their_milliseconds_add_up_to_the_clip() {
        let pictures = [0, 0, 1, 2, 2, 2, 0, 3, 3, 3, 3, 3];
        let run = runs(&anim(0, 11, 12), |f| pictures[f as usize]);
        assert_eq!(
            run.iter().map(|r| (r.picture, r.frame, r.frames)).collect::<Vec<_>>(),
            [(0, 0, 2), (1, 2, 1), (2, 3, 3), (0, 6, 1), (3, 7, 5)]
        );
        assert_eq!(run.iter().map(|r| r.ms).collect::<Vec<_>>(), [167, 83, 250, 83, 417]);
        assert_eq!(run.iter().map(|r| r.ms).sum::<u32>(), 1000);
        let at24 = runs(&anim(2, 8, 24), |f| pictures[f as usize]);
        assert_eq!(at24.iter().map(|r| r.ms).sum::<u32>(), 292, "7 frames at 24 fps");
        assert_eq!(at24[0].frame, 2);
    }

    #[test]
    fn animations_come_from_clips_or_the_whole_timeline() {
        let mut doc = Document::new(8, 8);
        doc.timeline.frames = 20;
        let names = |d: &Document, spec: &str| {
            animations(d, spec)
                .unwrap()
                .iter()
                .map(|a| (a.name.clone(), a.from, a.to, a.fps, a.looping))
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&doc, "all"), [("default".into(), 0, 19, 12, true)]);
        doc.timeline.clips = vec![
            Clip::new("idle", 0, 5, true),
            Clip {
                fps: Some(24),
                ..Clip::new("jump", 6, 13, false)
            },
        ];
        assert_eq!(
            names(&doc, "all"),
            [("idle".into(), 0, 5, 12, true), ("jump".into(), 6, 13, 24, false)]
        );
        assert_eq!(names(&doc, "jump"), [("jump".into(), 6, 13, 24, false)]);
        assert_eq!(names(&doc, "2-4"), [("2-4".into(), 2, 4, 12, true)]);
        assert!(animations(&doc, "walk").is_err());
    }

    #[test]
    fn packing_keeps_order_and_padding_and_starts_pages_under_the_limit() {
        let sizes = [(30, 20), (30, 20), (50, 10), (10, 40)];
        let (placed, pages) = pack(&sizes, 2, 128).unwrap();
        assert_eq!(pages.len(), 1);
        let boxes: Vec<[u32; 4]> = placed
            .iter()
            .zip(&sizes)
            .map(|(p, (w, h))| [p.x, p.y, p.x + w, p.y + h])
            .collect();
        boxes.iter().enumerate().for_each(|(i, a)| {
            assert!(a[0] >= 2 && a[1] >= 2 && a[2] + 2 <= pages[0].0 && a[3] + 2 <= pages[0].1);
            boxes[i + 1..].iter().for_each(|b| {
                let apart = a[2] + 2 <= b[0] || b[2] + 2 <= a[0] || a[3] + 2 <= b[1] || b[3] + 2 <= a[1];
                assert!(apart, "{a:?} {b:?}");
            });
        });
        assert_eq!((placed[0].x, placed[0].y, placed[1].x), (2, 2, 34));
        let (placed, pages) = pack(&[(60, 60); 5], 2, 128).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(placed.iter().map(|p| p.page).collect::<Vec<_>>(), [0, 0, 0, 0, 1]);
        assert_eq!(pages, [(126, 126), (64, 64)]);
        assert!(pack(&[(200, 10)], 2, 128).is_err());
    }

    #[test]
    fn trimming_finds_the_pixels_and_empty_pictures_keep_one_pixel() {
        let mut p = Pixels::blank(16, 8, 8);
        assert_eq!(p.trim(), Rect { x: 0, y: 0, w: 1, h: 1 });
        if let Data::Rgba8(d) = &mut p.data {
            [(3, 2), (9, 5), (5, 6)]
                .iter()
                .for_each(|&(x, y)| d[(y * 16 + x) * 4 + 3] = 255);
        }
        let trim = p.trim();
        assert_eq!(trim, Rect { x: 3, y: 2, w: 7, h: 5 });
        let cut = p.crop(trim);
        assert_eq!((cut.width, cut.height), (7, 5));
        assert_ne!(cut.hash(), p.hash());
        assert_eq!(cut.crop(cut.trim()), cut);
    }

    #[test]
    fn aseprite_json_lists_exposures_with_durations_and_tags_index_them() {
        let trims = [
            Rect {
                x: 4,
                y: 2,
                w: 10,
                h: 12,
            },
            Rect {
                x: 0,
                y: 0,
                w: 32,
                h: 32,
            },
        ];
        let placed = [Placed { page: 0, x: 2, y: 2 }, Placed { page: 1, x: 2, y: 2 }];
        let idle = Animation {
            looping: false,
            ..anim(0, 3, 10)
        };
        let anims = vec![
            (
                Animation {
                    name: "idle".into(),
                    ..idle
                },
                vec![
                    Run {
                        picture: 0,
                        frame: 0,
                        frames: 3,
                        ms: 300,
                    },
                    Run {
                        picture: 1,
                        frame: 3,
                        frames: 1,
                        ms: 100,
                    },
                ],
            ),
            (
                anim(4, 4, 10),
                vec![Run {
                    picture: 0,
                    frame: 4,
                    frames: 1,
                    ms: 100,
                }],
            ),
        ];
        let images = ["ball.png".to_string(), "ball-2.png".to_string()];
        let json = aseprite(&Sheet {
            stem: "ball",
            source: (32, 32),
            trims: &trims,
            placed: &placed,
            pages: &[(16, 18), (36, 36)],
            images: &images,
            animations: &anims,
            scale: 0.5,
            depth: 8,
        });
        let frames = json["frames"].as_array().unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0]["filename"], "idle 0");
        assert_eq!(frames[0]["frame"], json!({"x": 2, "y": 2, "w": 10, "h": 12}));
        assert_eq!(frames[0]["spriteSourceSize"], json!({"x": 4, "y": 2, "w": 10, "h": 12}));
        assert_eq!(
            (frames[0]["trimmed"].clone(), frames[1]["trimmed"].clone()),
            (json!(true), json!(false))
        );
        assert_eq!(frames[1]["image"], "ball-2.png");
        assert_eq!(frames.iter().map(|f| f["duration"].as_u64().unwrap()).sum::<u64>(), 500);
        let tags = json["meta"]["frameTags"].as_array().unwrap();
        assert_eq!(
            tags[0],
            json!({"name":"idle","from":0,"to":1,"direction":"forward","color":"#000000ff","repeat":"1"})
        );
        assert_eq!((tags[1]["from"].clone(), tags[1]["to"].clone()), (json!(2), json!(2)));
        assert_eq!(json["meta"]["scale"], "0.5");
        assert_eq!(json["meta"]["images"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn godot_text_has_atlas_frames_durations_and_an_offset_scene() {
        let trims = [
            Rect {
                x: 4,
                y: 2,
                w: 10,
                h: 12,
            },
            Rect {
                x: 0,
                y: 0,
                w: 32,
                h: 32,
            },
        ];
        let placed = [Placed { page: 0, x: 2, y: 2 }, Placed { page: 0, x: 14, y: 2 }];
        let anims = vec![(
            Animation {
                name: "say \"hi\"".into(),
                looping: false,
                ..anim(0, 3, 12)
            },
            vec![
                Run {
                    picture: 0,
                    frame: 0,
                    frames: 3,
                    ms: 250,
                },
                Run {
                    picture: 1,
                    frame: 3,
                    frames: 1,
                    ms: 83,
                },
            ],
        )];
        let textures = [Texture {
            path: "ball.png".into(),
            uid: Some("uid://abc".into()),
        }];
        let tres = sprite_frames(&SpriteFrames {
            uid: "uid://frames",
            textures: &textures,
            source: (32, 32),
            trims: &trims,
            placed: &placed,
            animations: &anims,
        });
        assert!(tres.starts_with("[gd_resource type=\"SpriteFrames\" format=3 uid=\"uid://frames\"]\n"));
        assert!(tres.contains("[ext_resource type=\"Texture2D\" uid=\"uid://abc\" path=\"ball.png\" id=\"1_sheet\"]"));
        assert!(tres.contains("region = Rect2(2, 2, 10, 12)\nmargin = Rect2(4, 2, 22, 20)\n"));
        assert!(!tres.contains("margin = Rect2(0, 0, 0, 0)"));
        assert!(tres.contains("\"duration\": 3.0,\n\"texture\": SubResource(\"AtlasTexture_0\")"));
        assert!(tres.contains("\"loop\": false,\n\"name\": &\"say \\\"hi\\\"\",\n\"speed\": 12.0\n}]"));
        assert_eq!(header_uid(&tres).as_deref(), Some("uid://frames"));
        let tscn = scene(&Scene {
            uid: "uid://scene",
            node: &node_name("ball.v2"),
            frames_path: "ball.tres",
            frames_uid: "uid://frames",
            animation: "idle",
            origin: [16.0, 30.5],
        });
        assert!(tscn.contains("[node name=\"ball_v2\" type=\"AnimatedSprite2D\"]"));
        assert!(tscn.contains("offset = Vector2(-16, -30.5)\n"));
        assert_eq!(header_uid(&tscn).as_deref(), Some("uid://scene"));
        let import = "[remap]\n\nimporter=\"texture\"\ntype=\"CompressedTexture2D\"\nuid=\"uid://c6xh0mqe8vbs6\"\n";
        assert_eq!(import_uid(import).as_deref(), Some("uid://c6xh0mqe8vbs6"));
    }

    #[test]
    fn uids_use_godots_alphabet_and_differ_by_path() {
        let a = uid_for("/game/art/ball.tres");
        let b = uid_for("/game/art/ball.tscn");
        assert_ne!(a, b);
        assert_eq!(a, uid_for("/game/art/ball.tres"));
        assert!(a.starts_with("uid://"));
        assert!(a[6..].chars().all(|c| matches!(c, 'a'..='y' | '0'..='8')), "{a}");
        assert_eq!(uid_text(0), "uid://a");
        assert_eq!(uid_text(34), "uid://ba");
        assert_eq!(uid_text(33), "uid://8");
    }
}
