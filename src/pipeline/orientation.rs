//! Reads the orientation tag that phones store in photos.

use objc2::rc::Retained;
use objc2_core_image::CIImage;
use objc2_foundation::{NSNumber, NSString};

/// Turns the picture upright according to its orientation tag.
/// Without a tag, the picture is returned unchanged.
pub fn make_upright(image: Retained<CIImage>) -> Retained<CIImage> {
    match read_tag(&image) {
        Some(tag) => unsafe { image.imageByApplyingOrientation(tag as i32) },
        None => image,
    }
}

/// Reads the orientation tag from the picture's metadata.
fn read_tag(image: &CIImage) -> Option<i64> {
    // The photo's metadata, as a dictionary of names and values.
    let properties = unsafe { image.properties() };
    let value = properties.objectForKey(&NSString::from_str("Orientation"))?;

    // The value is stored as a generic object; check that it is a number and read it.
    let number = value.downcast::<NSNumber>().ok()?;
    Some(number.integerValue() as i64)
}
