use anyhow::{Result, bail};
use std::collections::BTreeMap;

pub struct Shot {
    pub rgba: Vec<u8>,
    pub ms: u32,
}

const CLEAR_AT: usize = 128;

fn colours(shots: &[Shot]) -> BTreeMap<[u8; 3], usize> {
    let mut seen: BTreeMap<[u8; 3], usize> = BTreeMap::new();
    shots.iter().for_each(|shot| {
        shot.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[3] >= CLEAR_AT as u8)
            .for_each(|px| *seen.entry([px[0], px[1], px[2]]).or_default() += 1);
    });
    seen
}

fn median_cut(counts: &BTreeMap<[u8; 3], usize>, room: usize) -> Vec<[u8; 3]> {
    let mut boxes: Vec<Vec<([u8; 3], usize)>> = vec![counts.iter().map(|(c, n)| (*c, *n)).collect()];
    while boxes.len() < room {
        let widest = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| {
                let spans: [u8; 3] = std::array::from_fn(|ch| {
                    let low = b.iter().map(|(c, _)| c[ch]).min().unwrap_or(0);
                    let high = b.iter().map(|(c, _)| c[ch]).max().unwrap_or(0);
                    high - low
                });
                let channel = (0..3).max_by_key(|ch| spans[*ch]).unwrap_or(0);
                (spans[channel], i, channel)
            })
            .max();
        let Some((_, at, channel)) = widest else {
            break;
        };
        let mut split = boxes.swap_remove(at);
        split.sort_by_key(|(c, _)| c[channel]);
        let total: usize = split.iter().map(|(_, n)| n).sum();
        let mut run = 0;
        let cut = split
            .iter()
            .position(|(_, n)| {
                run += n;
                run * 2 >= total
            })
            .map_or(1, |i| (i + 1).clamp(1, split.len() - 1));
        let upper = split.split_off(cut);
        boxes.push(split);
        boxes.push(upper);
    }
    boxes
        .iter()
        .map(|b| {
            let total = b.iter().map(|(_, n)| *n).sum::<usize>().max(1);
            std::array::from_fn(|ch| (b.iter().map(|(c, n)| c[ch] as usize * n).sum::<usize>() / total) as u8)
        })
        .collect()
}

fn nearest(table: &[[u8; 3]], colour: [u8; 3]) -> usize {
    table
        .iter()
        .enumerate()
        .min_by_key(|(_, c)| (0..3).map(|ch| (c[ch] as i32 - colour[ch] as i32).pow(2)).sum::<i32>())
        .map_or(0, |(i, _)| i)
}

struct Bits {
    out: Vec<u8>,
    hold: u32,
    held: u32,
}

impl Bits {
    fn put(&mut self, code: u32, width: u32) {
        self.hold |= code << self.held;
        self.held += width;
        while self.held >= 8 {
            self.out.push((self.hold & 0xff) as u8);
            self.hold >>= 8;
            self.held -= 8;
        }
    }

    fn end(mut self) -> Vec<u8> {
        if self.held > 0 {
            self.out.push((self.hold & 0xff) as u8);
        }
        self.out
    }
}

fn lzw(indices: &[u8], min: u32) -> Vec<u8> {
    let (clear, end) = (1u32 << min, (1u32 << min) + 1);
    let mut bits = Bits {
        out: Vec::with_capacity(indices.len() / 2),
        hold: 0,
        held: 0,
    };
    let mut table: BTreeMap<(u32, u8), u32> = BTreeMap::new();
    let (mut next, mut width) = (end + 1, min + 1);
    bits.put(clear, width);
    let mut current: Option<u32> = None;
    indices.iter().for_each(|&index| {
        let Some(prefix) = current else {
            current = Some(index as u32);
            return;
        };
        match table.get(&(prefix, index)) {
            Some(&code) => current = Some(code),
            None => {
                bits.put(prefix, width);
                match next < 4096 {
                    true => {
                        table.insert((prefix, index), next);
                        if next == 1 << width && width < 12 {
                            width += 1;
                        }
                        next += 1;
                    }
                    false => {
                        bits.put(clear, width);
                        table.clear();
                        next = end + 1;
                        width = min + 1;
                    }
                }
                current = Some(index as u32);
            }
        }
    });
    if let Some(prefix) = current {
        bits.put(prefix, width);
    }
    bits.put(end, width);
    bits.end()
}

fn blocks(out: &mut Vec<u8>, data: &[u8]) {
    data.chunks(255).for_each(|chunk| {
        out.push(chunk.len() as u8);
        out.extend_from_slice(chunk);
    });
    out.push(0);
}

pub fn delays(durations: &[u32]) -> Vec<u16> {
    let mut owed = 0i64;
    durations
        .iter()
        .map(|&ms| {
            owed += ms as i64;
            let ticks = ((owed + 5) / 10).max(1);
            owed -= ticks * 10;
            ticks.min(u16::MAX as i64) as u16
        })
        .collect()
}

