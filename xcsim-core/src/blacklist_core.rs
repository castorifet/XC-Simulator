
use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/blacklist_gen.rs"));

const PHASH_MAX_DISTANCE: u32 = 28;

pub const BLACKLIST_ERR: &str = "respack-blacklisted::copyright";

#[inline]
pub fn enabled() -> bool {
    !BLACKLIST_SIGS.is_empty() || !BLACKLIST_PHASHES.is_empty()
}

fn sig(tag: &[u8], data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(tag);
    h.update([0u8]);
    h.update(data);
    h.finalize().into()
}

fn listed(s: &[u8; 32]) -> bool {
    BLACKLIST_SIGS.binary_search(s).is_ok()
}

fn phash(img: &image::DynamicImage) -> [u8; 32] {
    const W: u32 = 16;
    const H: u32 = 16;
    let small = img.resize_exact(W + 1, H, image::imageops::FilterType::Triangle).to_luma8();
    let mut bits = [0u8; 32];
    let mut idx = 0usize;
    for y in 0..H {
        for x in 0..W {
            if small.get_pixel(x, y)[0] < small.get_pixel(x + 1, y)[0] {
                bits[idx / 8] |= 1 << (idx % 8);
            }
            idx += 1;
        }
    }
    bits
}

fn hamming(a: &[u8; 32], b: &[u8; 32]) -> u32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x ^ y).count_ones()).sum()
}

fn phash_listed(h: &[u8; 32]) -> bool {
    BLACKLIST_PHASHES.iter().any(|b| hamming(h, b) <= PHASH_MAX_DISTANCE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbaImage};

    fn sample(w: u32, h: u32) -> DynamicImage {
        let mut img = RgbaImage::new(w, h);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let r = ((x * 255) / w.max(1)) as u8;
            let g = ((y * 255) / h.max(1)) as u8;
            let b = (((x + y) * 255) / (w + h).max(1)) as u8;
            *px = image::Rgba([r, g, b, 255]);
        }
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn phash_survives_resize() {
        let original = sample(512, 384);
        let h0 = phash(&original);

        for (w, h) in [(256, 192), (137, 201), (1024, 700), (300, 300)] {
            let resized = original.resize_exact(w, h, image::imageops::FilterType::Lanczos3);
            let d = hamming(&h0, &phash(&resized));
            assert!(d <= PHASH_MAX_DISTANCE, "resized {w}x{h} differs by {d} bits (> {PHASH_MAX_DISTANCE})");
        }
    }

    #[test]
    fn phash_distinguishes_different_images() {
        let a = phash(&sample(400, 400));

        let mut img = RgbaImage::new(400, 400);
        for (x, _y, px) in img.enumerate_pixels_mut() {
            let v = if (x / 8) % 2 == 0 { 0 } else { 255 };
            *px = image::Rgba([v, v, v, 255]);
        }
        let b = phash(&DynamicImage::ImageRgba8(img));
        assert!(hamming(&a, &b) > PHASH_MAX_DISTANCE, "unrelated images should be far apart");
    }
}

pub fn is_blacklisted(files: &[(&str, &[u8])]) -> bool {
    if !enabled() {
        return false;
    }

    for (fname, bytes) in files {
        if listed(&sig(b"raw", bytes)) {
            return true;
        }
        if fname.ends_with(".png") {
            if let Ok(img) = image::load_from_memory(bytes) {
                let rgba = img.to_rgba8();
                let mut buf = Vec::with_capacity(rgba.as_raw().len() + 8);
                buf.extend_from_slice(&rgba.width().to_le_bytes());
                buf.extend_from_slice(&rgba.height().to_le_bytes());
                buf.extend_from_slice(rgba.as_raw());
                if listed(&sig(b"pix", &buf)) {
                    return true;
                }
                if phash_listed(&phash(&img)) {
                    return true;
                }
            }
        }
    }
    false
}
