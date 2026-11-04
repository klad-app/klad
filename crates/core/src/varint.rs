use anyhow::{Result, bail};

pub fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

pub fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

pub fn put(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

pub fn put_signed(out: &mut Vec<u8>, v: i64) {
    put(out, zigzag(v));
}

pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let Some(slice) = self.bytes.get(self.pos..self.pos + n) else {
            bail!("stroke data ends early at byte {}", self.pos);
        };
        self.pos += n;
        Ok(slice)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }

    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into()?))
    }

    pub fn varint(&mut self) -> Result<u64> {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let byte = self.u8()?;
            value |= ((byte & 0x7F) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift > 63 {
                bail!("varint too long at byte {}", self.pos);
            }
        }
    }

    pub fn signed(&mut self) -> Result<i64> {
        Ok(unzigzag(self.varint()?))
    }

    pub fn done(&self) -> bool {
        self.pos == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_round_trip() {
        let values = [0i64, 1, -1, 63, -64, 64, 1 << 40, -(1 << 40), i64::MAX, i64::MIN];
        let mut out = Vec::new();
        values.iter().for_each(|&v| put_signed(&mut out, v));
        let mut reader = Reader::new(&out);
        let back: Vec<i64> = values.iter().map(|_| reader.signed().unwrap()).collect();
        assert_eq!(back, values);
        assert!(reader.done());
    }
}
