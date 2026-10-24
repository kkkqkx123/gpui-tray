//! Tray icon models: multi-scale bitmaps, template marking and named icons.

use gpui::SharedString;

use crate::error::TrayError;

/// A single bitmap image at a given logical size and scale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayIconImage {
    width: u32,
    height: u32,
    scale: u8,
    rgba: Vec<u8>,
}

impl TrayIconImage {
    /// Build an image from raw sRGBA bytes.
    ///
    /// The byte length must be `width * height * 4`; malformed input is
    /// rejected here instead of deep inside a platform backend.
    pub fn new(width: u32, height: u32, scale: u8, rgba: Vec<u8>) -> Result<Self, TrayError> {
        let expected = width as usize * height as usize * 4;
        if rgba.len() != expected {
            return Err(TrayError::InvalidIcon(
                format!(
                    "expected {expected} bytes for {width}x{height}@{scale}x, got {}",
                    rgba.len()
                )
                .into(),
            ));
        }
        Ok(Self {
            width,
            height,
            scale,
            rgba,
        })
    }

    /// Logical width in points (pixels divided by the scale factor).
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Logical height in points (pixels divided by the scale factor).
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The device scale factor this bitmap was drawn for.
    pub fn scale(&self) -> u8 {
        self.scale
    }

    /// The sRGBA bytes, row-major, `width * height * scale * scale * 4` long.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// The pixel width (`width * scale`).
    pub fn pixel_width(&self) -> u32 {
        self.width * self.scale as u32
    }

    /// The pixel height (`height * scale`).
    pub fn pixel_height(&self) -> u32 {
        self.height * self.scale as u32
    }

    /// Convert sRGBA to ARGB32 in network byte order (`[a, r, g, b, …]`), the
    /// layout expected by the StatusNotifierItem `IconPixmap` field.
    pub fn to_argb32(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.rgba.len());
        for px in self.rgba.chunks_exact(4) {
            // px = [r, g, b, a] (sRGBA)
            out.push(px[3]); // A
            out.push(px[0]); // R
            out.push(px[1]); // G
            out.push(px[2]); // B
        }
        out
    }
}

/// A multi-scale bitmap tray icon.
///
/// Provide one image per scale you care about (for example 1x and 2x of the
/// same 16x16 pt glyph); the platform picks the best fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayIconBitmap {
    images: Vec<TrayIconImage>,
    templated: bool,
}

impl TrayIconBitmap {
    /// Start building a bitmap icon.
    pub fn builder() -> TrayIconBitmapBuilder {
        TrayIconBitmapBuilder::default()
    }

    /// The bitmap variants, in the order they were added.
    pub fn images(&self) -> &[TrayIconImage] {
        &self.images
    }

    /// Whether the icon is a template (monochrome) icon. macOS adapts template
    /// icons to the light/dark menu bar; other platforms ignore the flag.
    pub fn is_templated(&self) -> bool {
        self.templated
    }
}

/// Builder for [`TrayIconBitmap`].
#[derive(Default)]
pub struct TrayIconBitmapBuilder {
    images: Vec<TrayIconImage>,
    templated: bool,
}

impl TrayIconBitmapBuilder {
    /// Add one bitmap variant.
    pub fn with_image(mut self, image: TrayIconImage) -> Self {
        self.images.push(image);
        self
    }

    /// Mark the icon as a template (monochrome) icon.
    pub fn with_template(mut self, templated: bool) -> Self {
        self.templated = templated;
        self
    }

    /// Finish building.
    pub fn build(self) -> TrayIconBitmap {
        TrayIconBitmap {
            images: self.images,
            templated: self.templated,
        }
    }
}

/// A named (theme) icon, resolved by the platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayIconName {
    name: SharedString,
}

impl TrayIconName {
    /// Create a named icon reference.
    pub fn new(name: impl Into<SharedString>) -> Self {
        Self { name: name.into() }
    }

    /// The platform icon name.
    pub fn as_str(&self) -> &str {
        &self.name
    }
}

/// A tray icon: either a named theme icon or a bitmap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayIcon {
    /// A named icon. Linux passes the name to the StatusNotifierItem;
    /// macOS resolves it through `NSImage::imageNamed`; Windows does not
    /// support named icons and reports [`TrayError::UnsupportedPlatform`].
    Named(TrayIconName),
    /// A bitmap icon.
    Bitmap(TrayIconBitmap),
}

impl TrayIcon {
    /// Convenience constructor for a single-scale bitmap icon.
    pub fn bitmap(
        width: u32,
        height: u32,
        scale: u8,
        rgba: Vec<u8>,
    ) -> Result<Self, TrayError> {
        Ok(Self::Bitmap(
            TrayIconBitmap::builder()
                .with_image(TrayIconImage::new(width, height, scale, rgba)?)
                .build(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_rejects_bad_length() {
        assert!(TrayIconImage::new(2, 2, 1, vec![0; 15]).is_err());
        assert!(TrayIconImage::new(2, 2, 1, vec![0; 16]).is_ok());
    }

    #[test]
    fn to_argb32_is_network_byte_order() {
        let img = TrayIconImage::new(1, 1, 1, vec![1, 2, 3, 4]).unwrap();
        assert_eq!(img.to_argb32(), vec![4, 1, 2, 3]);
    }
}
