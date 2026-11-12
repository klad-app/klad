use crate::color::{Rgba, from_u16, to_u16};
use crate::cut::{self, Cut};
use crate::fill::{self, Fill};
use crate::varint::{self, Reader};
use anyhow::{Result, bail};

pub const POS_SCALE: f32 = 16.0;
pub const PRESSURE_STEPS: f32 = 4095.0;
pub const TILT_STEPS: f32 = 127.0;
pub const ZSTD_LEVEL: i32 = 3;
const MAGIC: &[u8; 4] = b"KLST";
const OLD_MAGIC: &[u8; 4] = b"DAST";
const VERSION: u8 = 3;
const READS: [u8; 3] = [1, 2, 3];
const MARK_STROKE: u8 = 0;
const MARK_FILL: u8 = 1;
const HAS_TILT: u8 = 1;
const HAS_TIME: u8 = 2;
const HAS_TAPER: u8 = 4;
const HAS_LOCK: u8 = 8;
const HAS_CUTS: u8 = 16;
const HAS_SECOND: u8 = 32;
const HAS_ON: u8 = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    pub x: i32,
    pub y: i32,
    pub pressure: u16,
    pub tilt_x: i8,
    pub tilt_y: i8,
    pub dt_ms: u16,
}

impl Sample {
    pub fn quantize(x: f32, y: f32, pressure: f32, tilt: (f32, f32), dt_ms: f32) -> Self {
        Self {
            x: (x * POS_SCALE).round() as i32,
            y: (y * POS_SCALE).round() as i32,
            pressure: (pressure.clamp(0.0, 1.0) * PRESSURE_STEPS).round() as u16,
            tilt_x: (tilt.0.clamp(-1.0, 1.0) * TILT_STEPS).round() as i8,
            tilt_y: (tilt.1.clamp(-1.0, 1.0) * TILT_STEPS).round() as i8,
            dt_ms: dt_ms.round().clamp(0.0, u16::MAX as f32) as u16,
        }
    }

    pub fn pos(&self) -> (f32, f32) {
        (self.x as f32 / POS_SCALE, self.y as f32 / POS_SCALE)
    }

    pub fn pressure(&self) -> f32 {
        self.pressure as f32 / PRESSURE_STEPS
    }

