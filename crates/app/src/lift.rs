use anyhow::{Result, anyhow};
use half::f16;
use klad_brush::{BrushSet, CelReplay, Raster, TILE};
use klad_core::doc::{Node, Transform};
use klad_core::markedit::union;
use klad_core::rng::{FNV_OFFSET, fnv};
use klad_core::stroke::Mark;
use klad_render::{Lifted, rasterize_nodes, rasterize_region, vector};
use std::sync::mpsc::{Receiver, channel};

pub struct Prepared {
    pub key: String,
    pub rev: u64,
    pub cel: String,
    pub layer: Vec<usize>,
    pub index: usize,
    pub nodes: Vec<Node>,
    pub lifted: Lifted,
    pub lower: Raster,
    pub upper: Raster,
}

pub enum Source {
    Node {
        index: usize,
        nodes: Vec<Node>,
    },
    Marks {
        ids: Vec<u32>,
        marks: Option<Vec<Mark>>,
        replay: Box<CelReplay>,
        raster: Raster,
        brushes: Box<BrushSet>,
    },
}

pub struct Job {
    pub key: String,
    pub rev: u64,
    pub cel: String,
    pub layer: Vec<usize>,
    pub source: Source,
    pub canvas: (u32, u32),
    pub scale: f32,
}

fn spawn<T: Send + 'static>(
    ctx: egui::Context,
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Receiver<Result<T>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
        ctx.request_repaint();
    });
    rx
}

pub fn crop(raster: &Raster, [x, y, w, h]: [f32; 4], max_side: u32) -> Lifted {
    let (x0, y0) = (x.floor().max(0.0) as u32, y.floor().max(0.0) as u32);
    let x1 = ((x + w).ceil().max(0.0) as u32).min(raster.width).max(x0 + 1);
    let y1 = ((y + h).ceil().max(0.0) as u32).min(raster.height).max(y0 + 1);
    let step = (x1 - x0).max(y1 - y0).div_ceil(max_side).max(1);
    let (width, height) = ((x1 - x0).div_ceil(step), (y1 - y0).div_ceil(step));
    let pixel = |px: u32, py: u32| -> [f16; 4] {
        if px >= raster.width || py >= raster.height {
            return [f16::ZERO; 4];
        }
        let (px, py) = (px as usize, py as usize);
        raster.get(px / TILE, py / TILE).map_or([f16::ZERO; 4], |tile| {
            let at = ((py % TILE) * TILE + px % TILE) * 4;
            [tile[at], tile[at + 1], tile[at + 2], tile[at + 3]]
        })
    };
    let pixels = (0..height)
        .flat_map(|j| (0..width).map(move |i| (i, j)))
        .flat_map(|(i, j)| pixel(x0 + i * step, y0 + j * step))
        .collect();
    Lifted {
        rect: [x0 as f32, y0 as f32, (width * step) as f32, (height * step) as f32],
        width,
        height,
        pixels,
    }
}

pub fn prepare(job: Job, ctx: egui::Context) -> Receiver<Result<Prepared>> {
    spawn(ctx, move || {
        let (cw, ch) = job.canvas;
        let (index, nodes, lifted, lower, upper) = match job.source {
            Source::Node { index, nodes } => {
                let target = nodes.get(index).ok_or_else(|| anyhow!("node index out of range"))?;
                let [x, y, w, h] =
                    vector::node_box(target)?.ok_or_else(|| anyhow!("{} has nothing to draw", job.key))?;
                let lifted = rasterize_region(
                    std::slice::from_ref(target),
                    [x - 2.0, y - 2.0, w + 4.0, h + 4.0],
                    4096,
                    job.scale,
                )?;
                let lower = rasterize_nodes(&nodes[..index], cw, ch)?;
                let upper = rasterize_nodes(&nodes[index + 1..], cw, ch)?;
                (index, nodes, lifted, lower, upper)
            }
            Source::Marks {
                ids,
                marks,
                mut replay,
                mut raster,
                brushes,
            } => {
                let all = marks.unwrap_or_else(|| replay.marks().to_vec());
                let (chosen, rest): (Vec<Mark>, Vec<Mark>) = all.into_iter().partition(|m| ids.contains(&m.id()));
                let [x0, y0, x1, y1] = union(&chosen.iter().collect::<Vec<_>>())
                    .ok_or_else(|| anyhow!("the selected strokes have nothing to draw"))?;
                let (_, alone, _) = CelReplay::replay(cw, ch, chosen, &brushes)?;
                let lifted = crop(&alone, [x0 - 2.0, y0 - 2.0, x1 - x0 + 4.0, y1 - y0 + 4.0], 4096);
                replay.update(&mut raster, rest, &brushes)?;
                (0, Vec::new(), lifted, raster, Raster::new(cw, ch))
            }
        };
        Ok(Prepared {
            key: job.key,
            rev: job.rev,
            cel: job.cel,
            layer: job.layer,
            index,
            nodes,
            lifted,
            lower,
            upper,
        })
    })
}

