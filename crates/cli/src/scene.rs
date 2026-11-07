use crate::layers::{self, Vectors};
use crate::sprites::{bundle_dir, bundle_stem, file_safe, read};
use anyhow::{Context, Result, bail};
use klad_brush::{BrushSet, Raster, TILE};
use klad_core::bundle::{Bundle, write_atomic};
use klad_core::doc::Layer;
use klad_render::sprites::{header_uid, import_uid, node_name, number, quoted, uid_for};
use klad_render::{Rasters, flatten_with};
use std::fs;
use std::path::{Path, PathBuf};

pub fn rgba(raster: &Raster) -> Vec<u8> {
    let (w, h) = (raster.width as usize, raster.height as usize);
    let mut out = vec![0u8; w * h * 4];
    raster.occupied().for_each(|(index, tile)| {
        let (ox, oy) = ((index % raster.cols) * TILE, (index / raster.cols) * TILE);
        tile.chunks(4).enumerate().for_each(|(i, px)| {
            let (x, y) = (ox + i % TILE, oy + i / TILE);
            if x >= w || y >= h {
                return;
            }
            let a = px[3].to_f32().clamp(0.0, 1.0);
            if a <= 0.0 {
                return;
            }
            let at = (y * w + x) * 4;
            (0..3).for_each(|c| out[at + c] = ((px[c].to_f32() / a).clamp(0.0, 1.0) * 255.0).round() as u8);
            out[at + 3] = (a * 255.0).round() as u8;
        });
    });
    out
}

pub fn png(w: u32, h: u32, pixels: Vec<u8>) -> Result<Vec<u8>> {
    klad_render::assets::encode_png(&klad_render::assets::Rgba {
        width: w,
        height: h,
        pixels,
    })
}

fn target(bundle: &Bundle, dir: Option<PathBuf>) -> Result<PathBuf> {
    let home = bundle_dir(bundle);
    let target = match (dir, &bundle.doc.export.godot) {
        (Some(d), _) => d,
        (None, Some(s)) => home.join(s),
        (None, None) => {
            bail!("give the Godot folder to write into, or set one with klad export --godot <folder> first")
        }
    };
    fs::create_dir_all(&target).with_context(|| format!("making {}", target.display()))?;
    Ok(target)
}

fn uid_of(file: &Path) -> String {
    read(file)
        .and_then(|t| header_uid(&t))
        .unwrap_or_else(|| uid_for(&file.to_string_lossy()))
}

fn texture_uid(file: &Path) -> String {
    read(&file.with_extension("png.import"))
        .and_then(|t| import_uid(&t))
        .unwrap_or_else(|| uid_for(&format!("texture {}", file.display())))
}

pub fn scroll_scale(depth: f32) -> f32 {
    1.0 / (1.0 + depth.max(0.0))
}

pub struct Plane {
    pub name: String,
    pub depth: f32,
    pub file: PathBuf,
}

pub fn rasters_at(bundle: &Bundle, frame: u32) -> Result<Rasters> {
    let brushes = BrushSet::for_bundle(bundle)?;
    let mut rasters = layers::load(bundle, frame, &brushes, true)?.rasters;
    Vectors::default().add_to(&bundle.doc, frame, &mut rasters)?;
    Ok(rasters)
}

