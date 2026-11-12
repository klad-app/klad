use anyhow::{Context, Result, anyhow, bail};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use std::io::{Read, Write};

const MAGIC: u16 = 0xA5E0;
const FRAME_MAGIC: u16 = 0xF1FA;
const HEADER: usize = 128;
const OLD_PALETTE: u16 = 0x0004;
const LAYER: u16 = 0x2004;
const CEL: u16 = 0x2005;
const PROFILE: u16 = 0x2007;
const TAGS: u16 = 0x2018;
const PALETTE: u16 = 0x2019;
const SLICE: u16 = 0x2022;
const TILESET: u16 = 0x2023;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colour {
    pub rgba: [u8; 4],
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub group: bool,
    pub tilemap: bool,
    pub level: u16,
    pub blend: u16,
    pub opacity: u8,
    pub reference: bool,
    pub background: bool,
}

impl Layer {
    pub fn image(name: &str, level: u16) -> Self {
        Self {
            name: name.to_string(),
            visible: true,
            group: false,
            tilemap: false,
            level,
            blend: 0,
            opacity: 255,
            reference: false,
            background: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Picture {
    Pixels { width: u16, height: u16, rgba: Vec<u8> },
    Link(u16),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cel {
    pub layer: u16,
    pub frame: u16,
    pub x: i16,
    pub y: i16,
    pub opacity: u8,
    pub picture: Picture,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub from: u16,
    pub to: u16,
    pub direction: u8,
    pub repeat: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sprite {
    pub width: u16,
    pub height: u16,
    pub palette: Vec<Colour>,
    pub layers: Vec<Layer>,
    pub durations: Vec<u16>,
    pub cels: Vec<Cel>,
    pub tags: Vec<Tag>,
    pub notes: Vec<String>,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| anyhow!("the file ends early, at byte {}", self.bytes.len()))?;
        let part = &self.bytes[self.at..end];
        self.at = end;
        Ok(part)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn word(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn short(&mut self) -> Result<i16> {
        Ok(self.word()? as i16)
    }

    fn dword(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn string(&mut self) -> Result<String> {
        let length = self.word()? as usize;
        Ok(String::from_utf8_lossy(self.take(length)?).to_string())
    }

    fn rest(&mut self) -> &'a [u8] {
        let part = &self.bytes[self.at..];
        self.at = self.bytes.len();
        part
    }
}

struct RawCel {
    layer: u16,
    frame: u16,
    x: i16,
    y: i16,
    opacity: u8,
    width: u16,
    height: u16,
    data: Option<Vec<u8>>,
    link: u16,
}

fn inflate(data: &[u8], wanted: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(wanted);
    ZlibDecoder::new(data)
        .take(wanted as u64 + 1)
        .read_to_end(&mut out)
        .context("a cel's compressed pixels do not unpack")?;
    if out.len() != wanted {
        bail!("a cel unpacks to {} bytes, not the {wanted} its size needs", out.len());
    }
    Ok(out)
}

fn deflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

fn layer_chunk(data: &[u8], flags: u32) -> Result<Layer> {
    let mut r = Reader { bytes: data, at: 0 };
    let bits = r.word()?;
    let kind = r.word()?;
    let level = r.word()?;
    r.take(4)?;
    let blend = r.word()?;
    let opacity = r.byte()?;
    r.take(3)?;
    let name = r.string()?;
    let own_look = kind != 1 || flags & 2 != 0;
    Ok(Layer {
        name,
        visible: bits & 1 != 0,
        group: kind == 1,
        tilemap: kind == 2,
        level,
        blend: match own_look {
            true => blend,
            false => 0,
        },
        opacity: match own_look && (flags & 1 != 0 || kind == 1) {
            true => opacity,
            false => 255,
        },
        reference: bits & 64 != 0,
        background: bits & 8 != 0,
    })
}

fn cel_chunk(data: &[u8], frame: u16, bytes_per_pixel: usize) -> Result<Option<RawCel>> {
    let mut r = Reader { bytes: data, at: 0 };
    let layer = r.word()?;
    let (x, y) = (r.short()?, r.short()?);
    let opacity = r.byte()?;
    let kind = r.word()?;
    r.take(7)?;
    let plain = |width, height, data, link| RawCel {
        layer,
        frame,
        x,
        y,
        opacity,
        width,
        height,
        data,
        link,
    };
    match kind {
        0 => {
            let (width, height) = (r.word()?, r.word()?);
            let pixels = r.take(width as usize * height as usize * bytes_per_pixel)?;
            Ok(Some(plain(width, height, Some(pixels.to_vec()), 0)))
        }
        1 => Ok(Some(plain(0, 0, None, r.word()?))),
        2 => {
            let (width, height) = (r.word()?, r.word()?);
            let pixels = inflate(r.rest(), width as usize * height as usize * bytes_per_pixel)?;
            Ok(Some(plain(width, height, Some(pixels), 0)))
        }
        _ => Ok(None),
    }
}

fn palette_chunk(data: &[u8], palette: &mut Vec<Colour>) -> Result<()> {
    let mut r = Reader { bytes: data, at: 0 };
    let size = r.dword()? as usize;
    let (first, last) = (r.dword()? as usize, r.dword()? as usize);
    r.take(8)?;
    if size > 1 << 16 || last >= 1 << 16 {
        bail!("the palette says it holds {size} colours");
    }
    palette.resize(
        size.max(last + 1),
        Colour {
            rgba: [0, 0, 0, 255],
            name: String::new(),
        },
    );
    (first..=last).try_for_each(|at| {
        let named = r.word()? & 1 != 0;
        let c = r.take(4)?;
        palette[at] = Colour {
            rgba: [c[0], c[1], c[2], c[3]],
            name: match named {
                true => r.string()?,
                false => String::new(),
            },
        };
        Ok::<_, anyhow::Error>(())
    })?;
    palette.truncate(size);
    Ok(())
}

fn old_palette_chunk(data: &[u8], palette: &mut Vec<Colour>) -> Result<()> {
    let mut r = Reader { bytes: data, at: 0 };
    let packets = r.word()?;
    let mut at = 0usize;
    (0..packets).try_for_each(|_| {
        at += r.byte()? as usize;
        let count = match r.byte()? {
            0 => 256,
            n => n as usize,
        };
        (0..count).try_for_each(|_| {
            let c = r.take(3)?;
            if palette.len() <= at {
                palette.resize(
                    at + 1,
                    Colour {
                        rgba: [0, 0, 0, 255],
                        name: String::new(),
                    },
                );
            }
            palette[at].rgba = [c[0], c[1], c[2], 255];
            at += 1;
            Ok::<_, anyhow::Error>(())
        })
    })
}

fn tags_chunk(data: &[u8]) -> Result<Vec<Tag>> {
    let mut r = Reader { bytes: data, at: 0 };
    let count = r.word()?;
    r.take(8)?;
    (0..count)
        .map(|_| {
            let (from, to) = (r.word()?, r.word()?);
            let direction = r.byte()?;
            let repeat = r.word()?;
            r.take(10)?;
            Ok(Tag {
                name: r.string()?,
                from,
                to,
                direction,
                repeat,
            })
        })
        .collect()
}

fn to_rgba(raw: &[u8], depth: u16, palette: &[Colour], clear: Option<u8>) -> Vec<u8> {
    match depth {
        32 => raw.to_vec(),
        16 => raw
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        _ => raw
            .iter()
            .flat_map(|index| match (Some(*index) == clear, palette.get(*index as usize)) {
                (false, Some(colour)) => colour.rgba,
                _ => [0, 0, 0, 0],
            })
            .collect(),
    }
}

pub fn is_aseprite(bytes: &[u8]) -> bool {
    bytes.len() >= HEADER && u16::from_le_bytes([bytes[4], bytes[5]]) == MAGIC
}

pub fn read(bytes: &[u8]) -> Result<Sprite> {
    if !is_aseprite(bytes) {
        bail!("this is not an Aseprite file (.aseprite or .ase)");
    }
    let mut r = Reader { bytes, at: 6 };
    let frames = r.word()?;
    let (width, height) = (r.word()?, r.word()?);
    let depth = r.word()?;
    if ![8, 16, 32].contains(&depth) {
        bail!("the file has {depth} bits per pixel: Aseprite files have 8, 16 or 32");
    }
    let flags = r.dword()?;
    let speed = r.word()?;
    r.take(8)?;
    let transparent = r.byte()?;
    r.at = HEADER;
    let bytes_per_pixel = depth as usize / 8;
    let mut sprite = Sprite {
        width,
        height,
        ..Sprite::default()
    };
    let mut raw: Vec<RawCel> = Vec::new();
    let mut old_palette: Vec<Colour> = Vec::new();
    let mut dropped: Vec<&str> = Vec::new();
    (0..frames).try_for_each(|frame| {
        let start = r.at;
        let size = r.dword()? as usize;
        if r.word()? != FRAME_MAGIC {
            bail!("frame {frame} does not start where the file says it does");
        }
        let old_count = r.word()? as u32;
        let duration = r.word()?;
        r.take(2)?;
        let new_count = r.dword()?;
        let chunks = match new_count {
            0 => old_count,
            n => n,
        };
        sprite.durations.push(match duration {
            0 => speed.max(1),
            d => d,
        });
        (0..chunks).try_for_each(|_| {
            let chunk_start = r.at;
            let chunk_size = r.dword()? as usize;
            let kind = r.word()?;
            if chunk_size < 6 {
                bail!("a chunk in frame {frame} has no size");
            }
            let data = r.take(chunk_size - 6)?;
            match kind {
                LAYER => sprite.layers.push(layer_chunk(data, flags)?),
                CEL => match cel_chunk(data, frame, bytes_per_pixel)? {
                    Some(cel) => raw.push(cel),
                    None => dropped.push("tilemap cels"),
                },
                PALETTE if frame == 0 || sprite.palette.is_empty() => palette_chunk(data, &mut sprite.palette)?,
                PALETTE => dropped.push("palette changes after the first frame"),
                OLD_PALETTE if frame == 0 => old_palette_chunk(data, &mut old_palette)?,
                TAGS => sprite.tags.extend(tags_chunk(data)?),
                SLICE => dropped.push("slices"),
                TILESET => dropped.push("tilesets"),
                _ => {}
            }
            r.at = chunk_start + chunk_size;
            Ok(())
        })?;
        if size >= 16 {
            r.at = start + size;
        }
        Ok(())
    })?;
    if sprite.palette.is_empty() {
        sprite.palette = old_palette;
    }
    sprite.cels = raw
        .into_iter()
        .filter(|cel| (cel.layer as usize) < sprite.layers.len())
        .map(|cel| {
            let background = sprite.layers[cel.layer as usize].background;
            let clear = (depth == 8 && !background).then_some(transparent);
            Cel {
                layer: cel.layer,
                frame: cel.frame,
                x: cel.x,
                y: cel.y,
                opacity: cel.opacity,
                picture: match &cel.data {
                    Some(data) => Picture::Pixels {
                        width: cel.width,
                        height: cel.height,
                        rgba: to_rgba(data, depth, &sprite.palette, clear),
                    },
                    None => Picture::Link(cel.link),
                },
            }
        })
        .collect();
    if sprite.layers.iter().any(|l| l.tilemap) {
        dropped.push("tilemap layers");
    }
    dropped.sort_unstable();
    dropped.dedup();
    sprite.notes = dropped.into_iter().map(String::from).collect();
    Ok(sprite)
}

fn put_string(out: &mut Vec<u8>, text: &str) {
    let bytes = &text.as_bytes()[..text.len().min(u16::MAX as usize)];
    out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(bytes);
}

fn chunk(kind: u16, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 6);
    out.extend_from_slice(&(data.len() as u32 + 6).to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(data);
    out
}

fn profile_out() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&[0; 14]);
    chunk(PROFILE, &data)
}

fn palette_out(palette: &[Colour]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&(palette.len() as u32).to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&(palette.len() as u32 - 1).to_le_bytes());
    data.extend_from_slice(&[0; 8]);
    palette.iter().for_each(|colour| {
        data.extend_from_slice(&(!colour.name.is_empty() as u16).to_le_bytes());
        data.extend_from_slice(&colour.rgba);
        if !colour.name.is_empty() {
            put_string(&mut data, &colour.name);
        }
    });
    chunk(PALETTE, &data)
}

fn layer_out(layer: &Layer) -> Vec<u8> {
    let bits = (layer.visible as u16) | 2 | ((layer.background as u16) << 3) | ((layer.reference as u16) << 6);
    let mut data = Vec::new();
    data.extend_from_slice(&bits.to_le_bytes());
    data.extend_from_slice(&(layer.group as u16).to_le_bytes());
    data.extend_from_slice(&layer.level.to_le_bytes());
    data.extend_from_slice(&[0; 4]);
    data.extend_from_slice(&layer.blend.to_le_bytes());
    data.push(layer.opacity);
    data.extend_from_slice(&[0; 3]);
    put_string(&mut data, &layer.name);
    chunk(LAYER, &data)
}

fn tags_out(tags: &[Tag]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&(tags.len() as u16).to_le_bytes());
    data.extend_from_slice(&[0; 8]);
    tags.iter().for_each(|tag| {
        data.extend_from_slice(&tag.from.to_le_bytes());
        data.extend_from_slice(&tag.to.to_le_bytes());
        data.push(tag.direction);
        data.extend_from_slice(&tag.repeat.to_le_bytes());
        data.extend_from_slice(&[0; 10]);
        put_string(&mut data, &tag.name);
    });
    chunk(TAGS, &data)
}

fn cel_out(cel: &Cel) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    data.extend_from_slice(&cel.layer.to_le_bytes());
    data.extend_from_slice(&cel.x.to_le_bytes());
    data.extend_from_slice(&cel.y.to_le_bytes());
    data.push(cel.opacity);
    match &cel.picture {
        Picture::Link(frame) => {
            data.extend_from_slice(&1u16.to_le_bytes());
            data.extend_from_slice(&[0; 7]);
            data.extend_from_slice(&frame.to_le_bytes());
        }
        Picture::Pixels { width, height, rgba } => {
            if rgba.len() != *width as usize * *height as usize * 4 {
                bail!("a cel of {width}x{height} holds {} bytes", rgba.len());
            }
            data.extend_from_slice(&2u16.to_le_bytes());
            data.extend_from_slice(&[0; 7]);
            data.extend_from_slice(&width.to_le_bytes());
            data.extend_from_slice(&height.to_le_bytes());
            data.extend_from_slice(&deflate(rgba)?);
        }
    }
    Ok(chunk(CEL, &data))
}

