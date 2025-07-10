use crate::util::WebResult;
use axum::extract::Multipart;
use ocr_conn::ocr::Extractor;
use std::path::PathBuf;

pub async fn upload(mut multipart: Multipart) -> anyhow::Result<WebResult> {
    let mut data = vec![];
    let mut ocr = Extractor::new()?;
    while let Some(field) = multipart.next_field().await? {
        let file = PathBuf::from(field.file_name().unwrap_or_default());
        let bytes = field.bytes().await?;
        if file
            .extension()
            .is_some_and(|ext| ext.to_string_lossy().eq("pdf"))
        {
            let Ok(image_paths) =
                ocr_conn::pdf_render_jpg(file.to_str().unwrap_or_default(), bytes.to_vec())
            else {
                continue;
            };
            for image in image_paths {
                let Ok(contents) = ocr.ocr_and_parse(image.into()) else {
                    continue;
                };
                data.extend(contents.into_iter().map(|content| content.text));
            }
        } else {
            let Ok(contents) = ocr.ocr_and_parse(bytes.to_vec().into()) else {
                continue;
            };
            data.extend(contents.into_iter().map(|content| content.text));
        }
    }
    Ok(WebResult::ok(data))
}
