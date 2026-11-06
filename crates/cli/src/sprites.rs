use crate::export::scale_tag;
use crate::layers::load_cels_at;
use crate::sheet::{batches, frame_cels, shown};
use anyhow::{Context, Result, anyhow, bail};
use klad_brush::BrushSet;
use klad_core::bundle::{Bundle, write_atomic};
use klad_core::ops::Op;
use klad_render::export::{Options, Target, detail_for, render};
use klad_render::sprites::{self, Animation, Pixels, Placed, Rect, Run, Scene, Sheet, SpriteFrames, Texture};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

pub struct Look {
    pub layer: Option<String>,
    pub scale: f32,
    pub depth: u8,
    pub background: bool,
}

pub struct Packing {
    pub padding: u32,
    pub max: u32,
}

pub struct Pictures {
    pub source: (u32, u32),
    pub trims: Vec<Rect>,
    pub cut: Vec<Pixels>,
    pub animations: Vec<(Animation, Vec<Run>)>,
    pub firsts: Vec<u32>,
    pub frames: usize,
    pub composed: usize,
    pub ms: f64,
}

struct Name {
    dir: PathBuf,
    stem: String,
}

impl Name {
    fn file(&self, suffix: &str) -> PathBuf {
        self.dir.join(format!("{}{suffix}", self.stem))
    }

    fn page(&self, i: usize) -> PathBuf {
        match i {
            0 => self.file(".png"),
            _ => self.file(&format!("-{}.png", i + 1)),
        }
    }
}

pub(crate) fn bundle_stem(bundle: &Bundle) -> String {
    let root = bundle.root.components().collect::<PathBuf>();
    root.file_stem()
        .map_or("export".into(), |s| s.to_string_lossy().to_string())
}

pub(crate) fn bundle_dir(bundle: &Bundle) -> PathBuf {
    let root = bundle.root.components().collect::<PathBuf>();
    match root.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

pub(crate) fn file_safe(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c,
            _ => '_',
        })
        .collect()
}

