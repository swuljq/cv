use arboard::ImageData;
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader};
use sha2::{Digest, Sha256};
use std::{borrow::Cow, fs, io::{Cursor, Read}, path::Path};

pub const MAX_PNG_BYTES: usize = 10 * 1024 * 1024;
const MAX_SOURCE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;
const MAX_PIXELS: u64 = 40_000_000;
const THUMBNAIL_WIDTH: u32 = 320;
const THUMBNAIL_HEIGHT: u32 = 180;

#[derive(Clone, Debug)]
pub struct NormalizedImage {
    pub png: Vec<u8>,
    pub thumbnail: Vec<u8>,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
}

pub(crate) fn validate_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("图片尺寸不能为空".to_string());
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("图片尺寸超过同步限制".to_string());
    }
    Ok(())
}

fn encode_png(image: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).map_err(|error| error.to_string())?;
    let png = output.into_inner();
    if png.len() > MAX_PNG_BYTES {
        return Err("图片转换为 PNG 后超过 10 MiB".to_string());
    }
    Ok(png)
}

fn normalize_dynamic(image: DynamicImage) -> Result<NormalizedImage, String> {
    let (width, height) = image.dimensions();
    validate_dimensions(width, height)?;
    let png = encode_png(&image)?;
    let thumbnail_image = if width <= THUMBNAIL_WIDTH && height <= THUMBNAIL_HEIGHT {
        image.clone()
    } else {
        image.thumbnail(THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT)
    };
    let thumbnail = encode_png(&thumbnail_image)?;
    let sha256 = format!("{:x}", Sha256::digest(&png));
    Ok(NormalizedImage { png, thumbnail, sha256, width, height })
}

pub fn normalize_rgba(width: usize, height: usize, bytes: &[u8]) -> Result<NormalizedImage, String> {
    let width = u32::try_from(width).map_err(|_| "图片宽度无效".to_string())?;
    let height = u32::try_from(height).map_err(|_| "图片高度无效".to_string())?;
    validate_dimensions(width, height)?;
    let expected = usize::try_from(u64::from(width) * u64::from(height) * 4).map_err(|_| "图片数据过大".to_string())?;
    if bytes.len() != expected { return Err("图片像素数据长度无效".to_string()); }
    let rgba = image::RgbaImage::from_raw(width, height, bytes.to_vec()).ok_or_else(|| "无法读取图片像素".to_string())?;
    normalize_dynamic(DynamicImage::ImageRgba8(rgba))
}

pub fn normalize_encoded(bytes: &[u8]) -> Result<NormalizedImage, String> {
    let format = image::guess_format(bytes).map_err(|error| error.to_string())?;
    let reader = ImageReader::with_format(Cursor::new(bytes), format);
    let (width, height) = reader.into_dimensions().map_err(|error| error.to_string())?;
    validate_dimensions(width, height)?;
    let image = image::load_from_memory_with_format(bytes, format).map_err(|error| error.to_string())?;
    normalize_dynamic(image)
}

pub fn decode_wire(data: &str) -> Result<NormalizedImage, String> {
    if data.len() > ((MAX_PNG_BYTES + 2) / 3) * 4 {
        return Err("图片传输数据超过 10 MiB".to_string());
    }
    let bytes = STANDARD.decode(data).map_err(|error| format!("图片 Base64 无效：{error}"))?;
    if bytes.len() > MAX_PNG_BYTES { return Err("图片传输数据超过 10 MiB".to_string()); }
    if image::guess_format(&bytes).map_err(|error| error.to_string())? != ImageFormat::Png {
        return Err("图片传输格式必须为 PNG".to_string());
    }
    normalize_encoded(&bytes)
}

pub fn encode_wire(image: &NormalizedImage) -> String {
    STANDARD.encode(&image.png)
}

pub fn clipboard_data(image: &NormalizedImage) -> Result<ImageData<'static>, String> {
    let decoded = image::load_from_memory_with_format(&image.png, ImageFormat::Png).map_err(|error| error.to_string())?.to_rgba8();
    Ok(ImageData { width: image.width as usize, height: image.height as usize, bytes: Cow::Owned(decoded.into_raw()) })
}

fn supported_file(path: &Path) -> bool {
    matches!(path.extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase).as_deref(), Some("png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "tif" | "tiff"))
}

pub fn first_file_image() -> Option<Result<NormalizedImage, String>> {
    let files: Vec<String> = clipboard_win::get_clipboard(clipboard_win::formats::FileList).ok()?;
    let path = files.into_iter().map(std::path::PathBuf::from).find(|path| supported_file(path))?;
    Some((|| {
        let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
        if metadata.len() > MAX_SOURCE_BYTES { return Err("图片源文件超过 50 MiB".to_string()); }
        let mut bytes = Vec::new();
        fs::File::open(path).map_err(|error| error.to_string())?.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_SOURCE_BYTES { return Err("图片源文件超过 50 MiB".to_string()); }
        normalize_encoded(&bytes)
    })())
}

pub fn read_clipboard_image(clipboard: &mut arboard::Clipboard) -> Option<Result<NormalizedImage, String>> {
    if let Some(image) = first_file_image() { return Some(image); }
    clipboard.get_image().ok().map(|image| normalize_rgba(image.width, image.height, &image.bytes))
}

#[cfg(test)]
mod tests {
    use super::{decode_wire, encode_wire, normalize_encoded, normalize_rgba, validate_dimensions, MAX_PNG_BYTES};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use std::io::Cursor;

    fn sample_image() -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(3, 2, Rgba([40, 120, 220, 255])))
    }

    #[test]
    fn rgba_round_trip_produces_valid_wire_png() {
        let normalized = normalize_rgba(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255]).expect("normalize rgba");
        let decoded = decode_wire(&encode_wire(&normalized)).expect("decode wire image");
        assert_eq!((decoded.width, decoded.height), (2, 1));
        assert_eq!(decoded.sha256, normalized.sha256);
        assert!(!decoded.thumbnail.is_empty());
    }

    #[test]
    fn supported_source_formats_decode_to_png() {
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP, ImageFormat::Bmp, ImageFormat::Gif, ImageFormat::Tiff] {
            let mut encoded = Cursor::new(Vec::new());
            sample_image().write_to(&mut encoded, format).expect("encode source format");
            let normalized = normalize_encoded(&encoded.into_inner()).expect("decode source format");
            assert_eq!((normalized.width, normalized.height), (3, 2));
            assert_eq!(image::guess_format(&normalized.png).expect("normalized format"), ImageFormat::Png);
        }
    }

    #[test]
    fn invalid_and_oversized_wire_images_are_rejected() {
        assert!(decode_wire("not-base64").is_err());
        assert!(decode_wire(&STANDARD.encode(b"not an image")).is_err());
        assert!(decode_wire(&STANDARD.encode(vec![0; MAX_PNG_BYTES + 1])).is_err());
        assert!(validate_dimensions(16_385, 1).is_err());
        assert!(validate_dimensions(10_000, 5_000).is_err());
    }
}
