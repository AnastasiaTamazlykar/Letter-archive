//! Draws a Core Image picture into plain pixels we can work with: the page background
//! at A4 size, and the thumbnails.

use std::{error::Error, ptr::NonNull};

use image::{DynamicImage, RgbImage, RgbaImage};
use objc2::rc::Retained;
use objc2_core_foundation::{CGAffineTransform, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGColorSpace;
use objc2_core_image::{CIContext, CIImage, kCIFormatRGBA8};

use super::{PAGE_HEIGHT_MM, PAGE_WIDTH_MM};

/// Resolution of the page background, in dots per inch.
pub const DPI: f32 = 150.0;

// A Core Image context does the drawing. Making one is slow, so Apple advises keeping it.
// `thread_local!` gives every thread its own context, made on first use and reused after:
// the window's thread for thumbnails, the conversion thread for pages. Rust can't check
// that one Apple object is safe to share between threads, so each keeps its own.
thread_local! {
    static CONTEXT: Retained<CIContext> = unsafe { CIContext::new() };
}

/// Draws the flattened page into an RGB picture with exactly the size of A4 at `DPI`.
pub fn render_page(page: &CIImage) -> Result<RgbImage, Box<dyn Error>> {
    let (width, height) = a4_pixels();
    let rgba = render_rgba(page, width, height)?;
    Ok(DynamicImage::ImageRgba8(rgba).to_rgb8()) // drop alpha, it's not needed
}

/// Draws a Core Image picture into an RGBA picture of exactly `width` x `height` pixels,
/// stretching it to fit. Used for the page background and for thumbnails.
pub fn render_rgba(image: &CIImage, width: u32, height: u32) -> Result<RgbaImage, Box<dyn Error>> {
    let scaled = scale_to(image, width, height);

    // Core Image draws into a plain list of bytes: 4 per pixel (red, green, blue, alpha).
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let bounds = CGRect::new(
        CGPoint::new(0.0, 0.0),
        CGSize::new(width as f64, height as f64),
    );
    let color_space = CGColorSpace::new_device_rgb().ok_or("no RGB colour space")?;
    let target = NonNull::new(pixels.as_mut_ptr())
        .ok_or("no pixel memory")?
        .cast();
    // `with` lends this thread's context to the closure.
    CONTEXT.with(|context| unsafe {
        context.render_toBitmap_rowBytes_bounds_format_colorSpace(
            &scaled,
            target,
            (width * 4) as isize,
            bounds,
            kCIFormatRGBA8,
            Some(&color_space),
        );
    });

    Ok(RgbaImage::from_raw(width, height, pixels).ok_or("wrong number of pixels")?)
}

/// The size of an A4 page in pixels at `DPI`.
fn a4_pixels() -> (u32, u32) {
    let to_pixels = |mm: f32| (mm / 25.4 * DPI).round() as u32;
    (to_pixels(PAGE_WIDTH_MM), to_pixels(PAGE_HEIGHT_MM))
}

/// Stretches the picture to exactly `width` x `height` pixels, starting at point (0, 0).
fn scale_to(image: &CIImage, width: u32, height: u32) -> Retained<CIImage> {
    let extent = unsafe { image.extent() };
    let scale_x = width as f64 / extent.size.width;
    let scale_y = height as f64 / extent.size.height;
    // A transform: scale by (a, d), then move by (tx, ty) so the picture starts at (0, 0).
    let transform = CGAffineTransform {
        a: scale_x,
        b: 0.0,
        c: 0.0,
        d: scale_y,
        tx: -extent.origin.x * scale_x,
        ty: -extent.origin.y * scale_y,
    };
    unsafe { image.imageByApplyingTransform(transform) }
}

// Small file, so its one test stays here.
#[cfg(test)]
mod tests {
    use image::Rgb;

    use super::*;
    // A private function of `pipeline`: allowed, because this module lives inside it.
    use crate::pipeline::open_image;

    /// Core Image counts rows from the bottom, our pixels count from the top. A picture
    /// that is red on top and blue below must come out the same way round, or every
    /// page would end up upside down.
    #[test]
    fn test_render_rgba_keeps_top_at_top() {
        let file = std::env::temp_dir().join("paper_archive_test_red_over_blue.png");
        let red_over_blue = RgbImage::from_fn(20, 40, |_, y| {
            if y < 20 {
                Rgb([255, 0, 0])
            } else {
                Rgb([0, 0, 255])
            }
        });
        red_over_blue.save(&file).unwrap();

        let rendered = render_rgba(&open_image(&file).unwrap(), 20, 40).unwrap();

        let [red, _, blue, _] = rendered.get_pixel(10, 5).0;
        assert!(
            red > 200 && blue < 50,
            "top should be red, got {:?}",
            rendered.get_pixel(10, 5)
        );
        let [red, _, blue, _] = rendered.get_pixel(10, 35).0;
        assert!(
            blue > 200 && red < 50,
            "bottom should be blue, got {:?}",
            rendered.get_pixel(10, 35)
        );
    }
}
