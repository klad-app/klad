use anyhow::{Result, bail};
use half::f16;
use klad_core::rng::{FNV_OFFSET, fnv};
use std::sync::Arc;

pub const TILE: usize = 256;
pub const TILE_PX: usize = TILE * TILE;

#[derive(Clone)]
pub struct TileGrid<T, const C: usize> {
    pub width: u32,
    pub height: u32,
    pub cols: usize,
    pub rows: usize,
    tiles: Vec<Option<Arc<Box<[T]>>>>,
}

pub type Raster = TileGrid<f16, 4>;

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

impl Raster {
    pub fn hash(&self) -> u64 {
        self.occupied().fold(FNV_OFFSET, |h, (i, tile)| {
            fnv(fnv(h, &(i as u64).to_le_bytes()), bytemuck::cast_slice(tile))
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let header = [self.width, self.height, self.count() as u32];
        let mut out = Vec::with_capacity(16 + self.bytes() + self.count() * 4);
        out.extend_from_slice(RASTER_MAGIC);
        header.iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        self.occupied().for_each(|(i, tile)| {
            out.extend_from_slice(&(i as u32).to_le_bytes());
            out.extend_from_slice(bytemuck::cast_slice(tile));
        });
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
        let count = word(12)? as usize;
        let tile_bytes = TILE_PX * 4 * 2;
        (0..count).try_for_each(|n| {
            let at = 16 + n * (4 + tile_bytes);
            let index = word(at)? as usize;
            let Some(data) = bytes.get(at + 4..at + 4 + tile_bytes) else {
                bail!("raster tile {n} ends early");
            };
            if index >= raster.cols * raster.rows {
                bail!("raster tile index {index} out of range");
            }
            let tile: Vec<f16> = bytemuck::pod_collect_to_vec(data);
            raster.put(index, tile.into_boxed_slice());
            Ok(())
        })?;
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

pub struct StrokeBuffer {
    grid: TileGrid<f32, 1>,
    paint: TileGrid<f32, 4>,
    pub mode: Mode,
    touched: Vec<usize>,
    marked: Vec<bool>,
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
            mode: Mode::Flat,
            touched: Vec::new(),
            dirty_marked: marked.clone(),
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

    pub fn tile_mut(&mut self, index: usize) -> &mut [f32] {
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

    pub fn tiles_mut(&mut self, index: usize) -> (&mut [f32], Option<&mut [f32]>) {
        self.tile_mut(index);
        let paint = match self.mode {
            Mode::Flat => None,
            _ => Some(self.paint.ensure(index)),
        };
        (self.grid.ensure(index), paint)
    }

    pub fn paint(&self, index: usize) -> Option<&[f32]> {
        match self.mode {
            Mode::Flat => None,
            _ => self.paint.at(index),
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
            if let Some(tile) = self.grid.at_mut(i) {
                tile.fill(0.0);
            }
            if let Some(tile) = self.paint.at_mut(i) {
                tile.fill(0.0);
            }
        });
        self.touched = touched;
        self.touched.clear();
        self.mode = Mode::Flat;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_covers_partial_tiles() {
        let grid: Raster = TileGrid::new(300, 513);
        assert_eq!((grid.cols, grid.rows), (2, 3));
        assert_eq!(grid.count(), 0);
    }

    #[test]
    fn raster_encode_round_trip() {
        let mut raster: Raster = TileGrid::new(600, 300);
        raster.ensure(4)[17] = f16::from_f32(0.25);
        raster.ensure(1)[3] = f16::from_f32(1.0);
        let back = Raster::decode(&raster.encode()).unwrap();
        assert_eq!(back.hash(), raster.hash());
        assert_eq!((back.width, back.height, back.count()), (600, 300, 2));
        assert!(Raster::decode(&raster.encode()[..40]).is_err());
    }

    #[test]
    fn masked_tiles_count_as_touched_but_get_no_pixels() {
        let mut buffer = StrokeBuffer::new(512, 512);
        let mut mask = vec![false; 4];
        mask[2] = true;
        buffer.set_mask(Some(mask));
        assert!(buffer.skip(1));
        assert!(!buffer.skip(2));
        buffer.tile_mut(2)[0] = 0.5;
        assert_eq!(buffer.touched(), &[1, 2]);
        assert!(buffer.coverage(1).is_none());
        buffer.reset();
        assert!(buffer.touched().is_empty());
    }

    #[test]
    fn buffer_reset_clears_only_touched() {
        let mut buffer = StrokeBuffer::new(512, 512);
        buffer.tile_mut(3)[10] = 0.5;
        buffer.tile_mut(3)[11] = 0.25;
        assert_eq!(buffer.touched(), &[3]);
        assert_eq!(buffer.take_dirty(), vec![3]);
        assert!(buffer.take_dirty().is_empty());
        buffer.reset();
        assert!(buffer.touched().is_empty());
        assert!(buffer.coverage(3).unwrap().iter().all(|&v| v == 0.0));
    }
}