pub fn parallax(path: &Path, dir: Option<PathBuf>, frame: u32) -> Result<Vec<PathBuf>> {
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    let target = target(&bundle, dir)?;
    let stem = file_safe(&bundle_stem(&bundle));
    let rasters = rasters_at(&bundle, frame)?;
    let shown: Vec<&Layer> = doc.layers.iter().filter(|l| l.visible && !l.reference).collect();
    if shown.is_empty() {
        bail!("nothing to export: no visible layers");
    }
    let background = doc.canvas.background.clone();
    let planes: Vec<Plane> = shown
        .iter()
        .enumerate()
        .map(|(i, layer)| {
            let back = if i == 0 { background.as_deref() } else { None };
            let flat = flatten_with(doc, frame, &rasters, &[*layer], back, None, false)?;
            let file = target.join(format!("{stem}-{i:02}-{}.png", file_safe(&layer.name)));
            write_atomic(&file, &png(flat.width, flat.height, rgba(&flat))?)?;
            Ok(Plane {
                name: node_name(&layer.name),
                depth: layer.depth.unwrap_or(0.0),
                file,
            })
        })
        .collect::<Result<_>>()?;
    let scene = target.join(format!("{stem}-parallax.tscn"));
    let resources: String = planes
        .iter()
        .enumerate()
        .map(|(i, p)| {
            format!(
                "[ext_resource type=\"Texture2D\" uid={} path={} id=\"{}_plane\"]\n",
                quoted(&texture_uid(&p.file)),
                quoted(&p.file.file_name().unwrap_or_default().to_string_lossy()),
                i + 1
            )
        })
        .collect();
    let nodes: String = planes
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let s = number(scroll_scale(p.depth));
            format!(
                "\n[node name={name} type=\"Parallax2D\" parent=\".\"]\nscroll_scale = Vector2({s}, {s})\n\n[node name=\"Sprite2D\" type=\"Sprite2D\" parent={name}]\ntexture = ExtResource(\"{}_plane\")\ncentered = false\n",
                i + 1,
                name = quoted(&format!("{:02} {}", i, p.name)),
            )
        })
        .collect();
    let text = format!(
        "[gd_scene load_steps={} format=3 uid={}]\n\n{resources}\n[node name={} type=\"Node2D\"]\n{nodes}",
        planes.len() + 1,
        quoted(&uid_of(&scene)),
        quoted(&node_name(&bundle_stem(&bundle))),
    );
    write_atomic(&scene, text.as_bytes())?;
    Ok(planes.iter().map(|p| p.file.clone()).chain([scene]).collect())
}

struct Part {
    name: String,
    file: PathBuf,
    origin: [f32; 2],
    vertices: Vec<[f32; 2]>,
    triangles: Vec<[usize; 3]>,
}

const GRID: u32 = 16;

fn mesh(alpha: &[u8], w: u32, h: u32, origin: [f32; 2]) -> (Vec<[f32; 2]>, Vec<[usize; 3]>) {
    let (cols, rows) = (w.div_ceil(GRID), h.div_ceil(GRID));
    let solid = |cx: i64, cy: i64| -> bool {
        (cy * GRID as i64..((cy + 1) * GRID as i64).min(h as i64)).any(|y| {
            (cx * GRID as i64..((cx + 1) * GRID as i64).min(w as i64))
                .any(|x| x >= 0 && y >= 0 && alpha[(y as u32 * w + x as u32) as usize] > 8)
        })
    };
    let used: Vec<(u32, u32)> = (0..rows)
        .flat_map(|cy| (0..cols).map(move |cx| (cx, cy)))
        .filter(|&(cx, cy)| (-1..=1).any(|dy| (-1..=1).any(|dx| solid(cx as i64 + dx, cy as i64 + dy))))
        .collect();
    let mut index: std::collections::BTreeMap<(u32, u32), usize> = std::collections::BTreeMap::new();
    let mut vertices = Vec::new();
    let mut vertex = |gx: u32, gy: u32| -> usize {
        *index.entry((gx, gy)).or_insert_with(|| {
            vertices.push([
                origin[0] + (gx * GRID).min(w) as f32,
                origin[1] + (gy * GRID).min(h) as f32,
            ]);
            vertices.len() - 1
        })
    };
    let triangles = used
        .iter()
        .flat_map(|&(cx, cy)| {
            let (a, b, c, d) = (
                vertex(cx, cy),
                vertex(cx + 1, cy),
                vertex(cx + 1, cy + 1),
                vertex(cx, cy + 1),
            );
            [[a, b, c], [a, c, d]]
        })
        .collect();
    (vertices, triangles)
}

fn rest_cel(layer: &Layer) -> Option<String> {
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    layer
        .cels()
        .iter()
        .filter_map(|c| c.pose.as_ref())
        .for_each(|p| *counts.entry(p.rest.as_str()).or_default() += 1);
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(r, _)| r.to_string())
        .or_else(|| layer.content_at(0).map(|c| c.id.clone()))
}

fn transform(angle: f32, at: [f32; 2]) -> String {
    let (s, c) = (angle.sin(), angle.cos());
    format!(
        "Transform2D({}, {}, {}, {}, {}, {})",
        number(c),
        number(s),
        number(-s),
        number(c),
        number(at[0]),
        number(at[1])
    )
}

pub fn bone_paths(bones: &[klad_core::rig::Bone]) -> Vec<String> {
    bones
        .iter()
        .map(|b| {
            let mut chain = vec![node_name(&b.name)];
            let mut parent = b.parent.clone();
            while let Some(p) = parent {
                let found = bones.iter().find(|o| o.name == p);
                chain.push(node_name(&p));
                parent = found.and_then(|o| o.parent.clone());
            }
            chain.reverse();
            chain.join("/")
        })
        .collect()
}

