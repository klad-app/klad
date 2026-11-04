use crate::varint::{self, Reader};
use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    Inside,
    Outside,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    pub keep: Keep,
    pub polygon: Vec<[f32; 2]>,
}

pub fn inside_polygon(polygon: &[[f32; 2]], (x, y): (f32, f32)) -> bool {
    let n = polygon.len();
    (0..n).fold(false, |inside, i| {
        let (a, b) = (polygon[i], polygon[(i + n - 1) % n]);
        if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
            !inside
        } else {
            inside
        }
    })
}

pub fn crossings(polygon: &[[f32; 2]], y: f32) -> Vec<f32> {
    let n = polygon.len();
    let mut xs: Vec<f32> = (0..n)
        .filter_map(|i| {
            let (a, b) = (polygon[i], polygon[(i + n - 1) % n]);
            ((a[1] > y) != (b[1] > y)).then(|| (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0])
        })
        .collect();
    xs.sort_by(f32::total_cmp);
    xs
}

pub fn polygon_box(polygon: &[[f32; 2]]) -> Option<[f32; 4]> {
    let first = polygon.first()?;
    Some(
        polygon
            .iter()
            .fold([first[0], first[1], first[0], first[1]], |[x0, y0, x1, y1], p| {
                [x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1])]
            }),
    )
}

impl Cut {
    pub fn keeps(&self, at: (f32, f32)) -> bool {
        inside_polygon(&self.polygon, at) == (self.keep == Keep::Inside)
    }

    pub fn map(&mut self, f: impl Fn([f32; 2]) -> [f32; 2]) {
        self.polygon.iter_mut().for_each(|p| *p = f(*p));
    }
}

pub fn keeps(cuts: &[Cut], at: (f32, f32)) -> bool {
    cuts.iter().all(|c| c.keeps(at))
}

pub fn clip_bounds(cuts: &[Cut], bounds: [f32; 4]) -> Option<[f32; 4]> {
    cuts.iter()
        .filter(|c| c.keep == Keep::Inside)
        .filter_map(|c| polygon_box(&c.polygon))
        .try_fold(bounds, |[x0, y0, x1, y1], [a0, b0, a1, b1]| {
            let clipped = [x0.max(a0), y0.max(b0), x1.min(a1), y1.min(b1)];
            (clipped[0] < clipped[2] && clipped[1] < clipped[3]).then_some(clipped)
        })
}

pub fn encode(out: &mut Vec<u8>, cuts: &[Cut]) {
    varint::put(out, cuts.len() as u64);
    cuts.iter().for_each(|cut| {
        out.push(match cut.keep {
            Keep::Inside => 0,
            Keep::Outside => 1,
        });
        varint::put(out, cut.polygon.len() as u64);
        cut.polygon
            .iter()
            .flatten()
            .for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    });
}

pub fn decode(reader: &mut Reader) -> Result<Vec<Cut>> {
    let count = reader.varint()? as usize;
    (0..count)
        .map(|_| {
            let keep = match reader.u8()? {
                0 => Keep::Inside,
                1 => Keep::Outside,
                other => bail!("unknown cut side {other}"),
            };
            let n = reader.varint()? as usize;
            let polygon = (0..n)
                .map(|_| Ok([reader.f32()?, reader.f32()?]))
                .collect::<Result<Vec<_>>>()?;
            Ok(Cut { keep, polygon })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(keep: Keep) -> Cut {
        Cut {
            keep,
            polygon: vec![[10.0, 10.0], [50.0, 10.0], [50.0, 50.0], [10.0, 50.0]],
        }
    }

    #[test]
    fn cuts_keep_one_side_and_round_trip() {
        let inside = square(Keep::Inside);
        let outside = square(Keep::Outside);
        assert!(inside.keeps((20.0, 20.0)) && !inside.keeps((60.0, 20.0)));
        assert!(!outside.keeps((20.0, 20.0)) && outside.keeps((60.0, 20.0)));
        assert!(!keeps(&[inside.clone(), outside.clone()], (20.0, 20.0)));
        assert_eq!(
            clip_bounds(std::slice::from_ref(&inside), [0.0, 0.0, 30.0, 100.0]),
            Some([10.0, 10.0, 30.0, 50.0])
        );
        assert_eq!(clip_bounds(std::slice::from_ref(&inside), [60.0, 0.0, 80.0, 5.0]), None);
        assert_eq!(
            clip_bounds(std::slice::from_ref(&outside), [60.0, 0.0, 80.0, 5.0]),
            Some([60.0, 0.0, 80.0, 5.0])
        );
        assert_eq!(crossings(&inside.polygon, 20.0), vec![10.0, 50.0]);
        let mut bytes = Vec::new();
        encode(&mut bytes, &[inside.clone(), outside.clone()]);
        let mut reader = Reader::new(&bytes);
        assert_eq!(decode(&mut reader).unwrap(), vec![inside, outside]);
        assert!(reader.done());
    }
}
