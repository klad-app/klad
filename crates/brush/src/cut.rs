use crate::tiles::{TILE, TILE_PX};
use klad_core::cut::{Cut, Keep, crossings, polygon_box};
use klad_core::rng::{FNV_OFFSET, fnv};
use std::cell::RefCell;
use std::sync::Arc;

const SUB: usize = 4;
const REMEMBERED: usize = 96;

type Key = (u64, usize, usize);
type Inside = Option<Arc<[f32]>>;

thread_local! {
    static INSIDE: RefCell<Vec<(Key, Inside)>> = const { RefCell::new(Vec::new()) };
}

fn remembered(polygon: &[[f32; 2]], at: (usize, usize)) -> Inside {
    let bytes: Vec<u8> = polygon.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    let key = (fnv(FNV_OFFSET, &bytes), at.0, at.1);
    if let Some(found) = INSIDE.with_borrow(|seen| seen.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
        return found;
    }
    let fresh: Inside = inside(polygon, at).map(Arc::from);
    INSIDE.with_borrow_mut(|seen| {
        if seen.len() >= REMEMBERED {
            seen.remove(0);
        }
        seen.push((key, fresh.clone()));
    });
    fresh
}

pub fn inside(polygon: &[[f32; 2]], (ox, oy): (usize, usize)) -> Option<Vec<f32>> {
    let [x0, y0, x1, y1] = polygon_box(polygon)?;
    let (left, top) = (ox as f32, oy as f32);
    let (right, bottom) = (left + TILE as f32, top + TILE as f32);
    if x1 <= left || x0 >= right || y1 <= top || y0 >= bottom {
        return None;
    }
    let rows = (y0 - top).floor().max(0.0) as usize..((y1 - top).ceil().max(0.0) as usize).min(TILE);
    let width = TILE * SUB;
    let first = ((x0 - left).floor().max(0.0) as usize).min(TILE) * SUB;
    let last = (((x1 - left).ceil().max(0.0) as usize).min(TILE) * SUB).max(first);
    let mut out = vec![0.0f32; TILE_PX];
    let mut counts = vec![0i32; width + 1];
    rows.for_each(|row| {
        counts[first..=last].iter_mut().for_each(|c| *c = 0);
        (0..SUB).for_each(|k| {
            let sy = top + row as f32 + (k as f32 + 0.5) / SUB as f32;
            crossings(polygon, sy).as_chunks::<2>().0.iter().for_each(|[a, b]| {
                let sub = |x: f32| ((x - left) * SUB as f32 - 0.5).ceil().clamp(0.0, width as f32) as usize;
                let (from, to) = (sub(*a), sub(*b));
                if from < to {
                    counts[from] += 1;
                    counts[to] -= 1;
                }
            });
        });
        let mut running = 0;
        let mut pixel = 0;
        counts[first..last].iter().enumerate().for_each(|(i, c)| {
            running += c;
            pixel += running;
            if (first + i) % SUB == SUB - 1 {
                out[row * TILE + (first + i) / SUB] = pixel as f32 / (SUB * SUB) as f32;
                pixel = 0;
            }
        });
    });
    Some(out)
}

pub fn factor(cuts: &[Cut], (ox, oy): (usize, usize)) -> Option<Vec<f32>> {
    cuts.iter().fold(None, |acc: Option<Vec<f32>>, cut| {
        let part = match (remembered(&cut.polygon, (ox, oy)), cut.keep) {
            (None, Keep::Outside) => return acc,
            (None, Keep::Inside) => vec![0.0; TILE_PX],
            (Some(inner), Keep::Inside) => inner.to_vec(),
            (Some(inner), Keep::Outside) => inner.iter().map(|v| 1.0 - v).collect(),
        };
        Some(match acc {
            None => part,
            Some(mut all) => {
                all.iter_mut().zip(&part).for_each(|(a, p)| *a *= p);
                all
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insides_are_supersampled_and_cuts_multiply() {
        let square = vec![[10.0, 10.0], [20.5, 10.0], [20.5, 20.0], [10.0, 20.0]];
        let inner = inside(&square, (0, 0)).unwrap();
        assert_eq!(inner[15 * TILE + 15], 1.0);
        assert_eq!(inner[15 * TILE + 25], 0.0);
        assert_eq!(inner[15 * TILE + 20], 0.5);
        assert!(inside(&square, (256, 0)).is_none());
        let keep_out = Cut {
            keep: Keep::Outside,
            polygon: square.clone(),
        };
        let keep_in = Cut {
            keep: Keep::Inside,
            polygon: square,
        };
        assert!(factor(std::slice::from_ref(&keep_out), (256, 0)).is_none());
        assert!(
            factor(std::slice::from_ref(&keep_in), (256, 0))
                .unwrap()
                .iter()
                .all(|v| *v == 0.0)
        );
        let both = factor(&[keep_out.clone(), keep_in.clone()], (0, 0)).unwrap();
        assert_eq!(both[15 * TILE + 20], 0.25);
        assert_eq!(both[15 * TILE + 15], 0.0);
        let outer = factor(std::slice::from_ref(&keep_out), (0, 0)).unwrap();
        let inner = factor(std::slice::from_ref(&keep_in), (0, 0)).unwrap();
        assert!(outer.iter().zip(&inner).all(|(a, b)| a + b == 1.0));
    }
}