pub fn local(bones: &[klad_core::rig::Bone], i: usize) -> (f32, [f32; 2]) {
    let b = &bones[i];
    match b.parent.as_ref().and_then(|p| bones.iter().find(|o| &o.name == p)) {
        None => (b.angle(), b.head),
        Some(p) => {
            let a = p.angle();
            let (dx, dy) = (b.head[0] - p.head[0], b.head[1] - p.head[1]);
            let (s, c) = (a.sin(), a.cos());
            (b.angle() - a, [dx * c + dy * s, -dx * s + dy * c])
        }
    }
}

fn real(v: f32) -> String {
    let text = number(v);
    match text.contains('.') {
        true => text,
        false => format!("{text}.0"),
    }
}

fn floats(values: &[f32]) -> String {
    format!(
        "PackedFloat32Array({})",
        values.iter().map(|v| number(*v)).collect::<Vec<_>>().join(", ")
    )
}

pub fn rig(path: &Path, dir: Option<PathBuf>) -> Result<Vec<PathBuf>> {
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    let target = target(&bundle, dir)?;
    let stem = file_safe(&bundle_stem(&bundle));
    let owners = crate::rig::owners(doc);
    if owners.is_empty() {
        bail!("this drawing has no bones: add them with klad bone (or the bone tool) first");
    }
    let brushes = BrushSet::for_bundle(&bundle)?;
    let (cw, ch) = (doc.canvas.width, doc.canvas.height);
    let mut written = Vec::new();
    owners.iter().try_for_each(|owner| -> Result<()> {
        let bones = &owner.bones;
        let weights = match bundle.read_weights(&owner.id)? {
            w if w.maps.is_empty() => klad_core::rig::Weights::auto(bones, (cw, ch)),
            w => w,
        };
        let name = file_safe(&owner.name);
        let mut parts: Vec<Part> = Vec::new();
        klad_core::rig::painted(owner).iter().try_for_each(|layer| -> Result<()> {
            let Some(rest) = rest_cel(layer) else {
                return Ok(());
            };
            let cel = layer.cels().iter().find(|c| c.id == rest).context("the rest drawing is gone")?;
            let marks = bundle.read_marks(&layer.id, &rest)?;
            let mut groups: Vec<(String, Vec<klad_core::stroke::Mark>)> = cel
                .parts
                .iter()
                .map(|(n, ids)| (n.clone(), marks.iter().filter(|m| ids.contains(&m.id())).cloned().collect()))
                .collect();
            let loose: Vec<klad_core::stroke::Mark> = marks
                .iter()
                .filter(|m| !cel.parts.values().any(|ids| ids.contains(&m.id())))
                .cloned()
                .collect();
            if !loose.is_empty() {
                groups.insert(0, (layer.name.clone(), loose));
            }
            groups.into_iter().try_for_each(|(part, marks)| -> Result<()> {
                let raster = klad_brush::replay(cw, ch, &marks, &brushes)?.raster;
                let pixels = rgba(&raster);
                let alpha: Vec<u8> = pixels.chunks(4).map(|p| p[3]).collect();
                let (mut x0, mut y0, mut x1, mut y1) = (cw, ch, 0, 0);
                (0..ch).for_each(|y| {
                    (0..cw).filter(|&x| alpha[(y * cw + x) as usize] > 0).for_each(|x| {
                        x0 = x0.min(x);
                        y0 = y0.min(y);
                        x1 = x1.max(x + 1);
                        y1 = y1.max(y + 1);
                    })
                });
                if x1 <= x0 || y1 <= y0 {
                    return Ok(());
                }
                let (w, h) = (x1 - x0, y1 - y0);
                let crop: Vec<u8> = (y0..y1)
                    .flat_map(|y| pixels[((y * cw + x0) * 4) as usize..((y * cw + x1) * 4) as usize].to_vec())
                    .collect();
                let file = target.join(format!("{stem}-{name}-{}-{}.png", file_safe(&layer.name), file_safe(&part)));
                write_atomic(&file, &png(w, h, crop)?)?;
                let small: Vec<u8> = (y0..y1)
                    .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                    .map(|(x, y)| alpha[(y * cw + x) as usize])
                    .collect();
                let (vertices, triangles) = mesh(&small, w, h, [x0 as f32, y0 as f32]);
                parts.push(Part {
                    name: node_name(&format!("{} {}", layer.name, part)),
                    file,
                    origin: [x0 as f32, y0 as f32],
                    vertices,
                    triangles,
                });
                Ok(())
            })
        })?;
        let scene = target.join(format!("{stem}-{name}-rig.tscn"));
        let paths = bone_paths(bones);
        let resources: String = parts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                format!(
                    "[ext_resource type=\"Texture2D\" uid={} path={} id=\"{}_part\"]\n",
                    quoted(&texture_uid(&p.file)),
                    quoted(&p.file.file_name().unwrap_or_default().to_string_lossy()),
                    i + 1
                )
            })
            .collect();
        let fps = doc.timeline.fps.max(1) as f32;
        let clips: Vec<(String, u32, u32, bool, f32)> = match doc.timeline.clips.is_empty() {
            true => vec![("all".into(), 0, doc.timeline.frames.saturating_sub(1), true, fps)],
            false => doc
                .timeline
                .clips
                .iter()
                .map(|c| (c.name.clone(), c.from, c.to, c.looping, c.fps.map_or(fps, |f| f as f32)))
                .collect(),
        };
        let first = klad_core::rig::painted(owner).into_iter().next().context("no paint layer under the bones")?;
        let mut unposed = Vec::new();
        let animations: Vec<String> = clips
            .iter()
            .enumerate()
            .map(|(k, (clip, from, to, looping, fps))| {
                let poses: Vec<(f32, klad_core::rig::Pose)> = (*from..=*to)
                    .filter_map(|f| {
                        let cel = first.cel_at(f).filter(|c| c.frame == f || f == *from)?;
                        let shown = first.content_of(cel);
                        match &shown.pose {
                            Some(p) => Some(((f - from) as f32 / fps, p.clone())),
                            None if shown.id == rest_cel(first).unwrap_or_default() => {
                                Some(((f - from) as f32 / fps, klad_core::rig::Pose::default()))
                            }
                            None => {
                                unposed.push(f);
                                None
                            }
                        }
                    })
                    .collect();
                let times: Vec<f32> = poses.iter().map(|(t, _)| *t).collect();
                let tracks: String = bones
                    .iter()
                    .enumerate()
                    .map(|(i, b)| {
                        let (rest, _) = local(bones, i);
                        let values: Vec<f32> = poses
                            .iter()
                            .map(|(_, p)| {
                                let turned = klad_core::rig::applied(bones, &owner.ik, p);
                                rest + turned.get(&b.name).copied().unwrap_or(0.0) * (std::f32::consts::PI / 180.0)
                            })
                            .collect();
                        format!(
                            "tracks/{i}/type = \"value\"\ntracks/{i}/imported = false\ntracks/{i}/enabled = true\ntracks/{i}/path = NodePath(\"Skeleton2D/{}:rotation\")\ntracks/{i}/interp = 0\ntracks/{i}/loop_wrap = true\ntracks/{i}/keys = {{\n\"times\": {},\n\"transitions\": {},\n\"update\": 0,\n\"values\": [{}]\n}}\n",
                            paths[i],
                            floats(&times),
                            floats(&vec![1.0; times.len()]),
                            values.iter().map(|v| real(*v)).collect::<Vec<_>>().join(", ")
                        )
                    })
                    .collect();
                format!(
                    "[sub_resource type=\"Animation\" id=\"Animation_{k}\"]\nresource_name = {}\nlength = {}\nloop_mode = {}\nstep = {}\n{tracks}\n",
                    quoted(clip),
                    number((to - from + 1) as f32 / fps),
                    if *looping { 1 } else { 0 },
                    number(1.0 / fps)
                )
            })
            .collect();
        let library = format!(
            "[sub_resource type=\"AnimationLibrary\" id=\"AnimationLibrary_1\"]\n_data = {{\n{}\n}}\n",
            clips
                .iter()
                .enumerate()
                .map(|(k, (c, ..))| format!("&{}: SubResource(\"Animation_{k}\")", quoted(c)))
                .collect::<Vec<_>>()
                .join(",\n")
        );
        let skeleton: String = bones
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let (angle, at) = local(bones, i);
                let parent = match paths[i].rsplit_once('/') {
                    Some((p, _)) => format!("Skeleton2D/{p}"),
                    None => "Skeleton2D".into(),
                };
                format!(
                    "\n[node name={} type=\"Bone2D\" parent={}]\nposition = Vector2({}, {})\nrotation = {}\nrest = {}\nauto_calculate_length_and_angle = false\nlength = {}\nbone_angle = 0.0\n",
                    quoted(&node_name(&b.name)),
                    quoted(&parent),
                    number(at[0]),
                    number(at[1]),
                    number(angle),
                    transform(angle, at),
                    number(b.length())
                )
            })
            .collect();
        let polygons: String = parts
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let uv: Vec<String> = p
                    .vertices
                    .iter()
                    .map(|v| format!("{}, {}", number(v[0] - p.origin[0]), number(v[1] - p.origin[1])))
                    .collect();
                let points: Vec<String> = p.vertices.iter().map(|v| format!("{}, {}", number(v[0]), number(v[1]))).collect();
                let tris: Vec<String> = p
                    .triangles
                    .iter()
                    .map(|t| format!("PackedInt32Array({}, {}, {})", t[0], t[1], t[2]))
                    .collect();
                let per_vertex: Vec<Vec<f32>> = p.vertices.iter().map(|v| weights.at(bones, *v)).collect();
                let bone_list: Vec<String> = paths
                    .iter()
                    .enumerate()
                    .map(|(i, path)| {
                        format!(
                            "NodePath({}), {}",
                            quoted(path),
                            floats(&per_vertex.iter().map(|w| w[i]).collect::<Vec<_>>())
                        )
                    })
                    .collect();
                format!(
                    "\n[node name={} type=\"Polygon2D\" parent=\".\"]\ntexture = ExtResource(\"{}_part\")\nskeleton = NodePath(\"../Skeleton2D\")\npolygon = PackedVector2Array({})\nuv = PackedVector2Array({})\npolygons = [{}]\nbones = [{}]\n",
                    quoted(&format!("{:02} {}", k, p.name)),
                    k + 1,
                    points.join(", "),
                    uv.join(", "),
                    tris.join(", "),
                    bone_list.join(", ")
                )
            })
            .collect();
        let text = format!(
            "[gd_scene load_steps={} format=3 uid={}]\n\n{resources}\n{}{library}\n[node name={} type=\"Node2D\"]\n{polygons}\n[node name=\"Skeleton2D\" type=\"Skeleton2D\" parent=\".\"]\n{skeleton}\n[node name=\"AnimationPlayer\" type=\"AnimationPlayer\" parent=\".\"]\nlibraries = {{\n&\"\": SubResource(\"AnimationLibrary_1\")\n}}\nautoplay = {}\n",
            parts.len() + clips.len() + 2,
            quoted(&uid_of(&scene)),
            animations.join(""),
            quoted(&node_name(&owner.name)),
            quoted(&clips[0].0)
        );
        write_atomic(&scene, text.as_bytes())?;
        if !unposed.is_empty() {
            unposed.sort_unstable();
            unposed.dedup();
            eprintln!(
                "frames {} of {} are not poses of the rest drawing: the rig cannot show them; the frames export (--godot) carries them",
                unposed.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(", "),
                owner.name
            );
        }
        written.extend(parts.iter().map(|p| p.file.clone()));
        written.push(scene);
        Ok(())
    })?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Output;
    use klad_core::doc::Document;
    use klad_core::ops::parse_lines;
    use std::process::Command;

    const CHECK: &str = r#"extends SceneTree

