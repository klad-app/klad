use anyhow::{Result, bail};
use half::f16;
use klad_core::rng::{FNV_OFFSET, fnv};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

pub const TILE: usize = 256;
pub const TILE_PX: usize = TILE * TILE;
const EMPTY: [u16; 4] = [TILE as u16, TILE as u16, 0, 0];

#[derive(Clone)]
pub struct TileGrid<T, const C: usize> {
    pub width: u32,
    pub height: u32,
    pub cols: usize,
    pub rows: usize,
    tiles: Vec<Option<Arc<Box<[T]>>>>,
}

pub type Heights = TileGrid<f16, 1>;

#[derive(Clone)]
pub struct Raster {
    colour: TileGrid<f16, 4>,
    body: Option<Heights>,
}

impl Deref for Raster {
    type Target = TileGrid<f16, 4>;

    fn deref(&self) -> &Self::Target {
        &self.colour
    }
}

impl DerefMut for Raster {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.colour
    }
}

pub type Tile = (Option<Arc<[f16]>>, Option<Arc<[f16]>>);

impl<T: Copy + Default, const C: usize> TileGrid<T, C> {
    pub fn new(width: u32, height: u32) -> Self {
        let cols = (width as usize).div_ceil(TILE);
        let rows = (height as usize).div_ceil(TILE);
        Self {
            width,
            height,
            cols,
            rows,
            tiles: (0..cols * rows).map(|_| None).collect(),
        }
    }

    pub fn index(&self, tx: usize, ty: usize) -> usize {
        ty * self.cols + tx
    }

    pub fn at(&self, index: usize) -> Option<&[T]> {
        self.tiles[index].as_deref().map(|t| &t[..])
    }

    pub fn at_mut(&mut self, index: usize) -> Option<&mut [T]> {
        self.tiles[index].as_mut().map(|t| &mut Arc::make_mut(t)[..])
    }

    pub fn get(&self, tx: usize, ty: usize) -> Option<&[T]> {
        self.at(self.index(tx, ty))
    }

    pub fn ensure(&mut self, index: usize) -> &mut [T] {
        let tile =
            self.tiles[index].get_or_insert_with(|| Arc::new(vec![T::default(); TILE_PX * C].into_boxed_slice()));
        &mut Arc::make_mut(tile)[..]
    }

    pub fn clear(&mut self, index: usize) {
        self.tiles[index] = None;
    }

    pub fn put(&mut self, index: usize, data: Box<[T]>) {
        assert_eq!(data.len(), TILE_PX * C);
        self.tiles[index] = Some(Arc::new(data));
    }

    pub fn occupied(&self) -> impl Iterator<Item = (usize, &[T])> {
        self.tiles
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.as_deref().map(|t| (i, &t[..])))
    }

    pub fn count(&self) -> usize {
        self.tiles.iter().filter(|t| t.is_some()).count()
    }

    pub fn bytes(&self) -> usize {
        self.count() * TILE_PX * C * std::mem::size_of::<T>()
    }
}

const RASTER_MAGIC: &[u8; 4] = b"DART";
const HEIGHT_MAGIC: &[u8; 4] = b"HGHT";

fn fold_tiles<T: bytemuck::Pod + Default, const C: usize>(grid: &TileGrid<T, C>, from: u64) -> u64 {
    grid.occupied().fold(from, |h, (i, tile)| {
        fnv(fnv(h, &(i as u64).to_le_bytes()), bytemuck::cast_slice(tile))
    })
}

fn write_tiles<T: bytemuck::Pod + Default, const C: usize>(grid: &TileGrid<T, C>, out: &mut Vec<u8>) {
    out.extend_from_slice(&(grid.count() as u32).to_le_bytes());
    grid.occupied().for_each(|(i, tile)| {
        out.extend_from_slice(&(i as u32).to_le_bytes());
        out.extend_from_slice(bytemuck::cast_slice(tile));
    });
}