pub fn encode(width: u32, height: u32, shots: &[Shot], looping: bool) -> Result<Vec<u8>> {
    if shots.is_empty() {
        bail!("a GIF needs at least one frame");
    }
    if width == 0 || height == 0 || width > u16::MAX as u32 || height > u16::MAX as u32 {
        bail!("a GIF is at most 65535 px a side, not {width}x{height}");
    }
    let pixels = width as usize * height as usize;
    if let Some(short) = shots.iter().find(|s| s.rgba.len() != pixels * 4) {
        bail!("a frame holds {} bytes, not {width}x{height} pixels", short.rgba.len());
    }
    let counts = colours(shots);
    let table: Vec<[u8; 3]> = match counts.len() <= 255 {
        true => counts.keys().copied().collect(),
        false => median_cut(&counts, 255),
    };
    let exact: BTreeMap<[u8; 3], usize> = table.iter().enumerate().map(|(i, c)| (*c, i)).collect();
    let clear = table.len();
    let bits = (clear + 1).next_power_of_two().trailing_zeros().max(1);
    let slots = 1usize << bits;
    let mut out = Vec::with_capacity(pixels / 2);
    out.extend_from_slice(b"GIF89a");
    out.extend_from_slice(&(width as u16).to_le_bytes());
    out.extend_from_slice(&(height as u16).to_le_bytes());
    out.extend_from_slice(&[0x80 | 0x70 | (bits as u8 - 1), 0, 0]);
    (0..slots).for_each(|i| out.extend_from_slice(&table.get(i).copied().unwrap_or([0, 0, 0])));
    if looping && shots.len() > 1 {
        out.extend_from_slice(&[0x21, 0xff, 11]);
        out.extend_from_slice(b"NETSCAPE2.0");
        out.extend_from_slice(&[3, 1, 0, 0, 0]);
    }
    let mut cache: BTreeMap<[u8; 3], usize> = BTreeMap::new();
    let ticks = delays(&shots.iter().map(|s| s.ms).collect::<Vec<_>>());
    shots.iter().zip(&ticks).for_each(|(shot, delay)| {
        let indices: Vec<u8> = shot
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|px| match px[3] >= CLEAR_AT as u8 {
                false => clear as u8,
                true => {
                    let colour = [px[0], px[1], px[2]];
                    let at = exact
                        .get(&colour)
                        .copied()
                        .unwrap_or_else(|| *cache.entry(colour).or_insert_with(|| nearest(&table, colour)));
                    at as u8
                }
            })
            .collect();
        out.extend_from_slice(&[0x21, 0xf9, 4, (2 << 2) | 1]);
        out.extend_from_slice(&delay.to_le_bytes());
        out.extend_from_slice(&[clear as u8, 0]);
        out.push(0x2c);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(width as u16).to_le_bytes());
        out.extend_from_slice(&(height as u16).to_le_bytes());
        out.push(0);
        let min = bits.max(2);
        out.push(min as u8);
        blocks(&mut out, &lzw(&indices, min));
    });
    out.push(0x3b);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unpack(data: &[u8], min: u32) -> Vec<u8> {
        let (clear, end) = (1u32 << min, (1u32 << min) + 1);
        let mut table: Vec<Vec<u8>> = Vec::new();
        let reset = |table: &mut Vec<Vec<u8>>| {
            table.clear();
            (0..clear).for_each(|i| table.push(vec![i as u8]));
            table.push(Vec::new());
            table.push(Vec::new());
        };
        reset(&mut table);
        let (mut width, mut hold, mut held, mut at) = (min + 1, 0u32, 0u32, 0usize);
        let mut previous: Option<Vec<u8>> = None;
        let mut out = Vec::new();
        loop {
            while held < width {
                hold |= (data[at] as u32) << held;
                held += 8;
                at += 1;
            }
            let code = hold & ((1 << width) - 1);
            hold >>= width;
            held -= width;
            if code == clear {
                reset(&mut table);
                width = min + 1;
                previous = None;
                continue;
            }
            if code == end {
                return out;
            }
            let entry = match (table.get(code as usize), &previous) {
                (Some(known), _) if (code as usize) < table.len() => known.clone(),
                (_, Some(before)) => {
                    let mut made = before.clone();
                    made.push(before[0]);
                    made
                }
                _ => panic!("a code before any pixel"),
            };
            out.extend_from_slice(&entry);
            if let Some(before) = previous {
                let mut made = before;
                made.push(entry[0]);
                table.push(made);
                if table.len() == 1 << width && width < 12 {
                    width += 1;
                }
            }
            previous = Some(entry);
        }
    }

    fn frame(width: usize, height: usize, at: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
        (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .flat_map(|(x, y)| at(x, y))
            .collect()
    }

    #[test]
    fn the_packed_pixels_unpack_to_the_same_indices() {
        let noisy: Vec<u8> = (0..20_000u32)
            .map(|i| ((i.wrapping_mul(2_654_435_761) >> 13) % 7) as u8)
            .collect();
        assert_eq!(
            unpack(&lzw(&noisy, 3), 3),
            noisy,
            "enough codes to fill the table and start over"
        );
        let flat = vec![1u8; 5000];
        assert_eq!(unpack(&lzw(&flat, 2), 2), flat);
        assert_eq!(unpack(&lzw(&[0, 1, 2, 3], 2), 2), [0, 1, 2, 3]);
    }

    #[test]
    fn a_gif_keeps_exact_colours_a_clear_pixel_its_timing_and_its_loop() {
        let red = [0xef, 0x23, 0x3c, 255];
        let ink = [0x22, 0x22, 0x3b, 255];
        let shots = [
            Shot {
                rgba: frame(4, 2, |x, _| if x == 0 { red } else { [0, 0, 0, 0] }),
                ms: 83,
            },
            Shot {
                rgba: frame(4, 2, |x, _| if x == 1 { ink } else { [9, 9, 9, 40] }),
                ms: 83,
            },
            Shot {
                rgba: frame(4, 2, |x, _| if x == 2 { red } else { [0, 0, 0, 0] }),
                ms: 84,
            },
        ];
        let gif = encode(4, 2, &shots, true).unwrap();
        assert_eq!(&gif[..6], b"GIF89a");
        assert_eq!((gif[6], gif[8]), (4, 2));
        assert_eq!(gif[10] & 0x87, 0x81, "a global table of four slots");
        assert_eq!(
            &gif[13..19],
            [0x22, 0x22, 0x3b, 0xef, 0x23, 0x3c],
            "the two colours, exactly"
        );
        let text = String::from_utf8_lossy(&gif);
        assert!(text.contains("NETSCAPE2.0"), "it loops");
        assert_eq!(*gif.last().unwrap(), 0x3b);
        let controls: Vec<(u16, u8)> = gif
            .windows(8)
            .filter(|w| w[0] == 0x21 && w[1] == 0xf9 && w[2] == 4 && w[7] == 0)
            .map(|w| (u16::from_le_bytes([w[4], w[5]]), w[6]))
            .collect();
        assert_eq!(
            controls,
            [(8, 2), (9, 2), (8, 2)],
            "250 ms over three frames, in hundredths, with index 2 as the clear pixel"
        );
        assert!(!String::from_utf8_lossy(&encode(4, 2, &shots, false).unwrap()).contains("NETSCAPE"));
        assert_eq!(
            delays(&[83, 83, 84, 83, 83, 84]).iter().map(|d| *d as u32).sum::<u32>(),
            50
        );
        assert_eq!(delays(&[1]), [1], "no frame is shorter than a hundredth");
        assert!(encode(4, 2, &[], true).is_err());
    }

    #[test]
    fn another_decoder_reads_back_the_same_picture() {
        use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo, image::CachingHint};
        let (w, h) = (37usize, 23usize);
        let rgba = frame(w, h, |x, y| match (x * 7 + y * 13) % 5 {
            0 => [0, 0, 0, 0],
            1 => [0xef, 0x23, 0x3c, 255],
            2 => [0x22, 0x22, 0x3b, 255],
            3 => [(x * 6) as u8, (y * 11) as u8, 200, 255],
            _ => [250, 250, (x + y) as u8, 255],
        });
        let gif = encode(
            w as u32,
            h as u32,
            &[Shot {
                rgba: rgba.clone(),
                ms: 100,
            }],
            false,
        )
        .unwrap();
        let image = Image::from_encoded(Data::new_copy(&gif)).expect("Skia reads the GIF");
        assert_eq!((image.width() as usize, image.height() as usize), (w, h));
        let info = ImageInfo::new((w as i32, h as i32), ColorType::RGBA8888, AlphaType::Unpremul, None);
        let mut read = vec![0u8; w * h * 4];
        assert!(image.read_pixels(&info, &mut read, w * 4, (0, 0), CachingHint::Allow));
        rgba.as_chunks::<4>()
            .0
            .iter()
            .zip(read.as_chunks::<4>().0)
            .enumerate()
            .for_each(|(i, (made, back))| match made[3] {
                0 => assert_eq!(back[3], 0, "pixel {i} stays clear"),
                _ => assert_eq!(made, back, "pixel {i}"),
            });
    }

    #[test]
    fn more_than_255_colours_are_cut_down_to_a_palette_that_stays_close() {
        let rgba = frame(64, 64, |x, y| [(x * 4) as u8, (y * 4) as u8, ((x + y) * 2) as u8, 255]);
        let counts = colours(&[Shot {
            rgba: rgba.clone(),
            ms: 100,
        }]);
        assert!(counts.len() > 255);
        let table = median_cut(&counts, 255);
        assert_eq!(table.len(), 255);
        let worst = counts
            .keys()
            .map(|c| {
                let near = table[nearest(&table, *c)];
                (0..3)
                    .map(|ch| (near[ch] as i32 - c[ch] as i32).abs())
                    .max()
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0);
        assert!(worst <= 40, "the furthest colour is {worst} levels from its entry");
        assert!(encode(64, 64, &[Shot { rgba, ms: 100 }], false).is_ok());
    }
}