    pub fn tilt(&self) -> (f32, f32) {
        (self.tilt_x as f32 / TILT_STEPS, self.tilt_y as f32 / TILT_STEPS)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Author {
    #[default]
    Human,
    Agent,
}

impl Author {
    pub fn name(self) -> &'static str {
        match self {
            Author::Human => "human",
            Author::Agent => "agent",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub id: u32,
    pub brush: String,
    pub color: [u16; 4],
    pub size: f32,
    pub opacity: f32,
    pub flow: f32,
    pub seed: u32,
    pub author: Author,
    pub taper: [f32; 2],
    pub lock: bool,
    pub cuts: Vec<Cut>,
    pub second: Option<[u16; 4]>,
    pub on: Vec<u32>,
    pub samples: Vec<Sample>,
}

impl Stroke {
    pub fn new(id: u32, brush: &str, color: Rgba, size: f32, seed: u32) -> Self {
        Self {
            id,
            brush: brush.to_string(),
            color: to_u16(color),
            size,
            opacity: 1.0,
            flow: 1.0,
            seed,
            author: Author::Human,
            taper: [0.0, 0.0],
            lock: false,
            cuts: Vec::new(),
            second: None,
            on: Vec::new(),
            samples: Vec::new(),
        }
    }

    pub fn tapered(mut self, taper: [f32; 2]) -> Self {
        self.taper = taper;
        self
    }

    pub fn pressureless(&self) -> bool {
        self.samples.iter().all(|s| s.pressure == 0)
    }

    pub fn by(mut self, author: Author) -> Self {
        self.author = author;
        self
    }

    pub fn with_points(mut self, points: &[(f32, f32, f32)], dt_ms: f32) -> Self {
        self.samples = points
            .iter()
            .map(|&(x, y, p)| Sample::quantize(x, y, p, (0.0, 0.0), dt_ms))
            .collect();
        self
    }

    pub fn color(&self) -> Rgba {
        from_u16(self.color)
    }

    pub fn second_color(&self) -> Option<Rgba> {
        self.second.map(from_u16)
    }

    pub fn bounds(&self) -> Option<[f32; 4]> {
        let first = self.samples.first()?.pos();
        Some(
            self.samples
                .iter()
                .map(Sample::pos)
                .fold([first.0, first.1, first.0, first.1], |[x0, y0, x1, y1], (x, y)| {
                    [x0.min(x), y0.min(y), x1.max(x), y1.max(y)]
                }),
        )
    }

    fn channels(&self) -> u8 {
        let tilt = self.samples.iter().any(|s| s.tilt_x != 0 || s.tilt_y != 0);
        let time = self.samples.iter().any(|s| s.dt_ms != 0);
        let taper = self.taper != [0.0, 0.0];
        [
            (tilt, HAS_TILT),
            (time, HAS_TIME),
            (taper, HAS_TAPER),
            (self.lock, HAS_LOCK),
            (!self.cuts.is_empty(), HAS_CUTS),
            (self.second.is_some(), HAS_SECOND),
            (!self.on.is_empty(), HAS_ON),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .fold(0, |bits, (_, bit)| bits | bit)
    }
}

fn put_column(out: &mut Vec<u8>, values: impl Iterator<Item = i64>) {
    values.fold(0i64, |prev, v| {
        varint::put_signed(out, v - prev);
        v
    });
}

fn read_column(reader: &mut Reader, n: usize) -> Result<Vec<i64>> {
    let mut prev = 0i64;
    (0..n)
        .map(|_| {
            prev += reader.signed()?;
            Ok(prev)
        })
        .collect()
}

fn encode_stroke(out: &mut Vec<u8>, stroke: &Stroke) {
    let channels = stroke.channels();
    varint::put(out, stroke.id as u64);
    varint::put(out, stroke.brush.len() as u64);
    out.extend_from_slice(stroke.brush.as_bytes());
    stroke
        .color
        .iter()
        .for_each(|c| out.extend_from_slice(&c.to_le_bytes()));
    [stroke.size, stroke.opacity, stroke.flow]
        .iter()
        .for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    out.extend_from_slice(&stroke.seed.to_le_bytes());
    out.push(channels);
    out.push(match stroke.author {
        Author::Human => 0,
        Author::Agent => 1,
    });
    if channels & HAS_TAPER != 0 {
        stroke
            .taper
            .iter()
            .for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    }
    if channels & HAS_CUTS != 0 {
        cut::encode(out, &stroke.cuts);
    }
    if let Some(second) = stroke.second {
        second.iter().for_each(|c| out.extend_from_slice(&c.to_le_bytes()));
    }
    if channels & HAS_ON != 0 {
        varint::put(out, stroke.on.len() as u64);
        put_column(out, stroke.on.iter().map(|&id| id as i64));
    }
    varint::put(out, stroke.samples.len() as u64);
    let s = &stroke.samples;
    put_column(out, s.iter().map(|v| v.x as i64));
    put_column(out, s.iter().map(|v| v.y as i64));
    put_column(out, s.iter().map(|v| v.pressure as i64));
    if channels & HAS_TILT != 0 {
        put_column(out, s.iter().map(|v| v.tilt_x as i64));
        put_column(out, s.iter().map(|v| v.tilt_y as i64));
    }
    if channels & HAS_TIME != 0 {
        put_column(out, s.iter().map(|v| v.dt_ms as i64));
    }
}

fn decode_stroke(reader: &mut Reader) -> Result<Stroke> {
    let id = u32::try_from(reader.varint()?)?;
    let name_len = reader.varint()? as usize;
    let brush = String::from_utf8(reader.take(name_len)?.to_vec())?;
    let color = [reader.u16()?, reader.u16()?, reader.u16()?, reader.u16()?];
    let size = reader.f32()?;
    let opacity = reader.f32()?;
    let flow = reader.f32()?;
    let seed = reader.u32()?;
    let channels = reader.u8()?;
    let author = if reader.u8()? == 1 {
        Author::Agent
    } else {
        Author::Human
    };
    let taper = if channels & HAS_TAPER != 0 {
        [reader.f32()?, reader.f32()?]
    } else {
        [0.0, 0.0]
    };
    let lock = channels & HAS_LOCK != 0;
    let cuts = if channels & HAS_CUTS != 0 {
        cut::decode(reader)?
    } else {
        Vec::new()
    };
    let second = if channels & HAS_SECOND != 0 {
        Some([reader.u16()?, reader.u16()?, reader.u16()?, reader.u16()?])
    } else {
        None
    };
    let on = match channels & HAS_ON != 0 {
        true => {
            let count = reader.varint()? as usize;
            read_column(reader, count)?
                .into_iter()
                .map(|id| Ok(u32::try_from(id)?))
                .collect::<Result<Vec<_>>>()?
        }
        false => Vec::new(),
    };
    let n = reader.varint()? as usize;
    let xs = read_column(reader, n)?;
    let ys = read_column(reader, n)?;
    let ps = read_column(reader, n)?;
    let zeros = || vec![0i64; n];
    let (tx, ty) = if channels & HAS_TILT != 0 {
        (read_column(reader, n)?, read_column(reader, n)?)
    } else {
        (zeros(), zeros())
    };
    let dts = if channels & HAS_TIME != 0 {
        read_column(reader, n)?
    } else {
        zeros()
    };
    let samples = (0..n)
        .map(|i| {
            Ok(Sample {
                x: i32::try_from(xs[i])?,
                y: i32::try_from(ys[i])?,
                pressure: u16::try_from(ps[i])?,
                tilt_x: i8::try_from(tx[i])?,
                tilt_y: i8::try_from(ty[i])?,
                dt_ms: u16::try_from(dts[i])?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Stroke {
        id,
        brush,
        color,
        size,
        opacity,
        flow,
        seed,
        author,
        taper,
        lock,
        cuts,
        second,
        on,
        samples,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Stroke(Stroke),
    Fill(Fill),
}

impl From<Stroke> for Mark {
    fn from(stroke: Stroke) -> Self {
        Mark::Stroke(stroke)
    }
}

impl From<Fill> for Mark {
    fn from(fill: Fill) -> Self {
        Mark::Fill(fill)
    }
}

impl Mark {
    pub fn color(&self) -> Rgba {
        match self {
            Mark::Stroke(s) => s.color(),
            Mark::Fill(f) => f.color(),
        }
    }

    pub fn id(&self) -> u32 {
        match self {
            Mark::Stroke(s) => s.id,
            Mark::Fill(f) => f.id,
        }
    }

    pub fn set_id(&mut self, id: u32) {
        match self {
            Mark::Stroke(s) => s.id = id,
            Mark::Fill(f) => f.id = id,
        }
    }

    pub fn author(&self) -> Author {
        match self {
            Mark::Stroke(s) => s.author,
            Mark::Fill(f) => f.author,
        }
    }

    pub fn set_author(&mut self, author: Author) {
        match self {
            Mark::Stroke(s) => s.author = author,
            Mark::Fill(f) => f.author = author,
        }
    }

    pub fn as_stroke(&self) -> Option<&Stroke> {
        match self {
            Mark::Stroke(s) => Some(s),
            Mark::Fill(_) => None,
        }
    }

    pub fn as_fill(&self) -> Option<&Fill> {
        match self {
            Mark::Fill(f) => Some(f),
            Mark::Stroke(_) => None,
        }
    }

    pub fn bounds(&self) -> Option<[f32; 4]> {
        let whole = match self {
            Mark::Stroke(s) => s.bounds().map(|[x0, y0, x1, y1]| {
                let pad = s.size / 2.0;
                [x0 - pad, y0 - pad, x1 + pad, y1 + pad]
            }),
            Mark::Fill(f) => f.bounds(),
        }?;
        cut::clip_bounds(self.cuts(), whole)
    }

    pub fn cuts(&self) -> &[Cut] {
        match self {
            Mark::Stroke(s) => &s.cuts,
            Mark::Fill(f) => &f.cuts,
        }
    }

    pub fn cuts_mut(&mut self) -> &mut Vec<Cut> {
        match self {
            Mark::Stroke(s) => &mut s.cuts,
            Mark::Fill(f) => &mut f.cuts,
        }
    }

    pub fn locked(&self) -> bool {
        match self {
            Mark::Stroke(s) => s.lock,
            Mark::Fill(f) => f.lock,
        }
    }
}

pub fn strokes_of(marks: &[Mark]) -> impl Iterator<Item = &Stroke> {
    marks.iter().filter_map(Mark::as_stroke)
}

pub fn fills_of(marks: &[Mark]) -> impl Iterator<Item = &Fill> {
    marks.iter().filter_map(Mark::as_fill)
}

fn encode_mark(out: &mut Vec<u8>, mark: &Mark) {
    match mark {
        Mark::Stroke(s) => {
            out.push(MARK_STROKE);
            encode_stroke(out, s);
        }
        Mark::Fill(f) => {
            out.push(MARK_FILL);
            fill::encode(out, f);
        }
    }
}

pub fn fingerprint(mark: &Mark) -> u64 {
    let mut out = Vec::with_capacity(512);
    encode_mark(&mut out, mark);
    crate::rng::fnv(crate::rng::FNV_OFFSET, &out)
}

pub fn encode_raw(marks: &[Mark]) -> Vec<u8> {
    let mut out = Vec::with_capacity(marks.len() * 256);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    varint::put(&mut out, marks.len() as u64);
    marks.iter().for_each(|mark| encode_mark(&mut out, mark));
    out
}

pub fn decode_raw(bytes: &[u8]) -> Result<Vec<Mark>> {
    let mut reader = Reader::new(bytes);
    match reader.take(4)? {
        m if m == MAGIC => {}
        m if m == OLD_MAGIC => bail!("a stroke file of the old .draw format; this klad reads .klad drawings only"),
        _ => bail!("not a stroke file"),
    }
    let version = reader.u8()?;
    if !READS.contains(&version) {
        bail!("stroke file version {version} is not one of {READS:?}: made by a newer klad");
    }
    let count = reader.varint()? as usize;
    let marks = (0..count)
        .map(|_| match reader.u8()? {
            MARK_STROKE => decode_stroke(&mut reader).map(Mark::Stroke),
            MARK_FILL => fill::decode(&mut reader).map(Mark::Fill),
            other => bail!("unknown mark kind {other}"),
        })
        .collect::<Result<Vec<_>>>()?;
    if !reader.done() {
        bail!("trailing bytes after {count} marks");
    }
    Ok(marks)
}

pub fn encode_cel(marks: &[Mark]) -> Result<Vec<u8>> {
    Ok(zstd::encode_all(&encode_raw(marks)[..], ZSTD_LEVEL)?)
}

pub fn decode_cel(bytes: &[u8]) -> Result<Vec<Mark>> {
    decode_raw(&zstd::decode_all(bytes)?)
}

pub fn marks(strokes: impl IntoIterator<Item = Stroke>) -> Vec<Mark> {
    strokes.into_iter().map(Mark::Stroke).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fill::{Coverage, FillRef, FillSource, Seed};
    use crate::synth::Synth;

    #[test]
    fn cel_round_trip_is_exact() {
        let strokes = marks(Synth::canvas(2048.0, 2048.0).strokes(200, 3));
        let back = decode_cel(&encode_cel(&strokes).unwrap()).unwrap();
        assert_eq!(back, strokes);
    }

    #[test]
    fn strokes_without_tilt_or_time_round_trip() {
        let stroke = Stroke::new(9, "ink", [0.1, 0.2, 0.3, 1.0], 12.0, 4)
            .with_points(&[(1.0, 2.0, 0.5), (3.5, 2.25, 0.75), (-4.0, 9.0, 1.0)], 0.0);
        let back = decode_raw(&encode_raw(&[stroke.clone().into()])).unwrap();
        assert_eq!(back, vec![Mark::Stroke(stroke)]);
    }

    #[test]
    fn attachments_round_trip_change_the_fingerprint_and_version_1_files_still_read() {
        let line =
            Stroke::new(1, "ink", [0.0, 0.0, 0.0, 1.0], 4.0, 1).with_points(&[(1.0, 1.0, 1.0), (9.0, 2.0, 0.5)], 0.0);
        let free = Stroke::new(2, "eraser", [0.0, 0.0, 0.0, 1.0], 12.0, 2)
            .with_points(&[(4.0, 0.0, 1.0), (4.0, 6.0, 1.0)], 0.0);
        let attached = Stroke {
            on: vec![1, 7 | (5 << 22), 3],
            ..free.clone()
        };
        let marks: Vec<Mark> = vec![line.clone().into(), attached.clone().into()];
        assert_eq!(decode_raw(&encode_raw(&marks)).unwrap(), marks);
        assert_ne!(
            fingerprint(&attached.clone().into()),
            fingerprint(&free.clone().into()),
            "attaching changes what a stroke draws, so caches must see it"
        );
        let mut old = encode_raw(&[line.clone().into(), free.clone().into()]);
        old[4] = 1;
        assert_eq!(
            decode_raw(&old).unwrap(),
            vec![Mark::Stroke(line), Mark::Stroke(free)],
            "a version 1 file reads as strokes with nothing attached"
        );
        old[4] = 2;
        assert!(
            decode_raw(&old).is_ok(),
            "a version 2 file, from before pixel marks, reads as it is"
        );
        old[4] = 4;
        assert!(decode_raw(&old).unwrap_err().to_string().contains("newer klad"));
    }

    #[test]
    fn quantisation_is_stable_under_re_quantising() {
        let s = Sample::quantize(10.37, -4.21, 0.333, (0.5, -0.25), 5.2);
        let (x, y) = s.pos();
        let again = Sample::quantize(x, y, s.pressure(), s.tilt(), s.dt_ms as f32);
        assert_eq!(s, again);
    }

    #[test]
    fn authors_and_tapers_round_trip_and_old_files_are_refused() {
        let points = [(1.0, 1.0, 1.0), (9.0, 2.0, 0.5)];
        let agent = Stroke::new(1, "ink", [0.0, 0.0, 0.0, 1.0], 4.0, 1)
            .with_points(&points, 5.0)
            .by(Author::Agent)
            .tapered([12.0, 30.0]);
        let mut bytes = encode_raw(&[agent.clone().into()]);
        assert_eq!(decode_raw(&bytes).unwrap(), vec![Mark::Stroke(agent)]);
        bytes[..4].copy_from_slice(OLD_MAGIC);
        assert!(decode_raw(&bytes).unwrap_err().to_string().contains("old .draw format"));
        bytes[..4].copy_from_slice(MAGIC);
        bytes[4] = VERSION + 1;
        assert!(decode_raw(&bytes).is_err());
    }

    #[test]
    fn strokes_and_fills_keep_their_order() {
        let strokes = Synth::canvas(256.0, 256.0).strokes(3, 9);
        let mut mask = vec![0u8; 256 * 256];
        (40..90).for_each(|y| (30..200).for_each(|x| mask[y * 256 + x] = 255));
        let fill = Fill::new(
            7,
            [0.9, 0.8, 0.1, 1.0],
            FillSource::Seed {
                seed: Seed {
                    x: 50.0,
                    y: 50.0,
                    tolerance: 0.1,
                    gap: 4.0,
                    expand: 2.0,
                },
                reference: FillRef::This,
            },
            Coverage::from_mask(256, 256, &mask),
        );
        let mixed = vec![
            Mark::Stroke(strokes[0].clone()),
            Mark::Fill(fill.clone()),
            Mark::Stroke(strokes[1].clone()),
        ];
        let back = decode_cel(&encode_cel(&mixed).unwrap()).unwrap();
        assert_eq!(back, mixed);
        assert_eq!(strokes_of(&back).count(), 2);
        assert_eq!(fills_of(&back).next(), Some(&fill));
        assert_eq!(back[1].bounds(), Some([30.0, 40.0, 200.0, 90.0]));
        assert_eq!(
            crate::doc::MarkIds::default().next(&back),
            strokes[0].id.max(strokes[1].id).max(7) + 1
        );
    }

    #[test]
    fn locks_and_cuts_round_trip_and_leave_plain_records_alone() {
        let points = [(1.0, 1.0, 1.0), (90.0, 2.0, 0.5)];
        let plain = Stroke::new(3, "ink", [0.0, 0.0, 0.0, 1.0], 4.0, 1).with_points(&points, 5.0);
        let mut cut = plain.clone();
        cut.lock = true;
        cut.cuts = vec![Cut {
            keep: cut::Keep::Outside,
            polygon: vec![[40.0, -5.0], [60.0, -5.0], [60.0, 10.0]],
        }];
        let back = decode_raw(&encode_raw(&[cut.clone().into(), plain.clone().into()])).unwrap();
        assert_eq!(back, vec![Mark::Stroke(cut.clone()), Mark::Stroke(plain.clone())]);
        let bounds = Mark::Stroke(Stroke {
            cuts: vec![Cut {
                keep: cut::Keep::Inside,
                polygon: vec![[40.0, -5.0], [60.0, -5.0], [60.0, 10.0], [40.0, 10.0]],
            }],
            ..plain
        })
        .bounds()
        .unwrap();
        assert_eq!(bounds, [40.0, -1.0, 60.0, 4.0]);
    }

    #[test]
    fn all_zero_pressure_is_pressureless() {
        let flat =
            Stroke::new(1, "ink", [0.0, 0.0, 0.0, 1.0], 4.0, 1).with_points(&[(1.0, 1.0, 0.0), (5.0, 1.0, 0.0)], 5.0);
        assert!(flat.pressureless());
        let light =
            Stroke::new(1, "ink", [0.0, 0.0, 0.0, 1.0], 4.0, 1).with_points(&[(1.0, 1.0, 0.0), (5.0, 1.0, 0.01)], 5.0);
        assert!(!light.pressureless());
    }

    #[test]
    fn corrupt_data_is_an_error() {
        let mut bytes = encode_raw(&marks(Synth::canvas(512.0, 512.0).strokes(5, 1)));
        bytes.truncate(bytes.len() - 3);
        assert!(decode_raw(&bytes).is_err());
    }
}
