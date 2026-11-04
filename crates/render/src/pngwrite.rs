use crc32fast::Hasher;
use flate2::{Compress, Compression, FlushCompress, Status};
use rayon::prelude::*;

const STRIP: usize = 256 << 10;
const IDAT: usize = 1 << 20;
const ADLER: u32 = 65521;

pub fn write(width: u32, height: u32, depth: u8, rows: &[u8]) -> Vec<u8> {
    let bpp = 4 * depth as usize / 8;
    let line = width as usize * bpp;
    let filtered: Vec<Vec<u8>> = (0..height as usize)
        .into_par_iter()
        .map(|y| {
            let row = &rows[y * line..(y + 1) * line];
            let above = (y > 0).then(|| &rows[(y - 1) * line..y * line]);
            filter(row, above, bpp)
        })
        .collect();
    let per_strip = (STRIP / (line + 1)).max(1);
    let strips: Vec<&[Vec<u8>]> = filtered.chunks(per_strip).collect();
    let last = strips.len().saturating_sub(1);
    let packed: Vec<Vec<u8>> = strips
        .par_iter()
        .enumerate()
        .map(|(i, strip)| deflate(&strip.concat(), i == last))
        .collect();
    let sum = filtered.iter().fold((1u32, 0u32), |acc, row| adler(acc, row));
    let mut zlib = Vec::with_capacity(packed.iter().map(Vec::len).sum::<usize>() + 6);
    zlib.extend_from_slice(&[0x78, 0x9c]);
    packed.iter().for_each(|p| zlib.extend_from_slice(p));
    zlib.extend_from_slice(&((sum.1 << 16) | sum.0).to_be_bytes());
    let mut out = Vec::with_capacity(zlib.len() + 64);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[depth, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"sRGB", &[0]);
    zlib.chunks(IDAT).for_each(|part| chunk(&mut out, b"IDAT", part));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = Hasher::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.finalize().to_be_bytes());
}

fn deflate(data: &[u8], last: bool) -> Vec<u8> {
    let mut packer = Compress::new(Compression::new(6), false);
    let mut out = Vec::with_capacity(data.len() / 2 + 4096);
    let flush = match last {
        true => FlushCompress::Finish,
        false => FlushCompress::Sync,
    };
    loop {
        out.reserve(64 << 10);
        let read = packer.total_in() as usize;
        let status = packer
            .compress_vec(&data[read..], &mut out, flush)
            .expect("deflate never fails on memory input");
        let room = out.len() < out.capacity();
        match last {
            true if status == Status::StreamEnd => break,
            false if packer.total_in() as usize == data.len() && room => break,
            _ => {}
        }
    }
    out
}

fn adler((a, b): (u32, u32), data: &[u8]) -> (u32, u32) {
    data.chunks(5552).fold((a, b), |(mut a, mut b), part| {
        part.iter().for_each(|&v| {
            a += v as u32;
            b += a;
        });
        (a % ADLER, b % ADLER)
    })
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
    match (pa <= pb && pa <= pc, pb <= pc) {
        (true, _) => a,
        (false, true) => b,
        _ => c,
    }
}

fn filter(row: &[u8], above: Option<&[u8]>, bpp: usize) -> Vec<u8> {
    let zero = vec![0u8; row.len()];
    let up = above.unwrap_or(&zero);
    let predict = |kind: u8, i: usize| {
        let a = if i >= bpp { row[i - bpp] } else { 0 };
        let c = if i >= bpp { up[i - bpp] } else { 0 };
        match kind {
            0 => 0,
            1 => a,
            2 => up[i],
            3 => ((a as u16 + up[i] as u16) / 2) as u8,
            _ => paeth(a, up[i], c),
        }
    };
    let line = |kind: u8| -> Vec<u8> { (0..row.len()).map(|i| row[i].wrapping_sub(predict(kind, i))).collect() };
    let cost = |bytes: &[u8]| bytes.iter().map(|&v| (v as i8).unsigned_abs() as u32).sum::<u32>();
    let (kind, bytes) = (0..5u8)
        .map(|k| (k, line(k)))
        .min_by_key(|(k, bytes)| (cost(bytes), *k))
        .expect("five filters");
    std::iter::once(kind).chain(bytes).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(bytes: &[u8]) -> (png::OutputInfo, Vec<u8>) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        (info, buf)
    }

    #[test]
    fn what_it_writes_decodes_to_the_same_pixels_at_8_and_16_bits() {
        let (w, h) = (300u32, 700u32);
        let eight: Vec<u8> = (0..w * h * 4)
            .map(|i| ((i * 7 + (i / 1200) * 13) % 251) as u8 ^ ((i % 4 == 3) as u8 * 0x80))
            .collect();
        let (info, back) = decoded(&write(w, h, 8, &eight));
        assert_eq!((info.width, info.height, info.bit_depth as u8), (w, h, 8));
        assert_eq!(back, eight);
        let sixteen: Vec<u8> = (0..w * h * 8).map(|i| ((i * 31 + i / 2400) % 256) as u8).collect();
        let (info, back) = decoded(&write(w, h, 16, &sixteen));
        assert_eq!(info.bit_depth as u8, 16);
        assert_eq!(back, sixteen);
        assert_eq!(write(w, h, 8, &eight), write(w, h, 8, &eight));
    }
}
