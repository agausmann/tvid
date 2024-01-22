use base64::Engine;

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
