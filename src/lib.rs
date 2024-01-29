pub mod config;
pub mod tmdb;

use std::num::NonZeroU32;

use base64::Engine;
use fast_image_resize::{DynamicImageView, DynamicImageViewMut, ImageView, ImageViewMut};
use image::{EncodableLayout, GrayImage};
use serde::{
    de::{Error as _, Unexpected},
    Deserialize, Serialize,
};

type RawHash = [u8; 8];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hash(RawHash);

impl Hash {
    pub fn distance(&self, rhs: &Self) -> u32 {
        self.0
            .into_iter()
            .zip(rhs.0)
            .map(|(x, y)| (x ^ y).count_ones())
            .sum()
    }
}

impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        hash_encode(&self.0).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        let decoded = hash_decode(&string).map_err(|_| {
            D::Error::invalid_value(Unexpected::Str(&string), &"a base64-encoded string")
        })?;
        let raw_hash = RawHash::try_from(decoded.as_slice())
            .map_err(|_| D::Error::invalid_length(decoded.len(), &"8 bytes"))?;

        Ok(Hash(raw_hash))
    }
}

struct Resizer {
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

pub struct MeanHash {
    resizer: Resizer,
    resized_image: GrayImage,
}

impl MeanHash {
    pub fn new() -> Self {
        Self {
            resizer: Resizer::new(),
            resized_image: GrayImage::new(8, 8),
        }
    }

    pub fn hash(&mut self, image: &GrayImage) -> Hash {
        self.resizer.resize(image, &mut self.resized_image);
        let mut raw_hash = RawHash::default();
        collect_bits(mean_hash(&self.resized_image.as_ref()), &mut raw_hash);
        Hash(raw_hash)
    }
}

pub struct GradientHash {
    resizer: Resizer,
    resized_image: GrayImage,
}

impl GradientHash {
    pub fn new() -> Self {
        Self {
            resizer: Resizer::new(),
            resized_image: GrayImage::new(9, 8),
        }
    }

    pub fn hash(&mut self, image: &GrayImage) -> Hash {
        self.resizer.resize(image, &mut self.resized_image);
        let mut raw_hash = RawHash::default();
        collect_bits(gradient_hash(self.resized_image.as_ref(), 9), &mut raw_hash);
        Hash(raw_hash)
    }
}

fn mean_hash(luma: &[u8]) -> impl Iterator<Item = bool> + '_ {
    let mean = luma.iter().map(|&l| l as f32).sum::<f32>() / luma.len() as f32;
    luma.iter().map(move |&l| l as f32 > mean)
}

fn gradient_hash(luma: &[u8], row_stride: usize) -> impl Iterator<Item = bool> + '_ {
    luma.chunks(row_stride)
        .flat_map(|row| row.windows(2).map(|v| v[0] < v[1]))
}

fn collect_bits(bits: impl Iterator<Item = bool>, out: &mut [u8]) {
    out.fill(0);
    for (i, bit) in bits.enumerate() {
        out[i >> 3] |= (bit as u8) << (i & 0x7);
    }
}

fn hash_encode(hash: &[u8]) -> String {
    let engine = base64::engine::general_purpose::STANDARD;
    engine.encode(hash)
}

fn hash_decode(encoded: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let engine = base64::engine::general_purpose::STANDARD;
    engine.decode(encoded)
}
