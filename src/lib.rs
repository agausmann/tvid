pub mod config;
pub mod tmdb;

use std::num::NonZeroU32;

use base64::Engine;
use fast_image_resize::{DynamicImageView, DynamicImageViewMut, ImageView, ImageViewMut};
use image::{EncodableLayout, GrayImage};

pub struct Resizer {
    inner: fast_image_resize::Resizer,
}

impl Resizer {
    pub fn new() -> Self {
        Resizer {
            inner: fast_image_resize::Resizer::new(fast_image_resize::ResizeAlg::Convolution(
                fast_image_resize::FilterType::Bilinear,
            )),
        }
    }

    pub fn resize(&mut self, inp: &GrayImage, out: &mut GrayImage) {
        let hash_width = 8;
        let hash_height = 8;
        if out.width() != hash_width || out.height() != hash_height {
            // Reallocate
            *out = GrayImage::new(hash_width, hash_height);
        }
        self.inner
            .resize(
                &DynamicImageView::U8(
                    ImageView::from_buffer(
                        NonZeroU32::new(inp.width()).unwrap(),
                        NonZeroU32::new(out.height()).unwrap(),
                        inp.as_bytes(),
                    )
                    .unwrap(),
                ),
                &mut DynamicImageViewMut::U8(
                    ImageViewMut::from_buffer(
                        NonZeroU32::new(out.width()).unwrap(),
                        NonZeroU32::new(out.height()).unwrap(),
                        out.as_mut(),
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
    }
}

pub fn mean_hash(luma: &[u8]) -> impl Iterator<Item = bool> + '_ {
    let mean = luma.iter().map(|&l| l as f32).sum::<f32>() / luma.len() as f32;
    luma.iter().map(move |&l| l as f32 > mean)
}

pub fn gradient_hash(luma: &[u8], row_stride: usize) -> impl Iterator<Item = bool> + '_ {
    luma.chunks(row_stride)
        .flat_map(|row| row.windows(2).map(|v| v[0] < v[1]))
}

pub fn collect_bits(bits: impl Iterator<Item = bool>, out: &mut [u8]) {
    out.fill(0);
    for (i, bit) in bits.enumerate() {
        out[i >> 3] |= (bit as u8) << (i & 0x7);
    }
}

pub fn hash_distance(a: &[u8], b: &[u8]) -> u32 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(&x, &y)| (x ^ y).count_ones()).sum()
}

pub fn hash_encode(hash: &[u8]) -> String {
    let engine = base64::engine::general_purpose::STANDARD;
    engine.encode(hash)
}

pub fn hash_decode(encoded: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let engine = base64::engine::general_purpose::STANDARD;
    engine.decode(encoded)
}
