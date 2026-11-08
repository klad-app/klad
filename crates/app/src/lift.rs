use anyhow::Result;
use klad_brush::Raster;
use klad_core::doc::{Node, Transform};
use klad_core::rng::{FNV_OFFSET, fnv};
use klad_render::rasterize_nodes;
use std::sync::mpsc::{Receiver, channel};

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

pub fn render(nodes: Vec<Node>, canvas: (u32, u32), ctx: egui::Context) -> Receiver<Result<(u64, Raster)>> {
    spawn(ctx, move || {
        let hash = fnv(FNV_OFFSET, &serde_json::to_vec(&nodes)?);
        Ok((hash, rasterize_nodes(&nodes, canvas.0, canvas.1)?))
    })
}

pub fn affine_apply(a: [f32; 6], (x, y): (f32, f32)) -> (f32, f32) {
    (a[0] * x + a[1] * y + a[4], a[2] * x + a[3] * y + a[5])
}

pub fn committed(node: &Node, affine: [f32; 6], scale: f32, degrees: f32) -> Result<Transform> {
    klad_render::shift::moved_node(node, affine, scale, degrees)
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
