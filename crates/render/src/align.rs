use crate::overlay;
use anyhow::{Result, anyhow};
use klad_core::doc::Document;
use klad_core::ops::Op;
use serde_json::{Map, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Across {
    Left,
    Centre,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Down {
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum To {
    #[default]
    Selection,
    Canvas,
    Shape(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub x: Option<Across>,
    pub y: Option<Down>,
    pub spread: Option<Axis>,
    pub gap: Option<f32>,
    pub to: To,
}

fn union(boxes: &[[f32; 4]]) -> [f32; 4] {
    let low = |at: usize| boxes.iter().map(|b| b[at]).fold(f32::MAX, f32::min);
    let high = |at: usize| boxes.iter().map(|b| b[at] + b[at + 2]).fold(f32::MIN, f32::max);
    [low(0), low(1), high(0) - low(0), high(1) - low(1)]
}

fn spread(boxes: &[[f32; 4]], at: usize, gap: Option<f32>, within: Option<[f32; 4]>) -> Vec<f32> {
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|a, b| boxes[*a][at].total_cmp(&boxes[*b][at]).then(a.cmp(b)));
    let all = within.unwrap_or_else(|| union(boxes));
    let sizes: f32 = boxes.iter().map(|b| b[at + 2]).sum();
    let between = gap.unwrap_or_else(|| (all[at + 2] - sizes) / (boxes.len().max(2) - 1) as f32);
    let start = match (gap, within) {
        (Some(_), None) => boxes[order[0]][at],
        _ => all[at],
    };
    let mut moves = vec![0.0; boxes.len()];
    order.iter().fold(start, |edge, i| {
        moves[*i] = edge - boxes[*i][at];
        edge + boxes[*i][at + 2] + between
    });
    moves
}

pub fn moves(boxes: &[[f32; 4]], plan: &Plan, frame: Option<[f32; 4]>) -> Result<Vec<[f32; 2]>> {
    if plan.x.is_none() && plan.y.is_none() && plan.spread.is_none() {
        return Err(anyhow!(
            "say how: --x left|centre|right, --y top|middle|bottom or --spread x|y"
        ));
    }
    if boxes.is_empty() {
        return Err(anyhow!("give the shapes to align, by id"));
    }
    if plan.gap.is_some() && plan.spread.is_none() {
        return Err(anyhow!("--gap goes with --spread x or --spread y"));
    }
    let clash = match plan.spread {
        Some(Axis::X) => plan.x.is_some(),
        Some(Axis::Y) => plan.y.is_some(),
        None => false,
    };
    if clash {
        return Err(anyhow!(
            "aligning and spreading along the same direction cancel out: align on one, spread on the other"
        ));
    }
    let aligning = plan.x.is_some() || plan.y.is_some();
    if aligning && frame.is_none() && boxes.len() < 2 {
        return Err(anyhow!(
            "one shape has nothing to line up with: give two or more, or align it to the canvas or to another shape"
        ));
    }
    let least = if plan.gap.is_some() || frame.is_some() { 2 } else { 3 };
    if plan.spread.is_some() && boxes.len() < least {
        return Err(anyhow!(
            "spreading needs {least} shapes or more: the outer two stay and the ones between move"
        ));
    }
    let all = frame.unwrap_or_else(|| union(boxes));
    let across = |b: &[f32; 4]| match plan.x {
        Some(Across::Left) => all[0] - b[0],
        Some(Across::Centre) => all[0] + all[2] / 2.0 - (b[0] + b[2] / 2.0),
        Some(Across::Right) => all[0] + all[2] - (b[0] + b[2]),
        None => 0.0,
    };
    let down = |b: &[f32; 4]| match plan.y {
        Some(Down::Top) => all[1] - b[1],
        Some(Down::Middle) => all[1] + all[3] / 2.0 - (b[1] + b[3] / 2.0),
        Some(Down::Bottom) => all[1] + all[3] - (b[1] + b[3]),
        None => 0.0,
    };
    let spaced = plan.spread.map(|axis| {
        (
            axis,
            spread(boxes, if axis == Axis::X { 0 } else { 1 }, plan.gap, frame),
        )
    });
    Ok(boxes
        .iter()
        .enumerate()
        .map(|(i, b)| match &spaced {
            Some((Axis::X, by)) => [by[i], down(b)],
            Some((Axis::Y, by)) => [across(b), by[i]],
            None => [across(b), down(b)],
        })
        .collect())
}

fn tidy(v: f32) -> f64 {
    (f64::from(v) * 1000.0).round() / 1000.0
}

pub fn ops(doc: &Document, ids: &[String], plan: &Plan) -> Result<Vec<Op>> {
    let world = |id: &str| {
        overlay::node_world_box(doc, id)?.ok_or_else(|| {
            anyhow!("{id} is not a shape with something to align: give shape ids, as klad ls prints them")
        })
    };
    let boxes = ids.iter().map(|id| world(id)).collect::<Result<Vec<_>>>()?;
    let frame = match &plan.to {
        To::Selection => None,
        To::Canvas => Some([0.0, 0.0, doc.canvas.width as f32, doc.canvas.height as f32]),
        To::Shape(id) => Some(world(id)?),
    };
    let moved = moves(&boxes, plan, frame)?;
    ids.iter()
        .zip(moved)
        .filter(|(_, by)| by[0].abs() > 0.0005 || by[1].abs() > 0.0005)
        .map(|(id, by)| {
            let (node, parent) = overlay::node_world(doc, id)?.ok_or_else(|| anyhow!("no shape {id}"))?;
            let local = match parent.invert() {
                Some(inverse) => inverse.map_vector((by[0], by[1])),
                None => return Err(anyhow!("{id} sits in a group scaled to nothing and cannot be moved")),
            };
            let props = [
                ("transform.x", json!(tidy(node.transform.x + local.x))),
                ("transform.y", json!(tidy(node.transform.y + local.y))),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect::<Map<_, _>>();
            Ok(Op::Set { id: id.clone(), props })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::{Layer, Node, Shape};

    const BOXES: [[f32; 4]; 3] = [
        [10.0, 10.0, 20.0, 20.0],
        [100.0, 40.0, 40.0, 10.0],
        [50.0, 80.0, 10.0, 30.0],
    ];

    fn plan() -> Plan {
        Plan::default()
    }

    #[test]
    fn edges_line_up_on_the_bounds_of_the_shapes_together() {
        let left = moves(
            &BOXES,
            &Plan {
                x: Some(Across::Left),
                ..plan()
            },
            None,
        )
        .unwrap();
        assert_eq!(left, vec![[0.0, 0.0], [-90.0, 0.0], [-40.0, 0.0]]);
        let right = moves(
            &BOXES,
            &Plan {
                x: Some(Across::Right),
                ..plan()
            },
            None,
        )
        .unwrap();
        assert_eq!(right, vec![[110.0, 0.0], [0.0, 0.0], [80.0, 0.0]]);
        let middle = moves(
            &BOXES,
            &Plan {
                y: Some(Down::Middle),
                ..plan()
            },
            None,
        )
        .unwrap();
        assert_eq!(middle, vec![[0.0, 40.0], [0.0, 15.0], [0.0, -35.0]]);
    }

    #[test]
    fn one_shape_centres_on_the_canvas() {
        let plan = Plan {
            x: Some(Across::Centre),
            y: Some(Down::Middle),
            ..plan()
        };
        let moved = moves(&BOXES[..1], &plan, Some([0.0, 0.0, 400.0, 200.0])).unwrap();
        assert_eq!(moved, vec![[180.0, 80.0]]);
        assert!(moves(&BOXES[..1], &plan, None).is_err());
    }

    #[test]
    fn spreading_keeps_the_outer_two_and_evens_the_gaps() {
        let moved = moves(
            &BOXES,
            &Plan {
                spread: Some(Axis::X),
                ..plan()
            },
            None,
        )
        .unwrap();
        assert_eq!(moved, vec![[0.0, 0.0], [0.0, 0.0], [10.0, 0.0]]);
        let gaps = [60.0 - 30.0, 100.0 - 70.0];
        assert_eq!(gaps[0], gaps[1]);
        let fixed = moves(
            &BOXES,
            &Plan {
                spread: Some(Axis::Y),
                gap: Some(5.0),
                ..plan()
            },
            None,
        )
        .unwrap();
        assert_eq!(fixed, vec![[0.0, 0.0], [0.0, -5.0], [0.0, -30.0]]);
    }

    #[test]
    fn aligning_and_spreading_one_direction_is_refused_and_two_combine() {
        let both = Plan {
            x: Some(Across::Left),
            spread: Some(Axis::X),
            ..plan()
        };
        assert!(moves(&BOXES, &both, None).is_err());
        let row = Plan {
            y: Some(Down::Top),
            spread: Some(Axis::X),
            ..plan()
        };
        assert_eq!(
            moves(&BOXES, &row, None).unwrap(),
            vec![[0.0, 0.0], [0.0, -30.0], [10.0, -70.0]]
        );
        assert!(
            moves(
                &BOXES[..2],
                &Plan {
                    spread: Some(Axis::X),
                    ..plan()
                },
                None
            )
            .is_err()
        );
        assert!(
            moves(
                &BOXES,
                &Plan {
                    gap: Some(4.0),
                    ..plan()
                },
                None
            )
            .is_err()
        );
        assert!(moves(&BOXES, &plan(), None).is_err());
    }

    fn rect(id: &str, x: f32, y: f32, w: f32, h: f32) -> Node {
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
    }

    #[test]
    fn a_shape_in_a_scaled_group_moves_by_the_distance_seen_on_the_canvas() {
        let mut doc = Document::new(400, 200);
        let mut group = Node::new(
            "g",
            Shape::Group {
                children: vec![rect("inner", 0.0, 0.0, 10.0, 10.0)],
            },
        );
        group.transform.sx = 2.0;
        group.transform.sy = 2.0;
        group.transform.pivot = Some([0.0, 0.0]);
        doc.layers.push(Layer::vector(
            "l1",
            "shapes",
            "c1",
            vec![group, rect("free", 100.0, 50.0, 20.0, 20.0)],
        ));
        let plan = Plan {
            x: Some(Across::Left),
            to: To::Shape("free".into()),
            ..plan()
        };
        let ops = ops(&doc, &["inner".to_string()], &plan).unwrap();
        let Op::Set { id, props } = &ops[0] else {
            panic!("a set")
        };
        assert_eq!(id, "inner");
        assert_eq!(props["transform.x"], json!(50.0));
        assert_eq!(props["transform.y"], json!(0.0));
        assert!(super::ops(&doc, &["nothing".to_string()], &plan).is_err());
    }
}