pub fn pictures(bundle: &Bundle, animations: Vec<Animation>, look: &Look) -> Result<Pictures> {
    let start = Instant::now();
    let doc = &bundle.doc;
    let layer = look.layer.as_deref().map(|k| doc.find_layer(k)).transpose()?;
    let target = layer.map_or(Target::Frame, Target::Layer);
    let options = Options {
        scale: look.scale,
        background: look.background,
        references: false,
        detail: detail_for(look.scale),
    };
    let frames: BTreeSet<u32> = animations.iter().flat_map(|a| a.from..=a.to).collect();
    let mut firsts: BTreeMap<Vec<String>, u32> = BTreeMap::new();
    let first_of: BTreeMap<u32, u32> = frames
        .iter()
        .map(|&f| (f, *firsts.entry(shown(doc, f)).or_insert(f)))
        .collect();
    let unique: Vec<u32> = firsts.values().copied().collect::<BTreeSet<_>>().into_iter().collect();
    let brushes = BrushSet::for_bundle(bundle)?;
    let (w, h) = (doc.canvas.width, doc.canvas.height);
    let work: Vec<_> = unique.iter().map(|&f| (f, frame_cels(doc, f, &[]))).collect();
    let mut made: BTreeMap<u32, (Rect, Pixels)> = BTreeMap::new();
    batches(work, w as usize * h as usize * 8)
        .into_iter()
        .try_for_each(|batch| {
            let needed: Vec<_> = batch
                .iter()
                .flat_map(|(_, c)| c.iter().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let rasters = load_cels_at(bundle, &brushes, &needed, options.detail)?;
            let done = batch
                .par_iter()
                .map(|(frame, _)| {
                    let image = render(doc, *frame, &rasters, &target, options)?;
                    let pixels = Pixels::of(&image, look.depth)?;
                    let trim = pixels.trim();
                    Ok((*frame, (trim, pixels.crop(trim))))
                })
                .collect::<Result<Vec<_>>>()?;
            made.extend(done);
            Ok::<_, anyhow::Error>(())
        })?;
    let mut by_hash: BTreeMap<(u64, [u32; 4]), usize> = BTreeMap::new();
    let mut trims = Vec::new();
    let mut cut = Vec::new();
    let mut firsts = Vec::new();
    let index_of: BTreeMap<u32, usize> = made
        .into_iter()
        .map(|(frame, (trim, pixels))| {
            let key = (pixels.hash(), [trim.x, trim.y, trim.w, trim.h]);
            let index = *by_hash.entry(key).or_insert_with(|| {
                trims.push(trim);
                cut.push(pixels);
                firsts.push(frame);
                trims.len() - 1
            });
            (frame, index)
        })
        .collect();
    let picture = |f: u32| index_of[&first_of[&f]];
    let animations = animations
        .into_iter()
        .map(|a| {
            let runs = sprites::runs(&a, picture);
            (a, runs)
        })
        .collect();
    let source = (
        ((w as f32 * look.scale).round() as u32).max(1),
        ((h as f32 * look.scale).round() as u32).max(1),
    );
    Ok(Pictures {
        source,
        trims,
        cut,
        animations,
        firsts,
        frames: frames.len(),
        composed: unique.len(),
        ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}

struct Pages {
    placed: Vec<Placed>,
    sizes: Vec<(u32, u32)>,
    files: Vec<PathBuf>,
    hashes: Vec<u64>,
}

fn write_pages(name: &Name, pics: &Pictures, packing: &Packing, depth: u8) -> Result<Pages> {
    let sizes: Vec<(u32, u32)> = pics.trims.iter().map(|t| (t.w, t.h)).collect();
    let room = packing.max.saturating_sub(2 * packing.padding);
    if let Some((t, frame)) = pics
        .trims
        .iter()
        .zip(&pics.firsts)
        .find(|(t, _)| t.w > room || t.h > room)
    {
        bail!(
            "frame {frame} is {}x{} px after trimming, more than a {} px page holds with {} px padding; export at a smaller --scale or raise --max",
            t.w,
            t.h,
            packing.max,
            packing.padding
        );
    }
    let (placed, pages) = sprites::pack(&sizes, packing.padding, packing.max)?;
    let encoded = pages
        .par_iter()
        .enumerate()
        .map(|(i, &(w, h))| {
            let mut page = Pixels::blank(w, h, depth);
            pics.cut
                .iter()
                .zip(&placed)
                .filter(|(_, at)| at.page == i)
                .for_each(|(cut, at)| {
                    let all = Rect {
                        x: 0,
                        y: 0,
                        w: cut.width,
                        h: cut.height,
                    };
                    page.blit(cut, all, at.x, at.y);
                });
            let hash = page.hash();
            Ok((page.png()?, hash))
        })
        .collect::<Result<Vec<_>>>()?;
    let files: Vec<PathBuf> = (0..pages.len()).map(|i| name.page(i)).collect();
    files
        .iter()
        .zip(&encoded)
        .try_for_each(|(file, (bytes, _))| write_atomic(file, bytes))?;
    Ok(Pages {
        placed,
        sizes: pages,
        hashes: encoded.iter().map(|(_, h)| *h).collect(),
        files,
    })
}

fn remove_stale_pages(name: &Name, kept: usize) {
    (kept..)
        .map(|i| name.page(i))
        .take_while(|p| p.exists())
        .for_each(|page| {
            let _ = fs::remove_file(&page);
            let _ = fs::remove_file(page.with_extension("png.import"));
        });
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or(String::new(), |n| n.to_string_lossy().to_string())
}

fn summary(pics: &Pictures, pages: &Pages) -> String {
    let sizes: Vec<String> = pages.sizes.iter().map(|(w, h)| format!("{w}x{h}")).collect();
    format!(
        "{} pictures from {} frames ({} composed), {} page{} {}",
        pics.trims.len(),
        pics.frames,
        pics.composed,
        pages.sizes.len(),
        if pages.sizes.len() == 1 { "" } else { "s" },
        sizes.join(" ")
    )
}

fn check_look(look: &Look) -> Result<()> {
    if look.depth != 8 && look.depth != 16 {
        bail!("--depth takes 8 or 16, not {}", look.depth);
    }
    Ok(())
}

pub fn sheet(path: &Path, spec: &str, look: &Look, packing: &Packing, out: Option<PathBuf>) -> Result<()> {
    let start = Instant::now();
    check_look(look)?;
    let bundle = Bundle::open(path)?;
    let animations = sprites::animations(&bundle.doc, spec)?;
    let stem = bundle_stem(&bundle);
    let name = match out {
        Some(out) => Name {
            dir: out.parent().map_or(PathBuf::from("."), Path::to_path_buf),
            stem: file_name(&out).trim_end_matches(".png").to_string(),
        },
        None => Name {
            dir: bundle_dir(&bundle),
            stem: format!(
                "{stem}-{}{}",
                if spec == "all" { "sheet".into() } else { file_safe(spec) },
                scale_tag(look.scale)
            ),
        },
    };
    let pics = pictures(&bundle, animations, look)?;
    let pages = write_pages(&name, &pics, packing, look.depth)?;
    remove_stale_pages(&name, pages.files.len());
    let images: Vec<String> = pages.files.iter().map(|f| file_name(f)).collect();
    let json = sprites::aseprite(&Sheet {
        stem: &stem,
        source: pics.source,
        trims: &pics.trims,
        placed: &pages.placed,
        pages: &pages.sizes,
        images: &images,
        animations: &pics.animations,
        scale: look.scale,
        depth: look.depth,
    });
    let text = serde_json::to_string_pretty(&json)? + "\n";
    let json_path = name.file(".json");
    write_atomic(&json_path, text.as_bytes())?;
    pages.files.iter().for_each(|f| println!("{}", f.display()));
    println!("{}", json_path.display());
    eprintln!("{}", summary(&pics, &pages));
    pages
        .files
        .iter()
        .zip(&pages.hashes)
        .for_each(|(f, h)| eprintln!("hash {h:016x}  {}", file_name(f)));
    eprintln!(
        "hash {:016x}  {}",
        klad_core::rng::fnv(klad_core::rng::FNV_OFFSET, text.as_bytes()),
        file_name(&json_path)
    );
    eprintln!(
        "pictures {:.0} ms, {:.0} ms in all",
        pics.ms,
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

pub fn frames(path: &Path, spec: &str, look: &Look, out: Option<PathBuf>) -> Result<()> {
    let start = Instant::now();
    check_look(look)?;
    let bundle = Bundle::open(path)?;
    let animations = sprites::animations(&bundle.doc, spec)?;
    let stem = bundle_stem(&bundle);
    let prefix = match spec {
        "all" => stem.clone(),
        other => file_safe(other),
    };
    let dir = out.unwrap_or_else(|| {
        let tag = if spec == "all" {
            "frames".into()
        } else {
            file_safe(spec)
        };
        bundle_dir(&bundle).join(format!("{stem}-{tag}{}", scale_tag(look.scale)))
    });
    fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
    let pics = pictures(&bundle, animations, look)?;
    let (sw, sh) = pics.source;
    let encoded = pics
        .cut
        .par_iter()
        .zip(&pics.trims)
        .map(|(cut, trim)| {
            let mut full = Pixels::blank(sw, sh, look.depth);
            let all = Rect {
                x: 0,
                y: 0,
                w: cut.width,
                h: cut.height,
            };
            full.blit(cut, all, trim.x, trim.y);
            full.png()
        })
        .collect::<Result<Vec<_>>>()?;
    let sequence: Vec<usize> = pics
        .animations
        .iter()
        .flat_map(|(_, runs)| {
            runs.iter()
                .flat_map(|r| std::iter::repeat_n(r.picture, r.frames as usize))
        })
        .collect();
    sequence
        .iter()
        .enumerate()
        .try_for_each(|(i, &p)| write_atomic(&dir.join(format!("{prefix}-{i:04}.png")), &encoded[p]))?;
    println!("{}", dir.display());
    eprintln!(
        "{} frames as {prefix}-0000.png to {prefix}-{:04}.png, {} pictures ({} composed), {:.0} ms",
        sequence.len(),
        sequence.len().saturating_sub(1),
        pics.trims.len(),
        pics.composed,
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

pub fn relative(from: &Path, to: &Path) -> PathBuf {
    let a: Vec<Component> = from.components().collect();
    let b: Vec<Component> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let ups = std::iter::repeat_n(Component::ParentDir, a.len() - common);
    let path: PathBuf = ups.chain(b[common..].iter().copied()).collect();
    match path.as_os_str().is_empty() {
        true => PathBuf::from("."),
        false => path,
    }
}

pub(crate) fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

pub fn godot(path: &Path, dir: Option<PathBuf>, store: bool, look: &Look, packing: &Packing) -> Result<()> {
    let start = Instant::now();
    check_look(look)?;
    let mut bundle = Bundle::open(path)?;
    let home = bundle_dir(&bundle);
    let stored = bundle.doc.export.godot.clone();
    let target = match (&dir, &stored) {
        (Some(d), _) => d.clone(),
        (None, Some(s)) => home.join(s),
        (None, None) => bail!(
            "this document has no Godot folder yet: give one with --godot <folder> (it is kept in the document for the next export and the editor's button)"
        ),
    };
    fs::create_dir_all(&target).with_context(|| format!("making {}", target.display()))?;
    let target = target.canonicalize()?;
    let rel = relative(&home.canonicalize()?, &target).to_string_lossy().to_string();
    let keep = match (&dir, &stored) {
        (Some(_), None) => true,
        (Some(_), Some(s)) => store && *s != rel,
        _ => false,
    };
    if keep {
        bundle.apply(&[Op::Export {
            godot: Some(rel.clone()),
        }])?;
        bundle.stamp("cli")?;
        eprintln!("kept {rel} as this document's Godot folder (export.godot)");
    } else if let (Some(_), Some(s)) = (&dir, &stored)
        && *s != rel
    {
        eprintln!("exported to {rel}; the document's Godot folder stays {s} (add --store to replace it)");
    }
    let doc = &bundle.doc;
    let animations = sprites::animations(doc, "all")?;
    let stem = file_safe(&bundle_stem(&bundle));
    let name = Name {
        dir: target.clone(),
        stem: stem.clone(),
    };
    let pics = pictures(&bundle, animations, look)?;
    let pages = write_pages(&name, &pics, packing, look.depth)?;
    remove_stale_pages(&name, pages.files.len());
    let textures: Vec<Texture> = pages
        .files
        .iter()
        .map(|f| Texture {
            path: file_name(f),
            uid: read(&f.with_extension("png.import")).and_then(|t| sprites::import_uid(&t)),
        })
        .collect();
    let (tres, tscn) = (name.file(".tres"), name.file(".tscn"));
    let uid = |file: &Path| {
        read(file)
            .and_then(|t| sprites::header_uid(&t))
            .unwrap_or_else(|| sprites::uid_for(&file.to_string_lossy()))
    };
    let (frames_uid, scene_uid) = (uid(&tres), uid(&tscn));
    let frames_text = sprites::sprite_frames(&SpriteFrames {
        uid: &frames_uid,
        textures: &textures,
        source: pics.source,
        trims: &pics.trims,
        placed: &pages.placed,
        animations: &pics.animations,
    });
    let (w, h) = (doc.canvas.width as f32, doc.canvas.height as f32);
    let [ox, oy] = doc.timeline.origin.unwrap_or([w / 2.0, h / 2.0]);
    let (sx, sy) = (pics.source.0 as f32 / w, pics.source.1 as f32 / h);
    let first = pics
        .animations
        .first()
        .map(|(a, _)| a.name.clone())
        .ok_or_else(|| anyhow!("nothing to export"))?;
    let scene_text = sprites::scene(&Scene {
        uid: &scene_uid,
        node: &sprites::node_name(&bundle_stem(&bundle)),
        frames_path: &file_name(&tres),
        frames_uid: &frames_uid,
        animation: &first,
        origin: [ox * sx, oy * sy],
    });
    write_atomic(&tres, frames_text.as_bytes())?;
    write_atomic(&tscn, scene_text.as_bytes())?;
    pages
        .files
        .iter()
        .chain([&tres, &tscn])
        .for_each(|f| println!("{}", f.display()));
    let names: Vec<String> = pics
        .animations
        .iter()
        .map(|(a, runs)| {
            format!(
                "{} ({} frames, {} entries, {} fps{})",
                a.name,
                a.to - a.from + 1,
                runs.len(),
                a.fps,
                if a.looping { ", loops" } else { "" }
            )
        })
        .collect();
    eprintln!("{}", summary(&pics, &pages));
    eprintln!("animations: {}", names.join(", "));
    eprintln!(
        "pictures {:.0} ms, {:.0} ms in all",
        pics.ms,
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::{Cel, Clip, Document, Layer};
    use klad_core::stroke::{Stroke, marks};
    use serde_json::Value;
    use std::process::Command;

    const CHECK: &str = r#"extends SceneTree

func _init():
	var args = OS.get_cmdline_user_args()
	var frames: SpriteFrames = load("res://art/hop.tres")
	var out = {"animations": {}}
	for anim in frames.get_animation_names():
		var list = []
		for i in frames.get_frame_count(anim):
			var t: AtlasTexture = frames.get_frame_texture(anim, i)
			list.append([frames.get_frame_duration(anim, i), t.get_width(), t.get_height(), t.region.position.x, t.region.position.y, t.margin.position.x, t.margin.position.y])
		out["animations"][anim] = {"speed": frames.get_animation_speed(anim), "loop": frames.get_animation_loop(anim), "frames": list}
	var node = load("res://art/hop.tscn").instantiate()
	out["scene"] = [node.get_class(), String(node.animation), node.centered, node.offset.x, node.offset.y]
	node.free()
	out["uids"] = []
	for uid in args:
		var res = load(uid)
		out["uids"].append(res.resource_path if res else "")
	print("RESULT ", JSON.stringify(out))
	quit()
"#;

    fn godot_binary() -> Option<String> {
        let binary = std::env::var("GODOT").unwrap_or_else(|_| "godot".into());
        let ok = Command::new(&binary)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success());
        ok.then_some(binary)
    }

    fn godot_run(binary: &str, project: &Path, args: &[&str]) -> String {
        let out = Command::new(binary)
            .args(["--headless", "--path"])
            .arg(project)
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr)
    }

    fn hop(dir: &Path) -> PathBuf {
        let mut doc = Document::new(64, 48);
        doc.timeline.frames = 10;
        doc.timeline.origin = Some([20.0, 40.0]);
        doc.timeline.clips = vec![
            Clip::new("idle", 0, 3, true),
            Clip {
                fps: Some(24),
                ..Clip::new("hop", 4, 9, false)
            },
        ];
        let mut ball = Layer::paint("l1", "ball", "c1");
        *ball.cels_mut().unwrap() = vec![
            Cel {
                hold: Some(3),
                ..Cel::at("c1", 0)
            },
            Cel::at("c2", 3),
            Cel::at("c3", 4),
            Cel {
                hold: Some(2),
                ..Cel::at("c4", 5)
            },
            Cel {
                link: Some("c1".into()),
                ..Cel::at("c5", 7)
            },
        ];
        doc.layers = vec![ball];
        let path = dir.join("hop.klad");
        let bundle = Bundle::create(&path, doc).unwrap();
        let dot = |x: f32, y: f32| {
            marks(vec![
                Stroke::new(1, "ink", [0.8, 0.2, 0.1, 1.0], 10.0, 1).with_points(&[(x, y, 1.0)], 0.0),
            ])
        };
        [
            ("c1", 20.0, 36.0),
            ("c2", 22.0, 28.0),
            ("c3", 24.0, 16.0),
            ("c4", 26.0, 24.0),
        ]
        .iter()
        .for_each(|(cel, x, y)| {
            bundle.write_marks("l1", cel, &dot(*x, *y)).unwrap();
        });
        path
    }

    fn look(scale: f32) -> Look {
        Look {
            layer: None,
            scale,
            depth: 8,
            background: false,
        }
    }

    const PACKING: Packing = Packing { padding: 2, max: 4096 };

    #[test]
    fn godot_imports_the_export_with_its_clips_timing_origin_and_uids() {
        let Some(binary) = godot_binary() else {
            eprintln!("no godot on PATH (or $GODOT): skipping the import check");
            return;
        };
        let dir = std::env::temp_dir().join(format!("draw-godot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let project = dir.join("game");
        fs::create_dir_all(project.join("art")).unwrap();
        fs::write(
            project.join("project.godot"),
            "config_version=5\n\n[application]\n\nconfig/name=\"klad check\"\n",
        )
        .unwrap();
        fs::write(project.join("check.gd"), CHECK).unwrap();
        let path = hop(&dir);
        godot(&path, Some(project.join("art")), false, &look(2.0), &PACKING).unwrap();
        assert_eq!(
            Bundle::open(&path).unwrap().doc.export.godot.as_deref(),
            Some("game/art")
        );
        let tres = project.join("art/hop.tres");
        let tscn = project.join("art/hop.tscn");
        let uids = |p: &Path| sprites::header_uid(&fs::read_to_string(p).unwrap()).unwrap();
        let (frames_uid, scene_uid) = (uids(&tres), uids(&tscn));
        godot_run(&binary, &project, &["--import"]);
        let result = |text: &str| -> Value {
            let line = text
                .lines()
                .find_map(|l| l.strip_prefix("RESULT "))
                .unwrap_or_else(|| panic!("no result from godot:\n{text}"));
            serde_json::from_str(line).unwrap()
        };
        let first = result(&godot_run(&binary, &project, &["--script", "res://check.gd"]));
        let anims = &first["animations"];
        let idle = &anims["idle"];
        assert_eq!(
            (idle["speed"].as_f64(), idle["loop"].as_bool()),
            (Some(12.0), Some(true))
        );
        let durations = |a: &Value| -> Vec<f64> {
            a["frames"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f[0].as_f64().unwrap())
                .collect()
        };
        assert_eq!(durations(idle), [3.0, 1.0]);
        let hop_anim = &anims["hop"];
        assert_eq!(
            (hop_anim["speed"].as_f64(), hop_anim["loop"].as_bool()),
            (Some(24.0), Some(false))
        );
        assert_eq!(durations(hop_anim), [1.0, 2.0, 3.0]);
        assert!(anims.get("default").is_none());
        hop_anim["frames"].as_array().unwrap().iter().for_each(|f| {
            assert_eq!(
                (f[1].as_f64(), f[2].as_f64()),
                (Some(128.0), Some(96.0)),
                "full canvas size at 2x"
            );
        });
        let idle_first = &idle["frames"][0];
        let hop_last = &hop_anim["frames"][2];
        assert_eq!(
            idle_first.as_array().unwrap()[3..],
            hop_last.as_array().unwrap()[3..],
            "a link shares its picture"
        );
        assert_eq!(
            first["scene"],
            serde_json::json!(["AnimatedSprite2D", "idle", false, -40.0, -80.0])
        );
        let png = project.join("art/hop.png.import");
        let texture_uid = sprites::import_uid(&fs::read_to_string(&png).unwrap()).unwrap();
        let bundle = Bundle::open(&path).unwrap();
        let moved = marks(vec![
            Stroke::new(1, "ink", [0.1, 0.3, 0.9, 1.0], 12.0, 1).with_points(&[(40.0, 10.0, 1.0)], 0.0),
        ]);
        bundle.write_marks("l1", "c3", &moved).unwrap();
        godot(&path, None, false, &look(2.0), &PACKING).unwrap();
        assert_eq!((uids(&tres), uids(&tscn)), (frames_uid.clone(), scene_uid.clone()));
        let text = fs::read_to_string(&tres).unwrap();
        assert!(
            text.contains(&format!("uid=\"{texture_uid}\" path=\"hop.png\"")),
            "{text}"
        );
        godot_run(&binary, &project, &["--import"]);
        let second = result(&godot_run(
            &binary,
            &project,
            &["--script", "res://check.gd", "--", &frames_uid, &scene_uid],
        ));
        assert_eq!(
            second["uids"],
            serde_json::json!(["res://art/hop.tres", "res://art/hop.tscn"]),
            "the uids Godot knew still load the files"
        );
        assert_ne!(
            second["animations"]["hop"]["frames"][0],
            first["animations"]["hop"]["frames"][0]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sheets_and_frames_land_next_to_the_bundle_and_add_up_to_each_clip() {
        let dir = std::env::temp_dir().join(format!("draw-sprites-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = hop(&dir);
        sheet(&path, "all", &look(1.0), &PACKING, None).unwrap();
        let json: Value = serde_json::from_str(&fs::read_to_string(dir.join("hop-sheet.json")).unwrap()).unwrap();
        let frames = json["frames"].as_array().unwrap();
        let tags = json["meta"]["frameTags"].as_array().unwrap();
        let ms = |tag: &Value| -> u64 {
            let (a, b) = (
                tag["from"].as_u64().unwrap() as usize,
                tag["to"].as_u64().unwrap() as usize,
            );
            frames[a..=b].iter().map(|f| f["duration"].as_u64().unwrap()).sum()
        };
        assert_eq!(
            (tags[0]["name"].as_str(), ms(&tags[0])),
            (Some("idle"), 333),
            "4 frames at 12 fps"
        );
        assert_eq!(
            (tags[1]["name"].as_str(), ms(&tags[1])),
            (Some("hop"), 250),
            "6 frames at 24 fps"
        );
        assert_eq!(frames.len(), 5);
        let rects: BTreeSet<String> = frames.iter().map(|f| f["frame"].to_string()).collect();
        assert_eq!(rects.len(), 4, "the link back to c1 reuses its picture");
        assert!(
            frames
                .iter()
                .all(|f| f["trimmed"] == true && f["sourceSize"] == serde_json::json!({"w": 64, "h": 48}))
        );
        let png = fs::read(dir.join("hop-sheet.png")).unwrap();
        sheet(&path, "all", &look(1.0), &PACKING, None).unwrap();
        assert_eq!(
            fs::read(dir.join("hop-sheet.png")).unwrap(),
            png,
            "exports repeat exactly"
        );
        sheet(&path, "hop", &look(0.5), &PACKING, None).unwrap();
        assert!(dir.join("hop-hop@0.5x.png").exists() && dir.join("hop-hop@0.5x.json").exists());
        frames_out(&path, &dir);
        fs::remove_dir_all(&dir).unwrap();
    }

    fn frames_out(path: &Path, dir: &Path) {
        frames(path, "hop", &look(1.0), None).unwrap();
        let out = dir.join("hop-hop");
        let names: Vec<String> = fs::read_dir(&out)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(names.len(), 6);
        assert_eq!((names[0].as_str(), names[5].as_str()), ("hop-0000.png", "hop-0005.png"));
        let read = |n: usize| fs::read(out.join(&names[n])).unwrap();
        assert_eq!(read(1), read(2), "a held frame is the same picture");
        assert_ne!(read(0), read(1));
        assert_eq!(u32::from_be_bytes(read(0)[16..20].try_into().unwrap()), 64);
    }

    #[test]
    fn relative_folders_walk_up_and_down() {
        let rel = |a: &str, b: &str| relative(Path::new(a), Path::new(b)).to_string_lossy().to_string();
        assert_eq!(rel("/work/art", "/work/game/sprites"), "../game/sprites");
        assert_eq!(rel("/work/art", "/work/art/godot"), "godot");
        assert_eq!(rel("/work/art", "/work/art"), ".");
        assert_eq!(rel("/a/b/c", "/x"), "../../../x");
    }
}
