//! Step 3: writes finished pages into one PDF. Needs only `Page`s, never Vision,
//! so it can be tested with pages built by hand.

use std::{
    error::Error,
    fs::File,
    io::{BufWriter, Cursor},
    path::Path,
};

use image::{ImageFormat, RgbImage};
use printpdf::{
    Color, ColorBits, ColorSpace, Image, ImageFilter, ImageTransform, ImageXObject,
    IndirectFontRef, Mm, PdfDocument, PdfLayerReference, Px, Rgb, TextRenderingMode,
};
use ttf_parser::Face;

use super::{FONT, Line, PAGE_HEIGHT_MM, PAGE_WIDTH_MM, Page, is_letterhead, render};

#[cfg(test)]
mod tests;

/// Font size as a share of the height of a recognized line's box.
const FONT_SIZE_RATIO: f32 = 0.8;
/// Distance from the bottom of a line's box to the text baseline, as a share of its height.
const BASELINE_RATIO: f32 = 0.2;

/// Writes all pages into one PDF, in order, each one A4.
/// `pub`: a parent module can't see its child's private items, only the other way round.
pub fn write_pdf(pages: &[Page], pdf: &Path) -> Result<(), Box<dyn Error>> {
    let (doc, first_page, first_layer) =
        PdfDocument::new("OCR", Mm(PAGE_WIDTH_MM), Mm(PAGE_HEIGHT_MM), "text");
    // The font is added once and shared by all pages.
    let font_data = std::fs::read(FONT)?;
    let face = Face::parse(&font_data, 0)?; // used to measure text width
    let font = doc.add_external_font(font_data.as_slice())?;

    for (number, page) in pages.iter().enumerate() {
        // The document comes with its first page; every later one is added.
        let (page_index, layer_index) = if number == 0 {
            (first_page, first_layer)
        } else {
            doc.add_page(Mm(PAGE_WIDTH_MM), Mm(PAGE_HEIGHT_MM), "text")
        };
        let layer = doc.get_page(page_index).get_layer(layer_index);
        write_page(&layer, &font, &face, page)?;
    }

    doc.save(&mut BufWriter::new(File::create(pdf)?))?;
    Ok(())
}

/// Draws one page: the background picture, then every line at its position.
fn write_page(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    face: &Face,
    page: &Page,
) -> Result<(), Box<dyn Error>> {
    place_background(layer, &page.paper)?;
    for line in &page.lines {
        place_line(layer, font, face, line);
    }
    Ok(())
}

/// Puts the background picture across the whole page. Stored as JPEG to keep the PDF small.
fn place_background(layer: &PdfLayerReference, paper: &RgbImage) -> Result<(), Box<dyn Error>> {
    let mut jpeg = Vec::new();
    paper.write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)?;

    let picture = Image::from(ImageXObject {
        width: Px(paper.width() as usize),
        height: Px(paper.height() as usize),
        color_space: ColorSpace::Rgb,
        bits_per_component: ColorBits::Bit8,
        interpolate: true,
        image_data: jpeg,
        image_filter: Some(ImageFilter::DCT), // tells the PDF the data is JPEG
        clipping_bbox: None,
        smask: None,
    });
    // At the same DPI as it was drawn, the picture is exactly A4.
    let placement = ImageTransform {
        dpi: Some(render::DPI),
        ..Default::default()
    };
    picture.add_to_layer(layer.clone(), placement);
    Ok(())
}

/// Draws one line at its original position and about its original size.
fn place_line(layer: &PdfLayerReference, font: &IndirectFontRef, face: &Face, line: &Line) {
    let x = Mm(line.x * PAGE_WIDTH_MM);
    let y = Mm((line.y + line.height * BASELINE_RATIO) * PAGE_HEIGHT_MM);
    // Letterhead text stays visible in the background picture; ours is only added
    // invisibly there, so the PDF can still be searched.
    let mode = if is_letterhead(line) {
        TextRenderingMode::Invisible
    } else {
        TextRenderingMode::Fill
    };
    layer.set_text_rendering_mode(mode);

    // PDF colours go from 0.0 to 1.0 instead of 0 to 255.
    let [r, g, b] = line.colour.map(|c| c as f32 / 255.0);
    layer.set_fill_color(Color::Rgb(Rgb::new(r, g, b, None)));
    layer.use_text(&line.text, font_size(face, line), x, y, font);
}

/// The font size for a line, in points. It comes from the height of the line's box;
/// if the text would then be wider than the box, it shrinks until it fits, so lines
/// never run into each other.
fn font_size(face: &Face, line: &Line) -> f32 {
    let box_width_pt = mm_to_pt(line.width * PAGE_WIDTH_MM);
    let box_height_pt = mm_to_pt(line.height * PAGE_HEIGHT_MM);

    let size = box_height_pt * FONT_SIZE_RATIO;
    let text_width = text_width_pt(face, &line.text, size);
    if text_width > box_width_pt {
        size * box_width_pt / text_width
    } else {
        size
    }
}

/// How wide `text` is, in points, when written in `face` at `font_size`.
fn text_width_pt(face: &Face, text: &str, font_size: f32) -> f32 {
    let units: u32 = text
        .chars()
        .filter_map(|c| face.glyph_index(c))
        .filter_map(|glyph| face.glyph_hor_advance(glyph))
        .map(u32::from)
        .sum();
    units as f32 / face.units_per_em() as f32 * font_size
}

/// Converts millimetres to points (1 inch = 25.4 mm = 72 points).
fn mm_to_pt(mm: f32) -> f32 {
    mm * 72.0 / 25.4
}
