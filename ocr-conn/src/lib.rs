pub mod ocr;

use pdf2image::{Pages, RenderOptionsBuilder};
use std::env::current_dir;
use std::error::Error;
use std::fs::create_dir_all;
use std::path::PathBuf;
use std::sync::LazyLock;

pub static CURRENT_DIR: LazyLock<PathBuf> = LazyLock::new(|| current_dir().unwrap());

pub fn pdf_render_jpg(pdf_name: &str, bytes: Vec<u8>) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let pdf = pdf2image::PDF::from_bytes(bytes)?;
    let images = pdf.render(
        Pages::Range(1..=pdf.page_count()),
        RenderOptionsBuilder::default().pdftocairo(true).build()?,
    )?;
    let image_dir = CURRENT_DIR.join("images");
    if !image_dir.exists() {
        create_dir_all(image_dir)?;
    }
    Ok(images
        .into_iter()
        .enumerate()
        .filter_map(|(index, image)| {
            let path = CURRENT_DIR
                .join("images")
                .join(format!("{}_{index}.jpg", pdf_name));
            if image.save(&path).is_ok() {
                Some(path)
            } else {
                None
            }
        })
        .collect())
}
