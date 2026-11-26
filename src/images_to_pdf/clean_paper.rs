//! Cleans the page picture: white paper without shadows, the colours of logos, lines and
//! stamps kept, and the original text erased (the PDF draws it again, sharp).
//! Runs after Vision has read the text, because erasing needs the lines' boxes.

use image::{
    Rgb, RgbImage,
    imageops::{self, FilterType},
};

use super::Line;

// The tests live in `src/pipeline/clean_paper/tests.rs`.
#[cfg(test)]
mod tests;

/// Side of the squares used to measure the paper's brightness, in pixels.
const BLOCK: u32 = 64;
/// A pixel darker than this (average of red, green, blue; 0 to 255) counts as ink.
const INK_DARKNESS: u8 = 160;
/// A pixel whose strongest and weakest colour channel differ by more than this counts
/// as ink too: that catches coloured logos and stamps, even pale ones.
const INK_COLOURFULNESS: u8 = 50;
/// Extra space around a line's box, as a share of the line's height, so that no edges
/// of the original letters are left over.
const ERASE_MARGIN: f32 = 0.25;
/// Share of a line's pixels, the darkest ones, used to find the colour of its text.
const DARKEST_SHARE: f32 = 0.1;

/// A rectangle of pixels in the picture, counted from the top-left corner.
/// `right` and `bottom` lie just outside it, so `left..right` are exactly its columns.
/// `derive(Debug, PartialEq)` lets tests compare two areas and print them.
#[derive(Debug, PartialEq)]
struct Area {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

/// Makes the background pure white (paper, shadows, texture), keeping only ink:
/// lines, logos and stamps, in their original colours.
pub fn whiten_paper(picture: &mut RgbImage) {
    let paper = paper_colour(picture);
    for (pixel, paper_pixel) in picture.pixels_mut().zip(paper.pixels()) {
        // Divide by the paper's colour here: paper becomes white, ink keeps its colour.
        for (channel, paper_channel) in pixel.0.iter_mut().zip(paper_pixel.0) {
            let value = *channel as f32 * 255.0 / paper_channel.max(1) as f32;
            *channel = value.min(255.0) as u8;
        }
        // Everything that is not ink becomes white.
        if !is_ink(pixel.0) {
            pixel.0 = [255, 255, 255];
        }
    }
}

/// True if a pixel is clearly dark or clearly coloured, i.e. part of something printed.
fn is_ink([r, g, b]: [u8; 3]) -> bool {
    let brightness = (r as u32 + g as u32 + b as u32) / 3;
    let colourfulness = r.max(g).max(b) - r.min(g).min(b);
    brightness < INK_DARKNESS as u32 || colourfulness > INK_COLOURFULNESS
}

/// Finds the text colour of every line: the average of the darkest pixels in its box.
pub fn set_text_colours(picture: &RgbImage, lines: &mut [Line]) {
    for line in lines {
        line.colour = darkest_colour(picture, &line_area(picture, line));
    }
}

/// Paints the box of every line white, so only logos, lines and stamps stay.
/// Lines in the letterhead are skipped: they are part of logos.
pub fn erase_text(picture: &mut RgbImage, lines: &[Line]) {
    for line in lines {
        if super::is_letterhead(line) {
            continue;
        }
        let area = line_area(picture, line);
        for y in area.top..area.bottom {
            for x in area.left..area.right {
                picture.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
    }
}

/// The pixels a line covers in the picture, with a small margin around it.
fn line_area(picture: &RgbImage, line: &Line) -> Area {
    let (width, height) = (picture.width() as f32, picture.height() as f32);
    let margin = line.height * height * ERASE_MARGIN;

    let left = (line.x * width - margin).max(0.0);
    let right = ((line.x + line.width) * width + margin).min(width);
    // Lines are measured from the bottom of the page, picture rows from the top: flip.
    let top = ((1.0 - line.y - line.height) * height - margin).max(0.0);
    let bottom = ((1.0 - line.y) * height + margin).min(height);

    // `as u32` cuts off the part after the point, e.g. 18.75 becomes 18.
    Area {
        left: left as u32,
        top: top as u32,
        right: right as u32,
        bottom: bottom as u32,
    }
}

/// The average colour of the darkest pixels in an area; black if the area is empty.
fn darkest_colour(picture: &RgbImage, area: &Area) -> [u8; 3] {
    let mut pixels: Vec<[u8; 3]> = Vec::new();
    for y in area.top..area.bottom {
        for x in area.left..area.right {
            pixels.push(picture.get_pixel(x, y).0);
        }
    }
    if pixels.is_empty() {
        return [0, 0, 0];
    }

    // Sort from dark to bright (by the sum of red, green and blue), keep the darkest part.
    pixels.sort_by_key(|p| p.iter().map(|&c| c as u32).sum::<u32>());
    let count = ((pixels.len() as f32 * DARKEST_SHARE) as usize).max(1);
    let darkest = &pixels[..count];

    // Average each channel separately.
    let average = |channel: usize| {
        let sum: u32 = darkest.iter().map(|p| p[channel] as u32).sum();
        (sum / count as u32) as u8
    };
    [average(0), average(1), average(2)]
}

/// Estimates the colour of the bare paper at every pixel. In each square the brightest
/// value is taken (that's paper, not ink); then the squares are blended back to full size.
fn paper_colour(picture: &RgbImage) -> RgbImage {
    let (width, height) = picture.dimensions();
    let mut squares = RgbImage::new(width.div_ceil(BLOCK), height.div_ceil(BLOCK));
    for (x, y, pixel) in picture.enumerate_pixels() {
        let square = squares.get_pixel_mut(x / BLOCK, y / BLOCK);
        for (s, p) in square.0.iter_mut().zip(pixel.0) {
            *s = (*s).max(p);
        }
    }
    imageops::resize(&squares, width, height, FilterType::Triangle)
}
