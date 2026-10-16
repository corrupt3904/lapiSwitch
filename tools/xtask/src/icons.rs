use image::{
    ImageEncoder, RgbaImage,
    codecs::png::PngEncoder,
    imageops::{self, FilterType},
};
use std::{fs, path::Path};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
fn encode(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut png = Vec::new();
    PngEncoder::new(&mut png).write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(png)
}
pub fn generate(source: &Path, output: &Path) -> Result<()> {
    let original = image::open(source)?.into_rgba8();
    if original.width() != original.height() || original.width() == 0 {
        return Err("Source icon must be nonempty and square".into());
    }
    let mut frames = Vec::new();
    for size in SIZES {
        frames.push(encode(&imageops::resize(
            &original,
            size,
            size,
            FilterType::Lanczos3,
        ))?);
    }
    let mut ico = Vec::new();
    for word in [0_u16, 1, SIZES.len() as u16] {
        ico.extend_from_slice(&word.to_le_bytes());
    }
    let mut offset = 6 + 16 * SIZES.len() as u32;
    for (size, frame) in SIZES.into_iter().zip(&frames) {
        let dimension = if size == 256 { 0 } else { size as u8 };
        ico.extend_from_slice(&[dimension, dimension, 0, 0]);
        ico.extend_from_slice(&1_u16.to_le_bytes());
        ico.extend_from_slice(&32_u16.to_le_bytes());
        ico.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += frame.len() as u32;
    }
    for frame in &frames {
        ico.extend_from_slice(frame);
    }
    fs::create_dir_all(output)?;
    fs::write(output.join("lapiswitch.png"), frames.last().unwrap())?;
    fs::write(output.join("lapiswitch.ico"), ico)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_ico_frames_decode_with_the_declared_sizes_and_offsets() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.png");
        RgbaImage::from_fn(64, 64, |x, y| image::Rgba([x as u8, y as u8, 127, 255]))
            .save(&source)
            .unwrap();
        let out = dir.path().join("icons");
        generate(&source, &out).unwrap();
        let ico = fs::read(out.join("lapiswitch.ico")).unwrap();
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 7, 0]);
        let mut expected_offset = 6 + SIZES.len() * 16;
        for (i, size) in SIZES.into_iter().enumerate() {
            let entry = &ico[6 + i * 16..22 + i * 16];
            let len = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
            let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
            assert_eq!(offset, expected_offset);
            let image = image::load_from_memory(&ico[offset..offset + len]).unwrap();
            assert_eq!((image.width(), image.height()), (size, size));
            expected_offset += len;
        }
        assert_eq!(expected_offset, ico.len());
        let png = image::open(out.join("lapiswitch.png")).unwrap();
        assert_eq!((png.width(), png.height()), (256, 256));
    }
}
