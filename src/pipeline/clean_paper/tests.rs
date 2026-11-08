//! Tests for cleaning the page picture. The pictures are built by hand: a few colours
//! on a small canvas, so each test shows exactly what happens to which pixels.

// `super::*` brings in everything `clean_paper.rs` can see, private items included.
use super::*;

const WHITE: [u8; 3] = [255, 255, 255];
const BLACK: [u8; 3] = [0, 0, 0];

// ── Helpers ──

/// A picture of one colour.
fn filled(width: u32, height: u32, colour: [u8; 3]) -> RgbImage {
    RgbImage::from_pixel(width, height, Rgb(colour))
}

/// Paints a rectangle of the picture in one colour (columns `left..right`, rows `top..bottom`).
fn paint(picture: &mut RgbImage, area: Area, colour: [u8; 3]) {
    for y in area.top..area.bottom {
        for x in area.left..area.right {
            picture.put_pixel(x, y, Rgb(colour));
        }
    }
}

/// A line as Vision gives it: fractions of the page, measured from the bottom-left corner.
fn line(x: f32, y: f32, width: f32, height: f32) -> Line {
    Line {
        text: String::new(),
        x,
        y,
        width,
        height,
        colour: BLACK,
    }
}

// ── Whitening the paper ──

/// Grey paper with a shadow across it (darker on the left, brighter on the right)
/// becomes pure white everywhere.
#[test]
fn test_whiten_paper_removes_shadows() {
    let mut picture = RgbImage::from_fn(256, 256, |x, _| {
        let grey = (170 + x * 60 / 255) as u8; // 170 on the left .. 230 on the right
        Rgb([grey, grey, grey])
    });

    whiten_paper(&mut picture);

    assert!(picture.pixels().all(|pixel| pixel.0 == WHITE));
}

/// The paper turns white, while a black mark stays dark and a red stamp stays red.
#[test]
fn test_whiten_paper_keeps_ink_and_colour() {
    let mut picture = filled(256, 256, [200, 200, 200]);
    paint(
        &mut picture,
        Area {
            left: 100,
            top: 100,
            right: 120,
            bottom: 120,
        },
        [30, 30, 30],
    );
    paint(
        &mut picture,
        Area {
            left: 30,
            top: 30,
            right: 50,
            bottom: 50,
        },
        [200, 40, 40],
    );

    whiten_paper(&mut picture);

    assert_eq!(picture.get_pixel(5, 5).0, WHITE, "paper");
    let [r, g, b] = picture.get_pixel(110, 110).0;
    assert!(
        r < 80 && g < 80 && b < 80,
        "black mark should stay dark, got {:?}",
        [r, g, b]
    );
    let [r, g, b] = picture.get_pixel(40, 40).0;
    assert!(
        r > 200 && g < 100 && b < 100,
        "stamp should stay red, got {:?}",
        [r, g, b]
    );
}

/// The rule for ink: dark, or clearly coloured, even when pale.
#[test]
fn test_is_ink_dark_or_colourful() {
    assert!(is_ink([80, 80, 80]), "dark grey");
    assert!(!is_ink([200, 200, 200]), "light grey is paper");
    assert!(is_ink([255, 180, 180]), "pale red is colourful");
}

// ── Where a line lies in the picture ──
// The picture is 100 × 160 pixels. The lines are 0.125 of the page high, so 20 pixels,
// and the margin around them is a quarter of that: 5 pixels.

/// Vision measures from the bottom of the page, picture rows count from the top:
/// a line at the very bottom (`y = 0`) covers the last rows of the picture.
#[test]
fn test_line_area_flips_to_rows_from_the_top() {
    let picture = filled(100, 160, WHITE);

    let area = line_area(&picture, &line(0.25, 0.0, 0.5, 0.125));

    // Rows 140..160 plus the margin above; below, the picture ends at row 160.
    assert_eq!(
        area,
        Area {
            left: 20,
            top: 135,
            right: 80,
            bottom: 160
        }
    );
}

/// A line touching the edges of the page: its margin is cut off at the picture's edge.
#[test]
fn test_line_area_stays_inside_the_picture() {
    let picture = filled(100, 160, WHITE);

    let area = line_area(&picture, &line(0.0, 0.875, 1.0, 0.125));

    assert_eq!(
        area,
        Area {
            left: 0,
            top: 0,
            right: 100,
            bottom: 25
        }
    );
}

// ── Text colours and erasing ──

/// A line's colour comes from the darkest pixels in its box: blue text gives blue,
/// even though most of the box is white paper.
#[test]
fn test_set_text_colours_finds_the_ink_colour() {
    let blue = [20, 40, 200];
    let mut picture = filled(100, 160, WHITE);
    paint(
        &mut picture,
        Area {
            left: 30,
            top: 65,
            right: 70,
            bottom: 75,
        },
        blue,
    );
    let mut lines = [line(0.25, 0.5, 0.5, 0.125)]; // covers rows 55..85, columns 20..80

    set_text_colours(&picture, &mut lines);

    assert_eq!(lines[0].colour, blue);
}

/// Erasing paints a body line's box white, but leaves the letterhead (logos) and
/// everything outside the boxes as it was.
#[test]
fn test_erase_text_skips_the_letterhead() {
    let mut picture = filled(100, 160, BLACK);
    let body = line(0.25, 0.5, 0.5, 0.125);
    let letterhead = line(0.25, 0.875, 0.5, 0.1);

    erase_text(&mut picture, &[body, letterhead]);

    assert_eq!(picture.get_pixel(50, 70).0, WHITE, "inside the body line");
    assert_eq!(
        picture.get_pixel(50, 10).0,
        BLACK,
        "inside the letterhead line"
    );
    assert_eq!(picture.get_pixel(5, 120).0, BLACK, "outside every line");
}
