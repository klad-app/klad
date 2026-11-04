use anyhow::{Context, Result, anyhow, bail};
use klad_core::bundle::write_atomic;
use klad_core::rng::{FNV_OFFSET, fnv};
use skia_safe::{AlphaType, ColorSpace, ColorType, Data, Image, ImageInfo, images};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const IMAGES: &str = "images";
pub const FONTS: &str = "fonts";

static ROOTS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static PICTURES: Mutex<BTreeMap<String, Image>> = Mutex::new(BTreeMap::new());
static TYPEFACES: Mutex<BTreeMap<String, Arc<Vec<u8>>>> = Mutex::new(BTreeMap::new());

pub fn add_root(root: &Path) {
    let mut roots = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    if !roots.iter().any(|r| r == root) {
        roots.push(root.to_path_buf());
    }
}

fn find(folder: &str, name: &str) -> Option<PathBuf> {
    let added = ROOTS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    added
        .into_iter()
        .chain(klad_core::bundle::opened())
        .map(|root| root.join(folder).join(name))
        .find(|p| p.exists())
}

pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn decode(bytes: &[u8]) -> Result<Rgba> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => decode_png(bytes),
        [0xFF, 0xD8, ..] => decode_jpeg(bytes),
        _ => bail!("only PNG and JPEG pictures can be imported"),
    }
}

fn decode_png(bytes: &[u8]) -> Result<Rgba> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info()?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| anyhow!("the picture is too big"))?;
    let mut buf = vec![0u8; size];
    let info = reader.next_frame(&mut buf)?;
    let (w, h) = (info.width, info.height);
    let channels = info.color_type.samples();
    let pixels = buf[..(w * h) as usize * channels]
        .chunks_exact(channels)
        .flat_map(|px| match channels {
            2 => [px[0], px[0], px[0], px[1]],
            4 => [px[0], px[1], px[2], px[3]],
            3 => [px[0], px[1], px[2], 255],
            _ => [px[0], px[0], px[0], 255],
        })
        .collect();
    Ok(Rgba {
        width: w,
        height: h,
        pixels,
    })
}

fn decode_jpeg(bytes: &[u8]) -> Result<Rgba> {
    use zune_jpeg::zune_core::colorspace::ColorSpace as Space;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let options = DecoderOptions::default().jpeg_set_out_colorspace(Space::RGBA);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(std::io::Cursor::new(bytes), options);
    let pixels = decoder.decode().map_err(|e| anyhow!("reading the JPEG: {e:?}"))?;
    let info = decoder.info().ok_or_else(|| anyhow!("the JPEG has no size"))?;
    Ok(Rgba {
        width: info.width as u32,
        height: info.height as u32,
        pixels,
    })
}

pub fn encode_png(rgba: &Rgba) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, rgba.width, rgba.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Balanced);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&rgba.pixels)?;
    }
    Ok(out)
}

pub fn import_image(root: &Path, bytes: &[u8]) -> Result<(String, u32, u32)> {
    let rgba = decode(bytes)?;
    if rgba.width == 0 || rgba.height == 0 {
        bail!("the picture is empty");
    }
    let png = encode_png(&rgba)?;
    let name = format!("{:016x}.png", fnv(FNV_OFFSET, &png));
    let path = root.join(IMAGES).join(&name);
    if !path.exists() {
        write_atomic(&path, &png)?;
    }
    Ok((name, rgba.width, rgba.height))
}

pub fn import_font(root: &Path, file: &Path) -> Result<String> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    crate::text::parse(&bytes).map_err(|e| anyhow!("{} is not a font this can read: {e}", file.display()))?;
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("{} has no file name", file.display()))?
        .to_string();
    let path = root.join(FONTS).join(&name);
    if path.exists() && std::fs::read(&path)? != bytes {
        bail!("the bundle already has another font called {name}; rename the file");
    }
    write_atomic(&path, &bytes)?;
    Ok(name)
}

pub fn image(src: &str) -> Result<Image> {
    if let Some(found) = PICTURES.lock().unwrap_or_else(|e| e.into_inner()).get(src) {
        return Ok(found.clone());
    }
    let path = find(IMAGES, src).ok_or_else(|| anyhow!("the picture {src} is not in the bundle's images folder"))?;
    let rgba = decode(&std::fs::read(&path)?).with_context(|| format!("picture {src}"))?;
    let info = ImageInfo::new(
        (rgba.width as i32, rgba.height as i32),
        ColorType::RGBA8888,
        AlphaType::Unpremul,
        ColorSpace::new_srgb(),
    );
    let made = images::raster_from_data(&info, Data::new_copy(&rgba.pixels), rgba.width as usize * 4)
        .and_then(|i| i.with_default_mipmaps())
        .ok_or_else(|| anyhow!("cannot hold the picture {src}"))?;
    PICTURES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(src.to_string(), made.clone());
    Ok(made)
}

pub fn font(name: &str) -> Result<Arc<Vec<u8>>> {
    if let Some(found) = TYPEFACES.lock().unwrap_or_else(|e| e.into_inner()).get(name) {
        return Ok(found.clone());
    }
    let path = find(FONTS, name)
        .ok_or_else(|| anyhow!("the font {name} is not in the bundle's fonts folder; add it with klad font"))?;
    let bytes = Arc::new(std::fs::read(&path)?);
    crate::text::parse(&bytes).map_err(|e| anyhow!("the font {name} cannot be read: {e}"))?;
    TYPEFACES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(name.to_string(), bytes.clone());
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_stored_once_by_its_content_and_reads_back() {
        let root = std::env::temp_dir().join(format!("draw-assets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rgba = Rgba {
            width: 3,
            height: 2,
            pixels: (0..24).map(|i| (i * 10) as u8).collect(),
        };
        let png = encode_png(&rgba).unwrap();
        let (a, w, h) = import_image(&root, &png).unwrap();
        let (b, _, _) = import_image(&root, &png).unwrap();
        assert_eq!((a.clone(), w, h), (b, 3, 2));
        assert_eq!(std::fs::read_dir(root.join(IMAGES)).unwrap().count(), 1);
        add_root(&root);
        let image = image(&a).unwrap();
        assert_eq!((image.width(), image.height()), (3, 2));
        assert!(decode(b"GIF89a").is_err());
        assert!(font("nope.ttf").unwrap_err().to_string().contains("nope.ttf"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
