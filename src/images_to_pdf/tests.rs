//! Tests for the photo side of the pipeline, in its order: load, flatten, recognize,
//! build a page, and the whole run. Writing the PDF is tested in `pdf/tests.rs`.
//!
//! Tests that run Apple Vision take seconds, so only `test_make_page_gives_clean_a4_page`
//! runs every time; the others are `#[ignore]`d. Run them with `cargo test -- --ignored`.

// `super::*` brings in everything `pipeline.rs` can see, private items included.
use super::*;



/// Width and height of a Core Image picture, in pixels.
fn size_of(image: &CIImage) -> (f64, f64) {
    let size = unsafe { image.extent() }.size;
    (size.width, size.height)
}

/// A file in the system's temp folder, so `data/` stays untouched.
fn temp_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("paper_archive_test_{name}"))
}


#[test]
fn test_fit_within_scales_down_only() {
    assert_eq!(fit_within(4000.0, 3000.0, 200), (200, 150));
    assert_eq!(fit_within(100.0, 50.0, 200), (100, 50));
}


/// With no sheet of paper in sight (a plain grey picture), the picture comes back as it was.
#[test]
#[ignore = "runs Vision; run with --ignored"]
fn test_flatten_page_without_paper_keeps_picture() {
    let file = temp_file("grey.png");
    RgbImage::from_pixel(600, 800, image::Rgb([128, 128, 128]))
        .save(&file)
        .unwrap();
    let grey = open_image(&file).unwrap();

    assert_eq!(size_of(&flatten_page(grey).unwrap()), (600.0, 800.0));
}

// ── Step 2: recognizing text ──

/// Our three-letter codes become the names Vision expects. An unknown code gives
/// no names, and then Vision guesses the language itself.
#[test]
fn test_vision_languages_maps_codes() {
    let names = |lang: &str| -> Vec<String> {
        vision_languages(lang)
            .iter()
            .map(|name| name.to_string())
            .collect()
    };

    assert_eq!(names("deu"), ["de-DE"]);
    assert_eq!(names("deu+eng"), ["de-DE", "en-US"]);
    assert!(names("xyz").is_empty());
}


/// Only lines at the very top of the page count as letterhead.
#[test]
fn test_is_letterhead_only_at_top() {
    let line_at = |y| Line {
        text: String::new(),
        x: 0.1,
        y,
        width: 0.5,
        height: 0.02,
        colour: [0, 0, 0],
    };

    assert!(is_letterhead(&line_at(0.95)));
    assert!(!is_letterhead(&line_at(0.5)));
}

// ── One page ──


#[test]
fn test_images_to_pdf_needs_an_image() {
    assert!(images_to_pdf(&[], Path::new("unused.pdf"), "deu").is_err());
}

/// When a photo can't be read, the error says which one.
#[test]
fn test_images_to_pdf_names_the_failing_image() {
    let missing = PathBuf::from("/no/such/photo.HEIC");

    let error = images_to_pdf(&[missing], &temp_file("never.pdf"), "deu").unwrap_err();

    assert!(
        error.to_string().contains("/no/such/photo.HEIC"),
        "got: {error}"
    );
}