var frames := 0
var out := {}
var scene: Node
var camera: Camera2D

func _initialize():
	var rig = load("res://art/arm-arm-rig.tscn").instantiate()
	root.add_child(rig)
	var player: AnimationPlayer = rig.get_node("AnimationPlayer")
	var bone: Bone2D = rig.get_node("Skeleton2D/upper/fore")
	var poly: Polygon2D = rig.get_child(0)
	out["anims"] = player.get_animation_list()
	out["bones"] = poly.get_bone_count()
	out["vertices"] = poly.polygon.size()
	out["weights"] = poly.get_bone_weights(1).size()
	player.play("wave")
	player.seek(0.1, true)
	out["held"] = bone.rotation
	player.seek(0.35, true)
	out["posed"] = bone.rotation
	scene = load("res://art/arm-parallax.tscn").instantiate()
	root.add_child(scene)
	camera = Camera2D.new()
	root.add_child(camera)
	camera.make_current()

func _process(_delta):
	frames += 1
	if frames == 2:
		camera.position = Vector2(300, 0)
	if frames == 5:
		var planes = []
		for p in scene.get_children():
			if p is Parallax2D:
				planes.append([p.scroll_scale.x, p.screen_offset.x, p.get_child(0).global_position.x])
		out["planes"] = planes
		print("RESULT ", JSON.stringify(out))
		quit()
	return false