impl Raster {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            colour: TileGrid::new(width, height),
            body: None,
        }
    }

    pub fn alpha_at(&self, x: f32, y: f32) -> f32 {
        if x < 0.0 || y < 0.0 || x >= self.width as f32 || y >= self.height as f32 {
            return 0.0;
        }
        let (x, y) = (x as usize, y as usize);
        self.get(x / TILE, y / TILE)
            .map_or(0.0, |t| f32::from(t[((y % TILE) * TILE + x % TILE) * 4 + 3]))
    }

    pub fn heights(&self) -> Option<&Heights> {
        self.body.as_ref().filter(|h| h.count() > 0)
    }

    pub fn heights_mut(&mut self) -> &mut Heights {
        let (w, h) = (self.colour.width, self.colour.height);
        self.body.get_or_insert_with(|| TileGrid::new(w, h))
    }

    pub fn height_tile(&self, index: usize) -> Option<&[f16]> {
        self.body.as_ref().and_then(|h| h.at(index))
    }

    pub fn height_tile_mut(&mut self, index: usize) -> Option<&mut [f16]> {
        self.body.as_mut().and_then(|h| h.at_mut(index))
    }

    pub fn has_height(&self) -> bool {
        self.heights().is_some()
    }

    pub fn clear(&mut self, index: usize) {
        self.colour.clear(index);
        if let Some(h) = self.body.as_mut() {
            h.clear(index);
        }
    }

    pub fn tile(&self, index: usize) -> Tile {
        (
            self.colour.at(index).map(Arc::from),
            self.height_tile(index).map(Arc::from),
        )
    }

    pub fn restore(&mut self, index: usize, (colour, height): &Tile) {
        match colour {
            Some(data) => self.colour.put(index, Box::from(&data[..])),
            None => self.colour.clear(index),
        }
        match (height, self.body.as_mut()) {
            (Some(data), _) => self.heights_mut().put(index, Box::from(&data[..])),
            (None, Some(h)) => h.clear(index),
            (None, None) => {}
        }
    }

    pub fn hash(&self) -> u64 {
        let colour = fold_tiles(&self.colour, FNV_OFFSET);
        match self.heights() {
            Some(h) => fold_tiles(h, fnv(colour, HEIGHT_MAGIC)),
            None => colour,
        }
    }

    pub fn height_bytes(&self) -> usize {
        self.heights().map_or(0, |h| h.bytes())
    }

    pub fn bytes(&self) -> usize {
        self.colour.bytes() + self.height_bytes()
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.bytes() + self.count() * 4);
        out.extend_from_slice(RASTER_MAGIC);
        [self.colour.width, self.colour.height]
            .iter()
            .for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        write_tiles(&self.colour, &mut out);
        if let Some(h) = self.heights() {
            out.extend_from_slice(HEIGHT_MAGIC);
            write_tiles(h, &mut out);
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let word = |at: usize| -> Result<u32> {
            match bytes.get(at..at + 4) {
                Some(b) => Ok(u32::from_le_bytes(b.try_into()?)),
                None => bail!("raster data ends early"),
            }
        };
        if bytes.get(0..4) != Some(RASTER_MAGIC) {
            bail!("not a raster file");
        }
        let mut raster = Raster::new(word(4)?, word(8)?);
        let tiles = raster.cols * raster.rows;
        let read = |from: usize, channels: usize, put: &mut dyn FnMut(usize, Box<[f16]>)| -> Result<usize> {
            let count = word(from)? as usize;
            let tile_bytes = TILE_PX * channels * 2;
            (0..count).try_for_each(|n| {
                let at = from + 4 + n * (4 + tile_bytes);
                let index = word(at)? as usize;
                let Some(data) = bytes.get(at + 4..at + 4 + tile_bytes) else {
                    bail!("raster tile {n} ends early");
                };
                if index >= tiles {
                    bail!("raster tile index {index} out of range");
                }
                let tile: Vec<f16> = bytemuck::pod_collect_to_vec(data);
                put(index, tile.into_boxed_slice());
                Ok(())
            })?;
            Ok(from + 4 + count * (4 + tile_bytes))
        };
        let end = read(12, 4, &mut |i, t| raster.colour.put(i, t))?;
        if bytes.get(end..end + 4) == Some(HEIGHT_MAGIC) {
            read(end + 4, 1, &mut |i, t| raster.heights_mut().put(i, t))?;
        }
        Ok(raster)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Flat,
    Colour,
    Replace,
}

