use image::imageops;
use image::metadata::Orientation;
use image::{codecs::jpeg::JpegDecoder, DynamicImage, ImageDecoder, ImageFormat};
use std::fs;
use std::io::Cursor;
use std::path::Path;

const CONTRAST_BOOST: f32 = 12.0;

/// 对字节数据进行预处理（旋转、对比度增强等）
pub fn preprocess_bytes(input: &[u8]) -> Option<Vec<u8>> {
    let format = image::guess_format(input).ok()?;
    let mut image = image::load_from_memory(input).ok()?;

    if let Some(orientation) = read_orientation(input, format) {
        image.apply_orientation(orientation);
    }

    // 直接转成 PNG，避免因原格式编码问题产生误报日志
    let enhanced = enhance_contrast(image);
    encode_image(&enhanced, ImageFormat::Png)
}

/// 直接修改文件（仅在生成的中间图片上使用）
pub fn preprocess_file_in_place(path: &Path) -> std::io::Result<bool> {
    let bytes = fs::read(path)?;
    let Some(processed) = preprocess_bytes(&bytes) else {
        return Ok(false);
    };
    fs::write(path, processed)?;
    Ok(true)
}

fn enhance_contrast(image: DynamicImage) -> DynamicImage {
    let rgba = image.to_rgba8();
    let adjusted = imageops::contrast(&rgba, CONTRAST_BOOST);
    DynamicImage::ImageRgba8(adjusted)
}

fn encode_image(image: &DynamicImage, format: ImageFormat) -> Option<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::new());
    let preferred = match format {
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Bmp | ImageFormat::Tiff => format,
        _ => ImageFormat::Png,
    };
    image.write_to(&mut cursor, preferred).ok()?;
    Some(cursor.into_inner())
}

fn read_orientation(bytes: &[u8], format: ImageFormat) -> Option<Orientation> {
    match format {
        ImageFormat::Jpeg => {
            let cursor = Cursor::new(bytes);
            let mut decoder = JpegDecoder::new(cursor).ok()?;
            decoder
                .orientation()
                .ok()
                .filter(|orientation| *orientation != Orientation::NoTransforms)
        }
        _ => None,
    }
}