"#;

    fn quiet() -> Output {
        Output {
            json: false,
            render: false,
            scale: None,
            frame: 0,
            overlay: None,
        }
    }

    #[test]
    fn a_rig_and_a_parallax_scene_export_for_godot() {
        let dir = std::env::temp_dir().join(format!("klad-scene-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let project = dir.join("game");
        fs::create_dir_all(project.join("art")).unwrap();
        fs::write(
            project.join("project.godot"),
            "config_version=5\n\n[application]\n\nconfig/name=\"klad check\"\n",
        )
        .unwrap();
        fs::write(project.join("check.gd"), CHECK).unwrap();
        let path = dir.join("arm.klad");
        let mut doc = Document::new(600, 400);
        doc.canvas.background = Some("#f0e8dc".into());
        doc.timeline.frames = 8;
        doc.layers = vec![Layer::paint("far", "far", "c1"), Layer::paint("arm", "arm", "c2")];
        doc.layers[0].depth = Some(3.0);
        doc.next_id = 10;
        Bundle::create(&path, doc).unwrap();
        let edit = |ops: &str| crate::agent::edit(&path, &parse_lines(ops).unwrap(), &quiet()).unwrap();
        edit(
            r##"{"op":"fill","layer":"far","polygon":[[0,250],[600,250],[600,400],[0,400]],"color":"#8090a0"}
{"op":"stroke","layer":"arm","brush":"ink","size":24,"color":"#c06a3a","path":"M100 200 L 300 200 L 480 200"}
{"op":"clip","add":"wave","from":0,"to":7,"loop":true}"##,
        );
        edit(
            r#"{"op":"bone","layer":"arm","add":"upper","head":[100,200],"tail":[300,200]}
{"op":"bone","layer":"arm","add":"fore","parent":"upper","head":[300,200],"tail":[480,200]}"#,
        );
        edit(r#"{"op":"weights","layer":"arm","auto":true}"#);
        edit(r#"{"op":"pose","layer":"arm","from":0,"to":4,"bones":{"fore":-70}}"#);
        let made = rig(&path, Some(project.join("art"))).unwrap();
        let text = fs::read_to_string(made.last().unwrap()).unwrap();
        assert!(text.contains("type=\"Skeleton2D\"") && text.contains("NodePath(\"Skeleton2D/upper/fore:rotation\")"));
        assert!(
            text.contains("\"values\": [0.0, -1.222]"),
            "angles in radians, as floats"
        );
        let planes = parallax(&path, Some(project.join("art")), 0).unwrap();
        let scene = fs::read_to_string(planes.last().unwrap()).unwrap();
        assert!(scene.contains("scroll_scale = Vector2(0.25, 0.25)") && scene.contains("scroll_scale = Vector2(1, 1)"));
        let again = uid_of(planes.last().unwrap());
        parallax(&path, Some(project.join("art")), 0).unwrap();
        assert_eq!(uid_of(planes.last().unwrap()), again, "a re-export keeps its uid");
        let binary = std::env::var("GODOT").unwrap_or_else(|_| "godot".into());
        if !Command::new(&binary)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            eprintln!("no godot on PATH (or $GODOT): skipping the import check");
            return;
        }
        let run = |args: &[&str]| {
            let o = Command::new(&binary)
                .args(["--headless", "--path"])
                .arg(&project)
                .args(args)
                .output()
                .unwrap();
            String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)
        };
        run(&["--import"]);
        let output = run(&["-s", "check.gd"]);
        let line = output
            .lines()
            .find_map(|l| l.strip_prefix("RESULT "))
            .unwrap_or_else(|| panic!("{output}"));
        let got: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(got["anims"], serde_json::json!(["wave"]));
        assert_eq!(got["bones"], 2);
        assert_eq!(got["vertices"], got["weights"]);
        assert!(
            got["held"].as_f64().unwrap().abs() < 1e-3,
            "the rest pose holds until the pose's frame"
        );
        assert!((got["posed"].as_f64().unwrap() + 70f64.to_radians()).abs() < 1e-3);
        let planes = got["planes"].as_array().unwrap();
        let (far, near) = (&planes[0], &planes[1]);
        let moved = |p: &serde_json::Value| p[2].as_f64().unwrap();
        let offset = far[1].as_f64().unwrap();
        assert!(
            (moved(far) - offset * 0.75).abs() < 1.0,
            "the far plane scrolls at a quarter of the camera: {planes:?}"
        );
        assert!(moved(near).abs() < 1e-3);
        fs::remove_dir_all(&dir).unwrap();
    }
}
