//! Tray icon data and conversion to platform bitmap formats.

use gpui::SharedString;

/// Raw RGBA icon bitmap (sRGBA, 8 bits per channel, row-major, `width*height*4`
/// bytes). This is the format the tray backends consume; source data can be
/// generated procedurally, loaded from an image, or produced by rasterizing an SVG
/// (see the crate README for the `resvg` integration sketch).
#[derive(Debug, Clone)]
pub struct TrayIconData {
    /// The width of the bitmap, in pixels.
    pub width: u32,
    /// The height of the bitmap, in pixels.
    pub height: u32,
    /// sRGBA bytes: `[r, g, b, a, r, g, b, a, …]`.
    pub rgba: Vec<u8>,
}

impl TrayIconData {
    /// Build icon data from raw sRGBA bytes.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        debug_assert_eq!(rgba.len(), (width as usize) * (height as usize) * 4);
        Self {
            width,
            height,
            rgba,
        }
    }

    /// Convert sRGBA to ARGB32 in network byte order (`[a, r, g, b, …]`), the
    /// layout expected by ksni / the StatusNotifierItem `IconPixmap` field.
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

/// Reference to an icon: either a freedesktop icon name or raw bitmap data.
#[derive(Debug, Clone)]
pub enum TrayIconRef {
    /// A freedesktop-compliant icon name (e.g. `"computer"`). On Linux this is
    /// passed through to SNI as `IconName`; on other platforms it is ignored.
    Name(SharedString),
    /// Raw bitmap data.
    Data(TrayIconData),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_argb32_is_network_byte_order() {
        // One opaque red pixel: sRGBA = [R, G, B, A] = [255, 0, 0, 255].
        let data = TrayIconData::new(1, 1, vec![255, 0, 0, 255]);
        // ARGB32 network byte order: [A, R, G, B] = [255, 255, 0, 0].
        assert_eq!(data.to_argb32(), vec![255, 255, 0, 0]);

        // One fully transparent green pixel: sRGBA = [0, 255, 0, 0].
        let data = TrayIconData::new(1, 1, vec![0, 255, 0, 0]);
        assert_eq!(data.to_argb32(), vec![0, 0, 255, 0]);
    }
}