pub fn render(nodes: Vec<Node>, canvas: (u32, u32), ctx: egui::Context) -> Receiver<Result<(u64, Raster)>> {
    spawn(ctx, move || {
        let hash = fnv(FNV_OFFSET, &serde_json::to_vec(&nodes)?);
        Ok((hash, rasterize_nodes(&nodes, canvas.0, canvas.1)?))
    })
}

pub fn finish(
    mut nodes: Vec<Node>,
    index: usize,
    transform: Transform,
    canvas: (u32, u32),
    ctx: egui::Context,
) -> Receiver<Result<(u64, Raster)>> {
    if let Some(node) = nodes.get_mut(index) {
        node.transform = transform;
    }
    render(nodes, canvas, ctx)
}

pub fn affine_apply(a: [f32; 6], (x, y): (f32, f32)) -> (f32, f32) {
    (a[0] * x + a[1] * y + a[4], a[2] * x + a[3] * y + a[5])
}

pub fn committed(node: &Node, affine: [f32; 6], scale: f32, degrees: f32) -> Result<Transform> {
    let mut t = node.transform;
    let pivot = match t.pivot {
        Some(p) => p,
        None => {
            let mut plain = node.clone();
            plain.transform = Transform::default();
            vector::local_bounds(&plain)?.map_or([0.0, 0.0], |r| [r.center_x(), r.center_y()])
        }
    };
    let (qx, qy) = affine_apply(affine, (t.x + pivot[0], t.y + pivot[1]));
    t.x = qx - pivot[0];
    t.y = qy - pivot[1];
    t.sx *= scale;
    t.sy *= scale;
    t.rotate += degrees;
    Ok(t)
}

pub fn square(node: &Node) -> bool {
    let r = node.transform.rotate.rem_euclid(90.0);
    !(0.5..=89.5).contains(&r)
}

pub fn stretched(node: &Node, affine: [f32; 6], axis: usize, k: f32) -> Result<Transform> {
    let mut t = committed(node, affine, 1.0, 0.0)?;
    let quarter = (t.rotate / 90.0).round() as i64;
    match (axis + quarter.rem_euclid(2) as usize) % 2 {
        0 => t.sx *= k,
        _ => t.sy *= k,
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::Shape;

    #[test]
    fn crops_copy_pixels_across_tiles_and_shrink_big_regions() {
        let mut raster = Raster::new(600, 300);
        raster.ensure(raster.index(1, 0))[((10 * TILE) + 3) * 4 + 3] = f16::ONE;
        let lifted = crop(&raster, [250.0, 5.0, 20.0, 10.0], 4096);
        assert_eq!((lifted.width, lifted.height), (20, 10));
        assert_eq!(lifted.rect, [250.0, 5.0, 20.0, 10.0]);
        assert_eq!(lifted.pixels[(5 * 20 + 9) * 4 + 3], f16::ONE);
        let small = crop(&raster, [0.0, 0.0, 600.0, 300.0], 100);
        assert_eq!((small.width, small.height), (100, 50));
        assert_eq!(small.rect, [0.0, 0.0, 600.0, 300.0]);
    }

    #[test]
    fn stretching_a_square_shape_scales_its_own_axis_and_keeps_the_far_edge() {
        let mut node = Node::new(
            "r",
            Shape::Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 50.0,
                radius: 0.0,
            },
        );
        assert!(square(&node));
        let t = stretched(&node, [2.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0, 2.0).unwrap();
        assert_eq!((t.sx, t.sy, t.x, t.y), (2.0, 1.0, 50.0, 0.0));
        node.transform.rotate = 90.0;
        let t = stretched(&node, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0, 3.0).unwrap();
        assert_eq!((t.sx, t.sy), (1.0, 3.0));
        node.transform.rotate = 30.0;
        assert!(!square(&node));
    }

    #[test]
    fn committed_transforms_follow_the_preview() {
        let node = Node::new(
            "n",
            Shape::Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 50.0,
                radius: 0.0,
            },
        );
        let moved = committed(&node, [1.0, 0.0, 0.0, 1.0, 30.0, -10.0], 1.0, 0.0).unwrap();
        assert_eq!((moved.x, moved.y), (30.0, -10.0));
        let (c, k) = ((50.0, 25.0), 2.0);
        let scaled = committed(&node, [k, 0.0, 0.0, k, c.0 * (1.0 - k), c.1 * (1.0 - k)], k, 0.0).unwrap();
        assert_eq!((scaled.x, scaled.y, scaled.sx), (0.0, 0.0, 2.0));
        let mut pinned = node.clone();
        pinned.transform.pivot = Some([0.0, 50.0]);
        let grown = committed(&pinned, [k, 0.0, 0.0, k, c.0 * (1.0 - k), c.1 * (1.0 - k)], k, 0.0).unwrap();
        let m = klad_render::vector::node_matrix(&Node {
            transform: grown,
            ..pinned.clone()
        })
        .unwrap();
        let corner = m.map_point((100.0, 50.0));
        assert!(
            (corner.x - 150.0).abs() < 0.01 && (corner.y - 75.0).abs() < 0.01,
            "{corner:?}"
        );
    }
}