pub fn write(sprite: &Sprite) -> Result<Vec<u8>> {
    if sprite.durations.is_empty() || sprite.durations.len() > u16::MAX as usize {
        bail!(
            "an Aseprite file holds 1 to 65535 frames, not {}",
            sprite.durations.len()
        );
    }
    if sprite.width == 0 || sprite.height == 0 {
        bail!("an Aseprite file needs a size");
    }
    if sprite.layers.len() > u16::MAX as usize || sprite.palette.len() > 1 << 16 {
        bail!("too many layers or colours for an Aseprite file");
    }
    let frames = sprite
        .durations
        .iter()
        .enumerate()
        .map(|(frame, duration)| {
            let first: Vec<Vec<u8>> = match frame {
                0 => std::iter::once(profile_out())
                    .chain((!sprite.palette.is_empty()).then(|| palette_out(&sprite.palette)))
                    .chain(sprite.layers.iter().map(layer_out))
                    .chain((!sprite.tags.is_empty()).then(|| tags_out(&sprite.tags)))
                    .collect(),
                _ => Vec::new(),
            };
            let cels = sprite
                .cels
                .iter()
                .filter(|cel| cel.frame as usize == frame)
                .map(cel_out)
                .collect::<Result<Vec<_>>>()?;
            let chunks: Vec<Vec<u8>> = first.into_iter().chain(cels).collect();
            let size = 16 + chunks.iter().map(Vec::len).sum::<usize>();
            let mut out = Vec::with_capacity(size);
            out.extend_from_slice(&(size as u32).to_le_bytes());
            out.extend_from_slice(&FRAME_MAGIC.to_le_bytes());
            out.extend_from_slice(&(chunks.len().min(0xFFFF) as u16).to_le_bytes());
            out.extend_from_slice(&duration.to_le_bytes());
            out.extend_from_slice(&[0; 2]);
            out.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
            chunks.iter().for_each(|c| out.extend_from_slice(c));
            Ok(out)
        })
        .collect::<Result<Vec<_>>>()?;
    let size = HEADER + frames.iter().map(Vec::len).sum::<usize>();
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&(sprite.durations.len() as u16).to_le_bytes());
    out.extend_from_slice(&sprite.width.to_le_bytes());
    out.extend_from_slice(&sprite.height.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&sprite.durations[0].to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(sprite.palette.len().min(u16::MAX as usize) as u16).to_le_bytes());
    out.extend_from_slice(&[1, 1]);
    out.extend_from_slice(&0i16.to_le_bytes());
    out.extend_from_slice(&0i16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.resize(HEADER, 0);
    frames.iter().for_each(|f| out.extend_from_slice(f));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sprite {
        let red = [0xef, 0x23, 0x3c, 255];
        let ink = [0x22, 0x22, 0x3b, 255];
        let square = |colour: [u8; 4]| -> Vec<u8> {
            (0..6)
                .flat_map(|i| if i == 5 { [0, 0, 0, 0] } else { colour })
                .collect()
        };
        Sprite {
            width: 16,
            height: 12,
            palette: vec![
                Colour {
                    rgba: ink,
                    name: "ink".into(),
                },
                Colour {
                    rgba: red,
                    name: String::new(),
                },
            ],
            layers: vec![
                Layer::image("ground", 0),
                Layer {
                    group: true,
                    opacity: 200,
                    ..Layer::image("hero", 0)
                },
                Layer {
                    blend: 1,
                    opacity: 128,
                    visible: false,
                    ..Layer::image("shade ✦", 1)
                },
            ],
            durations: vec![100, 150, 100],
            cels: vec![
                Cel {
                    layer: 0,
                    frame: 0,
                    x: 2,
                    y: -1,
                    opacity: 255,
                    picture: Picture::Pixels {
                        width: 3,
                        height: 2,
                        rgba: square(ink),
                    },
                },
                Cel {
                    layer: 2,
                    frame: 0,
                    x: 4,
                    y: 5,
                    opacity: 90,
                    picture: Picture::Pixels {
                        width: 2,
                        height: 3,
                        rgba: square(red),
                    },
                },
                Cel {
                    layer: 0,
                    frame: 1,
                    x: 2,
                    y: -1,
                    opacity: 255,
                    picture: Picture::Link(0),
                },
                Cel {
                    layer: 2,
                    frame: 2,
                    x: 0,
                    y: 0,
                    opacity: 255,
                    picture: Picture::Pixels {
                        width: 1,
                        height: 1,
                        rgba: red.to_vec(),
                    },
                },
            ],
            tags: vec![
                Tag {
                    name: "idle".into(),
                    from: 0,
                    to: 1,
                    direction: 0,
                    repeat: 0,
                },
                Tag {
                    name: "blink".into(),
                    from: 2,
                    to: 2,
                    direction: 2,
                    repeat: 3,
                },
            ],
            notes: Vec::new(),
        }
    }

    #[test]
    fn a_sprite_written_and_read_back_is_the_same_sprite() {
        let sprite = sample();
        let bytes = write(&sprite).unwrap();
        assert!(is_aseprite(&bytes));
        assert_eq!(
            u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize,
            bytes.len()
        );
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 3);
        assert_eq!(u16::from_le_bytes([bytes[12], bytes[13]]), 32);
        assert_eq!(read(&bytes).unwrap(), sprite);
        assert!(
            read(&bytes[..bytes.len() - 9])
                .unwrap_err()
                .to_string()
                .contains("ends early")
        );
        assert!(
            read(b"GIF89a")
                .unwrap_err()
                .to_string()
                .contains("not an Aseprite file")
        );
        assert!(
            write(&Sprite {
                durations: Vec::new(),
                ..sample()
            })
            .is_err()
        );
    }

    fn by_hand(depth: u16, flags: u32, transparent: u8, chunks: &[Vec<u8>]) -> Vec<u8> {
        let size = 16 + chunks.iter().map(Vec::len).sum::<usize>();
        let mut out = Vec::new();
        out.extend_from_slice(&((HEADER + size) as u32).to_le_bytes());
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&depth.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&125u16.to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        out.push(transparent);
        out.resize(HEADER, 0);
        out.extend_from_slice(&(size as u32).to_le_bytes());
        out.extend_from_slice(&FRAME_MAGIC.to_le_bytes());
        out.extend_from_slice(&(chunks.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        chunks.iter().for_each(|c| out.extend_from_slice(c));
        out
    }

    fn raw_cel(layer: u16, pixels: &[u8], width: u16, height: u16) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&layer.to_le_bytes());
        data.extend_from_slice(&[0; 4]);
        data.push(255);
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&[0; 7]);
        data.extend_from_slice(&width.to_le_bytes());
        data.extend_from_slice(&height.to_le_bytes());
        data.extend_from_slice(pixels);
        chunk(CEL, &data)
    }

    #[test]
    fn indexed_and_grey_files_with_old_chunk_counts_and_raw_cels_are_read() {
        let palette = [
            Colour {
                rgba: [255, 0, 255, 255],
                name: String::new(),
            },
            Colour {
                rgba: [10, 20, 30, 255],
                name: "night".into(),
            },
            Colour {
                rgba: [200, 210, 220, 128],
                name: String::new(),
            },
        ];
        let back = Layer {
            background: true,
            ..Layer::image("back", 0)
        };
        let indexed = by_hand(
            8,
            1,
            0,
            &[
                palette_out(&palette),
                layer_out(&back),
                layer_out(&Layer::image("top", 0)),
                raw_cel(0, &[0, 1, 2, 9], 4, 1),
                raw_cel(1, &[0, 1, 2, 9], 4, 1),
            ],
        );
        let sprite = read(&indexed).unwrap();
        assert_eq!(
            sprite.durations,
            [125],
            "a frame without a duration takes the file's speed"
        );
        assert_eq!(sprite.palette, palette);
        let pixels = |at: usize| match &sprite.cels[at].picture {
            Picture::Pixels { rgba, .. } => rgba.clone(),
            Picture::Link(_) => panic!("a link"),
        };
        assert_eq!(
            pixels(0),
            [255, 0, 255, 255, 10, 20, 30, 255, 200, 210, 220, 128, 0, 0, 0, 0],
            "on the background layer index 0 is a colour; an index past the palette is clear"
        );
        assert_eq!(pixels(1)[..4], [0, 0, 0, 0], "elsewhere the transparent index is clear");
        let grey = by_hand(
            16,
            0,
            0,
            &[
                layer_out(&Layer {
                    opacity: 40,
                    ..Layer::image("g", 0)
                }),
                raw_cel(0, &[7, 255, 90, 128], 2, 1),
            ],
        );
        let sprite = read(&grey).unwrap();
        assert_eq!(
            sprite.layers[0].opacity, 255,
            "without the flag a layer's opacity is not valid"
        );
        assert_eq!(
            sprite.cels[0].picture,
            Picture::Pixels {
                width: 2,
                height: 1,
                rgba: vec![7, 7, 7, 255, 90, 90, 90, 128]
            }
        );
        let mut old = Vec::new();
        old.extend_from_slice(&1u16.to_le_bytes());
        old.extend_from_slice(&[0, 2, 1, 2, 3, 4, 5, 6]);
        let sprite = read(&by_hand(32, 1, 0, &[chunk(OLD_PALETTE, &old), chunk(SLICE, &[0; 4])])).unwrap();
        let colours: Vec<[u8; 4]> = sprite.palette.iter().map(|c| c.rgba).collect();
        assert_eq!(colours, [[1, 2, 3, 255], [4, 5, 6, 255]]);
        assert_eq!(sprite.notes, ["slices"]);
    }
}
