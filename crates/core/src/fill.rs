use crate::color::{Rgba, from_u16, to_u16};
use crate::cut::{self, Cut};
use crate::stroke::Author;
use crate::varint::{self, Reader};
use anyhow::{Result, bail};

pub const TOLERANCE: f32 = 0.1;
pub const EXPAND: f32 = 2.0;
pub const MAX_GAP: f32 = 64.0;
pub const MAX_EXPAND: f32 = 32.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FillRef {
    This,
    All,
    Layer(String),
}

impl FillRef {
    pub fn name(&self) -> &str {
        match self {
            FillRef::This => "this",
            FillRef::All => "all",
            FillRef::Layer(id) => id,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seed {
    pub x: f32,
    pub y: f32,
    pub tolerance: f32,
    pub gap: f32,
    pub expand: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FillSource {
    Seed { seed: Seed, reference: FillRef },
    Polygon(Vec<[f32; 2]>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
}

impl Coverage {
    pub fn from_mask(width: u32, height: u32, mask: &[u8]) -> Self {
        let (w, h) = (width as usize, height as usize);
        let rows: Vec<usize> = (0..h)
            .filter(|&y| mask[y * w..(y + 1) * w].iter().any(|&a| a > 0))
            .collect();
        let (Some(&y0), Some(&y1)) = (rows.first(), rows.last()) else {
            return Self::default();
        };
        let (x0, x1) = rows.iter().fold((w, 0), |(lo, hi), &y| {
            let row = &mask[y * w..(y + 1) * w];
            let first = row.iter().position(|&a| a > 0).unwrap_or(w);
            let last = row.iter().rposition(|&a| a > 0).unwrap_or(0);
            (lo.min(first), hi.max(last))
        });
        let alpha = (y0..=y1)
            .flat_map(|y| mask[y * w + x0..=y * w + x1].iter().copied())
            .collect();
        Self {
            x: x0 as u32,
            y: y0 as u32,
            width: (x1 - x0 + 1) as u32,
            height: (y1 - y0 + 1) as u32,
            alpha,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn pixels(&self) -> usize {
        self.alpha.iter().filter(|&&a| a > 0).count()
    }

    pub fn at(&self, x: u32, y: u32) -> u8 {
        if x < self.x || y < self.y || x >= self.x + self.width || y >= self.y + self.height {
            return 0;
        }
        self.alpha[((y - self.y) * self.width + (x - self.x)) as usize]
    }

    pub fn bounds(&self) -> Option<[f32; 4]> {
        (!self.is_empty()).then(|| {
            [
                self.x as f32,
                self.y as f32,
                (self.x + self.width) as f32,
                (self.y + self.height) as f32,
            ]
        })
    }

    fn encode(&self, out: &mut Vec<u8>) {
        [self.x, self.y, self.width, self.height]
            .iter()
            .for_each(|v| varint::put(out, *v as u64));
        let runs = self.alpha.chunk_by(|a, b| a == b).collect::<Vec<_>>();
        varint::put(out, runs.len() as u64);
        runs.iter().for_each(|run| {
            varint::put(out, run.len() as u64);
            out.push(run[0]);
        });
    }

    fn decode(reader: &mut Reader) -> Result<Self> {
        let mut next = || -> Result<u32> { Ok(u32::try_from(reader.varint()?)?) };
        let (x, y, width, height) = (next()?, next()?, next()?, next()?);
        let total = width as usize * height as usize;
        let runs = reader.varint()? as usize;
        let mut alpha = Vec::with_capacity(total);
        (0..runs).try_for_each(|_| {
            let len = reader.varint()? as usize;
            let value = reader.u8()?;
            if alpha.len() + len > total {
                bail!("fill coverage runs past its {width}x{height} box");
            }
            alpha.resize(alpha.len() + len, value);
            Ok(())
        })?;
        if alpha.len() != total {
            bail!("fill coverage has {} of {total} pixels", alpha.len());
        }
        Ok(Self {
            x,
            y,
            width,
            height,
            alpha,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    pub id: u32,
    pub color: [u16; 4],
    pub opacity: f32,
    pub author: Author,
    pub source: FillSource,
    pub coverage: Coverage,
    pub lock: bool,
    pub cuts: Vec<Cut>,
}

const FILL_LOCK: u8 = 0x80;
const FILL_CUTS: u8 = 0x40;

impl Fill {
    pub fn new(id: u32, color: Rgba, source: FillSource, coverage: Coverage) -> Self {
        Self {
            id,
            color: to_u16(color),
            opacity: 1.0,
            author: Author::Human,
            source,
            coverage,
            lock: false,
            cuts: Vec::new(),
        }
    }

    pub fn by(mut self, author: Author) -> Self {
        self.author = author;
        self
    }

    pub fn color(&self) -> Rgba {
        from_u16(self.color)
    }

    pub fn bounds(&self) -> Option<[f32; 4]> {
        self.coverage.bounds()
    }

    pub fn reference(&self) -> Option<&FillRef> {
        match &self.source {
            FillSource::Seed { reference, .. } => Some(reference),
            FillSource::Polygon(_) => None,
        }
    }
}

fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn encode(out: &mut Vec<u8>, fill: &Fill) {
    varint::put(out, fill.id as u64);
    fill.color.iter().for_each(|c| out.extend_from_slice(&c.to_le_bytes()));
    put_f32(out, fill.opacity);
    out.push(match fill.author {
        Author::Human => 0,
        Author::Agent => 1,
    });
    let flags = (if fill.lock { FILL_LOCK } else { 0 }) | (if fill.cuts.is_empty() { 0 } else { FILL_CUTS });
    match &fill.source {
        FillSource::Seed { seed, reference } => {
            out.push(flags);
            put_f32(out, seed.x);
            put_f32(out, seed.y);
            match reference {
                FillRef::This => out.push(0),
                FillRef::All => out.push(1),
                FillRef::Layer(id) => {
                    out.push(2);
                    varint::put(out, id.len() as u64);
                    out.extend_from_slice(id.as_bytes());
                }
            }
            [seed.tolerance, seed.gap, seed.expand]
                .iter()
                .for_each(|v| put_f32(out, *v));
        }
        FillSource::Polygon(points) => {
            out.push(1 | flags);
            varint::put(out, points.len() as u64);
            points.iter().flatten().for_each(|v| put_f32(out, *v));
        }
    }
    fill.coverage.encode(out);
    if !fill.cuts.is_empty() {
        cut::encode(out, &fill.cuts);
    }
}

pub(crate) fn decode(reader: &mut Reader) -> Result<Fill> {
    let id = u32::try_from(reader.varint()?)?;
    let color = [reader.u16()?, reader.u16()?, reader.u16()?, reader.u16()?];
    let opacity = reader.f32()?;
    let author = if reader.u8()? == 1 {
        Author::Agent
    } else {
        Author::Human
    };
    let tag = reader.u8()?;
    let flags = tag & (FILL_LOCK | FILL_CUTS);
    let source = match tag & !flags {
        0 => {
            let (x, y) = (reader.f32()?, reader.f32()?);
            let reference = match reader.u8()? {
                0 => FillRef::This,
                1 => FillRef::All,
                2 => {
                    let len = reader.varint()? as usize;
                    FillRef::Layer(String::from_utf8(reader.take(len)?.to_vec())?)
                }
                other => bail!("unknown fill reference {other}"),
            };
            let (tolerance, gap, expand) = (reader.f32()?, reader.f32()?, reader.f32()?);
            FillSource::Seed {
                seed: Seed {
                    x,
                    y,
                    tolerance,
                    gap,
                    expand,
                },
                reference,
            }
        }
        1 => {
            let n = reader.varint()? as usize;
            FillSource::Polygon(
                (0..n)
                    .map(|_| Ok([reader.f32()?, reader.f32()?]))
                    .collect::<Result<_>>()?,
            )
        }
        other => bail!("unknown fill source {other}"),
    };
    let coverage = Coverage::decode(reader)?;
    let cuts = if flags & FILL_CUTS != 0 {
        cut::decode(reader)?
    } else {
        Vec::new()
    };
    Ok(Fill {
        id,
        color,
        opacity,
        author,
        source,
        coverage,
        lock: flags & FILL_LOCK != 0,
        cuts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_crop_to_their_bounds() {
        let (w, h) = (8u32, 6u32);
        let mut mask = vec![0u8; (w * h) as usize];
        [(2, 1), (5, 1), (3, 4)]
            .iter()
            .for_each(|&(x, y)| mask[y * 8 + x] = 255);
        mask[2 * 8 + 4] = 128;
        let c = Coverage::from_mask(w, h, &mask);
        assert_eq!((c.x, c.y, c.width, c.height), (2, 1, 4, 4));
        assert_eq!((c.at(5, 1), c.at(4, 2), c.at(0, 0), c.at(7, 5)), (255, 128, 0, 0));
        assert_eq!(c.pixels(), 4);
        assert_eq!(c.bounds(), Some([2.0, 1.0, 6.0, 5.0]));
        assert!(Coverage::from_mask(4, 4, &[0; 16]).is_empty());
    }

    #[test]
    fn fills_round_trip_with_every_source() {
        let mut mask = vec![0u8; 64 * 64];
        (10..40).for_each(|y| (5..60).for_each(|x| mask[y * 64 + x] = if x == 5 { 90 } else { 255 }));
        let coverage = Coverage::from_mask(64, 64, &mask);
        let seed = Seed {
            x: 20.5,
            y: 30.0,
            tolerance: 0.1,
            gap: 4.0,
            expand: 2.0,
        };
        let fills = [
            Fill::new(
                3,
                [0.2, 0.4, 0.6, 1.0],
                FillSource::Seed {
                    seed,
                    reference: FillRef::Layer("lines".into()),
                },
                coverage.clone(),
            )
            .by(Author::Agent),
            Fill::new(
                4,
                [1.0, 0.0, 0.0, 0.5],
                FillSource::Seed {
                    seed,
                    reference: FillRef::All,
                },
                Coverage::default(),
            ),
            Fill::new(
                5,
                [0.0, 0.0, 0.0, 1.0],
                FillSource::Polygon(vec![[1.0, 1.0], [30.0, 2.5], [9.0, 40.0]]),
                coverage,
            ),
        ];
        let mut locked = fills[2].clone();
        locked.lock = true;
        locked.cuts = vec![Cut {
            keep: cut::Keep::Inside,
            polygon: vec![[0.0, 0.0], [20.0, 0.0], [0.0, 20.0]],
        }];
        fills.iter().chain([&locked]).for_each(|fill| {
            let mut bytes = Vec::new();
            encode(&mut bytes, fill);
            let mut reader = Reader::new(&bytes);
            assert_eq!(&decode(&mut reader).unwrap(), fill);
            assert!(reader.done());
        });
        let mut bytes = Vec::new();
        encode(&mut bytes, &fills[0]);
        assert!(bytes.len() < 200, "{} bytes", bytes.len());
    }
}
