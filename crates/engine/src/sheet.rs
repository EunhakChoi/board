//! A sheet: several captures side by side in one image, for reading a flow or a path at a glance.

use std::io::Cursor;

use charet_model::Actor;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::{self, FilterType};
use image::{Rgb, RgbImage};

use crate::policy::Command;
use crate::{Engine, EngineError, StoreError};

/// Height of each picture on the sheet.
const ROW_HEIGHT: u32 = 480;
const GAP: u32 = 24;
const PER_ROW: usize = 4;
const MAX_ITEMS: usize = 24;
/// Width of a placeholder (planned step or missing capture).
const PLACEHOLDER_WIDTH: u32 = 300;
const BACKGROUND: Rgb<u8> = Rgb([240, 240, 240]);
const PLACEHOLDER: Rgb<u8> = Rgb([214, 214, 214]);

/// One place on a sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SheetItem {
    /// A stored capture (`screen` or `story`).
    Capture { kind: String, target: String },
    /// A step that has no screen yet.
    Planned,
}

impl Engine {
    /// The captures of `items` in `theme`, left to right and wrapping every few, as one JPEG.
    /// Planned steps and targets with no capture get a grey placeholder.
    pub fn sheet(
        &self,
        actor: &Actor,
        key: &str,
        items: &[SheetItem],
        theme: &str,
    ) -> Result<Vec<u8>, EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        if items.is_empty() || items.len() > MAX_ITEMS {
            return Err(EngineError::Invalid(format!(
                "a sheet takes 1 to {MAX_ITEMS} items"
            )));
        }
        let mut pictures = Vec::with_capacity(items.len());
        for item in items {
            let picture = match item {
                SheetItem::Capture { kind, target } => {
                    match self.ports.captures.image(key, kind, target, theme)? {
                        Some(jpeg) => Some(scaled(&jpeg)?),
                        None => None,
                    }
                }
                SheetItem::Planned => None,
            };
            pictures.push(picture.unwrap_or_else(|| {
                RgbImage::from_pixel(PLACEHOLDER_WIDTH, ROW_HEIGHT, PLACEHOLDER)
            }));
        }
        encode(&compose(&pictures))
    }
}

fn scaled(jpeg: &[u8]) -> Result<RgbImage, EngineError> {
    let picture = image::load_from_memory(jpeg)
        .map_err(|e| EngineError::Store(StoreError(format!("capture image: {e}"))))?
        .to_rgb8();
    let width = (u64::from(picture.width()) * u64::from(ROW_HEIGHT)
        / u64::from(picture.height().max(1)))
    .clamp(1, 4096);
    Ok(imageops::resize(
        &picture,
        u32::try_from(width).unwrap_or(4096),
        ROW_HEIGHT,
        FilterType::Triangle,
    ))
}

fn compose(pictures: &[RgbImage]) -> RgbImage {
    let rows: Vec<&[RgbImage]> = pictures.chunks(PER_ROW).collect();
    let row_width =
        |row: &[RgbImage]| -> u32 { row.iter().map(|p| p.width() + GAP).sum::<u32>() + GAP };
    let width = rows.iter().map(|r| row_width(r)).max().unwrap_or(GAP);
    let count = u32::try_from(rows.len()).unwrap_or(1);
    let height = count * (ROW_HEIGHT + GAP) + GAP;
    let mut sheet = RgbImage::from_pixel(width, height, BACKGROUND);
    for (index, row) in (0u32..).zip(rows) {
        let mut x = GAP;
        let y = GAP + index * (ROW_HEIGHT + GAP);
        for picture in row {
            imageops::replace(&mut sheet, picture, i64::from(x), i64::from(y));
            x += picture.width() + GAP;
        }
    }
    sheet
}

fn encode(sheet: &RgbImage) -> Result<Vec<u8>, EngineError> {
    let mut out = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut out, 85)
        .encode_image(sheet)
        .map_err(|e| EngineError::Store(StoreError(format!("sheet image: {e}"))))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        encode(&RgbImage::from_pixel(width, height, Rgb([10, 20, 30]))).unwrap()
    }

    #[test]
    fn pictures_are_scaled_to_the_row_height() {
        let picture = scaled(&jpeg(1440, 900)).unwrap();
        assert_eq!((picture.width(), picture.height()), (768, ROW_HEIGHT));
    }

    #[test]
    fn rows_wrap_after_a_few_pictures() {
        let pictures = vec![RgbImage::new(100, ROW_HEIGHT); PER_ROW + 1];
        let sheet = compose(&pictures);
        let per_row = u32::try_from(PER_ROW).unwrap();
        assert_eq!(sheet.width(), per_row * (100 + GAP) + GAP);
        assert_eq!(sheet.height(), 2 * (ROW_HEIGHT + GAP) + GAP);
    }
}
