#![allow(warnings)]

use crate::CURRENT_DIR;
use crossbeam::channel::{self, bounded};
use crossbeam::channel::{Receiver, Sender};
use image::EncodableLayout;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::LazyLock;
use std::time::Duration;
use std::{fmt, io, process};

type Point = [usize; 2];

#[derive(Deserialize, Debug, Clone)]
pub struct Content {
    code: u32,
    data: Vec<ContentData>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ContentData {
    #[serde(rename = "box")]
    pub rect: Rectangle,
    pub score: f64,
    pub text: String,
}

pub type Rectangle = [Point; 4];

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ImageData {
    ImagePathDict { image_path: String },
    ImageBase64Dict { image_base64: String },
}

impl ImageData {
    pub fn from_path<S>(path: S) -> ImageData
    where
        S: AsRef<str> + fmt::Display,
    {
        ImageData::ImagePathDict {
            image_path: path.to_string(),
        }
    }

    pub fn from_base64(base64: String) -> ImageData {
        ImageData::ImageBase64Dict {
            image_base64: base64,
        }
    }

    pub fn from_bytes<T>(bytes: T) -> ImageData
    where
        T: AsRef<[u8]>,
    {
        use base64::Engine;
        let engine = base64::engine::general_purpose::STANDARD;
        ImageData::ImageBase64Dict {
            image_base64: engine.encode(bytes),
        }
    }
}

impl From<&Path> for ImageData {
    fn from(path: &Path) -> Self {
        ImageData::from_path(path.to_string_lossy())
    }
}

impl From<PathBuf> for ImageData {
    fn from(path: PathBuf) -> Self {
        ImageData::from_path(path.to_string_lossy())
    }
}

impl From<Vec<u8>> for ImageData {
    fn from(value: Vec<u8>) -> Self {
        ImageData::from_bytes(value.as_bytes())
    }
}

pub struct Extractor {
    process: Child,
    receiver: Receiver<String>,
}

impl Extractor {
    pub fn new() -> io::Result<Self> {
        let path = CURRENT_DIR.join("ocr");
        let mut process = Command::new(path.join("PaddleOCR-json"))
            .env("LD_LIBRARY_PATH", path.join("lib"))
            .current_dir(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let (sender, receiver) = bounded::<String>(0);
        let stdout = Mutex::new(process.stdout.take().unwrap());
        rayon::spawn(move || {
            let stdout = stdout;
            let mut guard = stdout.lock();
            let mut stdout = BufReader::new(guard.deref_mut());
            let mut content = String::new();
            while stdout.read_line(&mut content).is_ok() {
                if sender.send(content.to_string()).is_err() {
                    break;
                }
                content.clear();
            }
        });
        while receiver.recv_timeout(Duration::from_secs(1)).is_ok() {}

        Ok(Self { process, receiver })
    }

    fn read_line(&mut self) -> String {
        let Ok(content) = self.receiver.recv_timeout(Duration::from_secs(3)) else {
            return String::new();
        };
        content
    }

    #[inline]
    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
        let inner = self
            .process
            .stdin
            .as_mut()
            .ok_or(io::Error::new(io::ErrorKind::Other, "stdin not piped"))?;
        inner.write_fmt(fmt)
    }

    pub fn ocr(&mut self, image: ImageData) -> io::Result<String> {
        let s = serde_json::to_string(&image)?;
        self.write_fmt(format_args!("{}\n", s.trim()))?;
        Ok(self.read_line())
    }

    pub fn ocr_and_parse(&mut self, image: ImageData) -> Result<Vec<ContentData>, String> {
        let ocr_result = self.ocr(image);
        let Ok(ocr_string) = ocr_result.as_ref() else {
            return Err("OCR failed".to_string());
        };
        match serde_json::from_str::<Content>(ocr_string) {
            Ok(content) => Ok(content.data),
            Err(e) => Err(format!("Response JSON parse failed: {}", e)),
        }
    }
}

impl Drop for Extractor {
    fn drop(&mut self) {
        self.process.kill().ok();
    }
}