pub struct Tiles<'a> {
    pub coverage: &'a mut [f32],
    pub paint: Option<&'a mut [f32]>,
    pub lift: Option<&'a mut [f32]>,
    pub ridge: Option<&'a mut [f32]>,
}

pub struct StrokeBuffer {
    grid: TileGrid<f32, 1>,
    paint: TileGrid<f32, 4>,
    lift: TileGrid<f32, 1>,
    ridge: TileGrid<f32, 1>,
    pub mode: Mode,
    pub body: f32,
    pub lifting: bool,
    touched: Vec<usize>,
    marked: Vec<bool>,
    rects: Vec<[u16; 4]>,
    dirty: Vec<usize>,
    dirty_marked: Vec<bool>,
    mask: Option<Vec<bool>>,
}

impl StrokeBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        let grid = TileGrid::new(width, height);
        let marked = vec![false; grid.cols * grid.rows];
        Self {
            grid,
            paint: TileGrid::new(width, height),
            lift: TileGrid::new(width, height),
            ridge: TileGrid::new(width, height),
            mode: Mode::Flat,
            body: 0.0,
            lifting: false,
            touched: Vec::new(),
            dirty_marked: marked.clone(),
            rects: vec![EMPTY; marked.len()],
            marked,
            dirty: Vec::new(),
            mask: None,
        }
    }

    pub fn set_mask(&mut self, mask: Option<Vec<bool>>) {
        self.mask = mask;
    }

    pub fn take_mask(&mut self) -> Option<Vec<bool>> {
        self.mask.take()
    }

    pub fn allowed(&self, index: usize) -> bool {
        self.mask.as_ref().is_none_or(|m| m[index])
    }

    pub fn skip(&mut self, index: usize) -> bool {
        if self.allowed(index) {
            return false;
        }
        if !self.marked[index] {
            self.marked[index] = true;
            self.touched.push(index);
        }
        true
    }

    pub fn grid(&self) -> &TileGrid<f32, 1> {
        &self.grid
    }

    pub fn tile_mut(&mut self, index: usize, rect: [usize; 4]) -> &mut [f32] {
        let r = &mut self.rects[index];
        *r = [
            r[0].min(rect[0] as u16),
            r[1].min(rect[1] as u16),
            r[2].max(rect[2] as u16),
            r[3].max(rect[3] as u16),
        ];
        if !self.marked[index] {
            self.marked[index] = true;
            self.touched.push(index);
        }
        if !self.dirty_marked[index] {
            self.dirty_marked[index] = true;
            self.dirty.push(index);
        }
        self.grid.ensure(index)
    }

    pub fn rect(&self, index: usize) -> Option<[usize; 4]> {
        let r = self.rects[index];
        (r[0] < r[2] && r[1] < r[3]).then(|| r.map(usize::from))
    }

    pub fn rows(&self, index: usize) -> impl Iterator<Item = std::ops::Range<usize>> + use<> {
        let r = self.rect(index).unwrap_or([0; 4]);
        (r[1]..r[3]).map(move |y| y * TILE + r[0]..y * TILE + r[2])
    }

    pub fn tiles_mut(&mut self, index: usize, rect: [usize; 4]) -> Tiles<'_> {
        self.tile_mut(index, rect);
        Tiles {
            coverage: self.grid.ensure(index),
            paint: match self.mode {
                Mode::Flat => None,
                _ => Some(self.paint.ensure(index)),
            },
            lift: match self.lifting {
                true => Some(self.lift.ensure(index)),
                false => None,
            },
            ridge: match self.body > 0.0 {
                true => Some(self.ridge.ensure(index)),
                false => None,
            },
        }
    }

    pub fn ridge(&self, index: usize) -> Option<&[f32]> {
        match self.body > 0.0 {
            true => self.ridge.at(index),
            false => None,
        }
    }

    pub fn paint(&self, index: usize) -> Option<&[f32]> {
        match self.mode {
            Mode::Flat => None,
            _ => self.paint.at(index),
        }
    }

    pub fn lift(&self, index: usize) -> Option<&[f32]> {
        match self.lifting {
            true => self.lift.at(index),
            false => None,
        }
    }

    pub fn take_dirty(&mut self) -> Vec<usize> {
        let dirty = std::mem::take(&mut self.dirty);
        dirty.iter().for_each(|&i| self.dirty_marked[i] = false);
        dirty
    }

    pub fn touched(&self) -> &[usize] {
        &self.touched
    }

    pub fn coverage(&self, index: usize) -> Option<&[f32]> {
        self.grid.at(index)
    }

    pub fn coverage_mut(&mut self, index: usize) -> Option<&mut [f32]> {
        self.grid.at_mut(index)
    }

    pub fn reset(&mut self) {
        let touched = std::mem::take(&mut self.touched);
        touched.iter().for_each(|&i| {
            self.marked[i] = false;
            let rows: Vec<std::ops::Range<usize>> = self.rows(i).collect();
            rows.iter().for_each(|row| {
                if let Some(tile) = self.grid.at_mut(i) {
                    tile[row.clone()].fill(0.0);
                }
                if let Some(tile) = self.paint.at_mut(i) {
                    tile[row.start * 4..row.end * 4].fill(0.0);
                }
                if let Some(tile) = self.lift.at_mut(i) {
                    tile[row.clone()].fill(0.0);
                }
                if let Some(tile) = self.ridge.at_mut(i) {
                    tile[row.clone()].fill(0.0);
                }
            });
            self.rects[i] = EMPTY;
        });
        self.touched = touched;
        self.touched.clear();
        self.mode = Mode::Flat;
        self.body = 0.0;
        self.lifting = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_covers_partial_tiles() {
        let grid = Raster::new(300, 513);
        assert_eq!((grid.cols, grid.rows), (2, 3));
        assert_eq!(grid.count(), 0);
    }

    #[test]
    fn raster_encode_round_trip() {
        let mut raster = Raster::new(600, 300);
        raster.ensure(4)[17] = f16::from_f32(0.25);
        raster.ensure(1)[3] = f16::from_f32(1.0);
        let back = Raster::decode(&raster.encode()).unwrap();
        assert_eq!(back.hash(), raster.hash());
        assert_eq!((back.width, back.height, back.count()), (600, 300, 2));
        assert!(!back.has_height());
        assert!(Raster::decode(&raster.encode()[..40]).is_err());
        let flat = raster.hash();
        raster.heights_mut().ensure(4)[17] = f16::from_f32(0.5);
        let back = Raster::decode(&raster.encode()).unwrap();
        assert_eq!(back.hash(), raster.hash());
        assert_ne!(raster.hash(), flat);
        assert_eq!(back.height_tile(4).unwrap()[17].to_f32(), 0.5);
        raster.clear(4);
        assert!(raster.height_tile(4).is_none());
    }

    #[test]
    fn masked_tiles_count_as_touched_but_get_no_pixels() {
        let mut buffer = StrokeBuffer::new(512, 512);
        let mut mask = vec![false; 4];
        mask[2] = true;
        buffer.set_mask(Some(mask));
        assert!(buffer.skip(1));
        assert!(!buffer.skip(2));
        buffer.tile_mut(2, [0, 0, 1, 1])[0] = 0.5;
        assert_eq!(buffer.touched(), &[1, 2]);
        assert!(buffer.coverage(1).is_none());
        buffer.reset();
        assert!(buffer.touched().is_empty());
    }

    #[test]
    fn buffer_reset_clears_only_touched() {
        let mut buffer = StrokeBuffer::new(512, 512);
        buffer.tile_mut(3, [10, 0, 12, 1])[10] = 0.5;
        buffer.tile_mut(3, [10, 0, 12, 1])[11] = 0.25;
        assert_eq!(buffer.touched(), &[3]);
        assert_eq!(buffer.take_dirty(), vec![3]);
        assert!(buffer.take_dirty().is_empty());
        buffer.reset();
        assert!(buffer.touched().is_empty());
        assert!(buffer.coverage(3).unwrap().iter().all(|&v| v == 0.0));
    }
}
