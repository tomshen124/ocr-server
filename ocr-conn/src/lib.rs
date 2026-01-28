pub mod ocr;
pub mod preprocess;

use pdf2image::{Pages, RenderOptionsBuilder, DPI};
use std::env::current_dir;
use std::error::Error;
use std::fs::create_dir_all;
use std::path::PathBuf;
use std::sync::LazyLock;

pub static CURRENT_DIR: LazyLock<PathBuf> = LazyLock::new(|| current_dir().unwrap());

// PDF处理安全限制（默认值；推荐由上层配置约束下载与页数）
const DEFAULT_MAX_PDF_PAGES: u32 = 100;        // 默认最大页数
const DEFAULT_MAX_PDF_SIZE_MB: usize = 40;     // 默认最大文件大小(MB)

/// 带上层限制的PDF渲染
pub fn pdf_render_jpg_with_limits(
    pdf_name: &str,
    bytes: Vec<u8>,
    max_pages: u32,
    max_size_mb: usize,
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    // 🚨 文件大小检查
    let file_size_mb = bytes.len() / 1024 / 1024;
    if file_size_mb > max_size_mb {
        return Err(format!(
            "PDF文件大小{}MB超过限制{}MB，拒绝处理",
            file_size_mb, max_size_mb
        )
        .into());
    }

    let pdf = pdf2image::PDF::from_bytes(bytes)?;

    // 🚨 页数检查与限制（大于限制则仅渲染前 max_pages 页）
    let page_count = pdf.page_count();
    let render_pages = if page_count > max_pages {
        eprintln!(
            "⚠️ PDF页数{}超过限制{}，仅渲染前{}页",
            page_count, max_pages, max_pages
        );
        max_pages
    } else {
        page_count
    };

    eprintln!("✅ PDF安全检查通过: {}页, {}MB", page_count, file_size_mb);

    let images = pdf.render(
        Pages::Range(1..=render_pages),
        RenderOptionsBuilder::default()
            .pdftocairo(true)
            .build()?,
    )?;
    let image_dir = CURRENT_DIR.join("images");
    if !image_dir.exists() {
        create_dir_all(image_dir)?;
    }
    // Sanitize filename stem to avoid path traversal and invalid chars
    let safe_stem = {
        use std::ffi::OsStr;
        let p = PathBuf::from(pdf_name);
        let stem_os = p.file_stem().unwrap_or(OsStr::new("document"));
        let stem_owned: String = stem_os.to_string_lossy().into_owned();
        // allow only [A-Za-z0-9_-], replace others with '_', and clamp length
        let mut out = String::with_capacity(stem_owned.len());
        for ch in stem_owned.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { out.push(ch); }
            else { out.push('_'); }
        }
        if out.is_empty() { out.push_str("document"); }
        if out.len() > 64 { out.truncate(64); }
        out
    };

    Ok(images
        .into_iter()
        .enumerate()
        .filter_map(|(index, image)| {
            let filename = format!("{}_{index}.jpg", safe_stem);
            let path = CURRENT_DIR.join("images").join(filename);
            if image.save(&path).is_ok() { Some(path) } else { None }
        })
        .collect())
}

/// 兼容旧接口（使用默认限制）
pub fn pdf_render_jpg(pdf_name: &str, bytes: Vec<u8>) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    pdf_render_jpg_with_limits(
        pdf_name,
        bytes,
        DEFAULT_MAX_PDF_PAGES,
        DEFAULT_MAX_PDF_SIZE_MB,
    )
}

/// 获取PDF页数
pub fn pdf_page_count(bytes: &[u8]) -> Result<u32, Box<dyn Error>> {
    let pdf = pdf2image::PDF::from_bytes(bytes.to_vec())?;
    Ok(pdf.page_count())
}

/// 按页范围渲染JPG（包含大小限制检查）
pub fn pdf_render_jpg_range(
    pdf_name: &str,
    bytes: &[u8],
    start_page: u32,
    end_page: u32,
    max_size_mb: usize,
    dpi: u32,
    jpeg_quality: Option<u8>,
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    if start_page == 0 || end_page < start_page {
        return Err("invalid page range".into());
    }
    let file_size_mb = bytes.len() / 1024 / 1024;
    if file_size_mb > max_size_mb {
        return Err(format!(
            "PDF文件大小{}MB超过限制{}MB，拒绝处理",
            file_size_mb, max_size_mb
        )
        .into());
    }

    let pdf = pdf2image::PDF::from_bytes(bytes.to_vec())?;
    let total = pdf.page_count();
    let start = start_page.min(total).max(1);
    let end = end_page.min(total).max(start);

    let dpi = dpi.clamp(72, 600);
    let mut binding = RenderOptionsBuilder::default();
    let builder = binding.pdftocairo(true).resolution(DPI::Uniform(dpi));
    let images = pdf.render(
        Pages::Range(start..=end),
        builder.build()?,
    )?;

    let image_dir = CURRENT_DIR.join("images");
    if !image_dir.exists() {
        create_dir_all(&image_dir)?;
    }

    // 安全文件名前缀
    let safe_stem = {
        use std::ffi::OsStr;
        let p = PathBuf::from(pdf_name);
        let stem_os = p.file_stem().unwrap_or(OsStr::new("document"));
        let stem_owned: String = stem_os.to_string_lossy().into_owned();
        let mut out = String::with_capacity(stem_owned.len());
        for ch in stem_owned.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { out.push(ch); }
            else { out.push('_'); }
        }
        if out.is_empty() { out.push_str("document"); }
        if out.len() > 64 { out.truncate(64); }
        out
    };

    use std::fs::File;
    use std::io::BufWriter;
    use image::codecs::jpeg::JpegEncoder;

    let mut out_paths = Vec::new();
    for (idx, img) in images.into_iter().enumerate() {
        let abs_index = start as usize + idx;
        let filename = format!("{}_{abs_index}.jpg", safe_stem);
        let path = image_dir.join(filename);
        if let Some(q) = jpeg_quality {
            let file = match File::create(&path) { Ok(f) => f, Err(_) => continue };
            let mut writer = BufWriter::new(file);
            let mut encoder = JpegEncoder::new_with_quality(&mut writer, q);
            if encoder.encode_image(&img).is_ok() { out_paths.push(path); }
        } else {
            if img.save(&path).is_ok() { out_paths.push(path); }
        }
    }
    Ok(out_paths)
}
