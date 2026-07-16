//! Native CrowQuant-compatible vector compression and compressed retrieval.
//!
//! This is a focused Rust port of CrowQuant's verified 4-bit pipeline:
//! randomized Walsh-Hadamard rotation, Gaussian Lloyd-Max scalar
//! quantization, MSB-first bit packing, and cosine search directly over
//! quantized values. The block serialization matches CrowQuant's
//! `<ddBIIII` little-endian format.

const BITS: u8 = 4;
const SEED: u32 = 42;
pub const TEXT_VECTOR_DIMENSION: usize = 256;
pub const ALGORITHM: &str = "CrowQuant WHT + Lloyd-Max 4-bit (native)";

const CENTROIDS: [f64; 16] = [
    -2.7326, -2.0690, -1.6180, -1.2562, -0.9424, -0.6568, -0.3881, -0.1284, 0.1284, 0.3881, 0.6568,
    0.9424, 1.2562, 1.6180, 2.0690, 2.7326,
];

// NumPy PCG64/default_rng(42).choice([-1, 1], size=256), matching the
// CrowQuant reference implementation for the fixed local text-vector size.
const SIGN_BITS: &str = "0110010100111111101010011101100001101101011001011110000010111101001000100010001110011100110110100110101011110101101100000110110111110110100010001100101010010111110001000000111010111001000000111010000101010011010110100001001010111001010110010101101010000010";

#[derive(Clone, Debug, PartialEq)]
pub struct CrowQuantBlock {
    pub scale: f64,
    pub zero: f64,
    pub bits: u8,
    pub dimension: u32,
    pub seed: u32,
    pub padded_dimension: u32,
    pub packed_data: Vec<u8>,
}

pub fn vectorize_text(text: &str) -> Result<Vec<f64>, String> {
    let normalized = text.to_lowercase();
    let tokens = normalized
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return Err("Memory text must contain at least one letter or number".into());
    }

    let mut vector = vec![0.0; TEXT_VECTOR_DIMENSION];
    for token in &tokens {
        add_feature(&mut vector, "word", token, 3.0);
        let characters = token.chars().collect::<Vec<_>>();
        if characters.len() < 3 {
            add_feature(&mut vector, "short", token, 1.0);
        } else {
            for trigram in characters.windows(3) {
                let feature = trigram.iter().collect::<String>();
                add_feature(&mut vector, "tri", &feature, 0.75);
            }
        }
    }
    for pair in tokens.windows(2) {
        add_feature(
            &mut vector,
            "pair",
            &format!("{} {}", pair[0], pair[1]),
            1.5,
        );
    }

    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    if !norm.is_finite() || norm < 1e-12 {
        return Err("Memory text did not produce a usable local vector".into());
    }
    for value in &mut vector {
        *value /= norm;
    }
    Ok(vector)
}

fn add_feature(vector: &mut [f64], namespace: &str, feature: &str, weight: f64) {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in namespace.bytes().chain([0xff]).chain(feature.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let index = (hash as usize) % vector.len();
    let sign = if (hash >> 63) == 0 { 1.0 } else { -1.0 };
    vector[index] += sign * weight;
}

pub fn quantize(vector: &[f64]) -> Result<CrowQuantBlock, String> {
    if vector.is_empty() || vector.len() > TEXT_VECTOR_DIMENSION {
        return Err(format!(
            "CrowQuant vector dimension must be 1-{TEXT_VECTOR_DIMENSION}"
        ));
    }
    if vector.iter().any(|value| !value.is_finite()) {
        return Err("CrowQuant vector contains a non-finite value".into());
    }
    let padded_dimension = vector.len().next_power_of_two();
    let mut rotated = vec![0.0; padded_dimension];
    rotated[..vector.len()].copy_from_slice(vector);
    for (index, value) in rotated.iter_mut().enumerate() {
        *value *= if SIGN_BITS.as_bytes()[index] == b'1' {
            1.0
        } else {
            -1.0
        };
    }
    wht(&mut rotated);

    let zero = rotated.iter().sum::<f64>() / rotated.len() as f64;
    let mut scale = (rotated
        .iter()
        .map(|value| (value - zero).powi(2))
        .sum::<f64>()
        / rotated.len() as f64)
        .sqrt();
    if scale < 1e-12 {
        scale = 1.0;
    }
    let indices = rotated
        .iter()
        .map(|value| centroid_index((value - zero) / scale))
        .collect::<Vec<_>>();

    Ok(CrowQuantBlock {
        scale,
        zero,
        bits: BITS,
        dimension: vector.len() as u32,
        seed: SEED,
        padded_dimension: padded_dimension as u32,
        packed_data: pack_nibbles(&indices),
    })
}

fn wht(values: &mut [f64]) {
    let mut width = 1;
    while width < values.len() {
        for start in (0..values.len()).step_by(width * 2) {
            for offset in 0..width {
                let a = values[start + offset];
                let b = values[start + offset + width];
                values[start + offset] = a + b;
                values[start + offset + width] = a - b;
            }
        }
        width *= 2;
    }
    let normalization = (values.len() as f64).sqrt();
    for value in values {
        *value /= normalization;
    }
}

fn centroid_index(value: f64) -> u8 {
    for (index, pair) in CENTROIDS.windows(2).enumerate() {
        if value < (pair[0] + pair[1]) / 2.0 {
            return index as u8;
        }
    }
    15
}

fn pack_nibbles(indices: &[u8]) -> Vec<u8> {
    indices
        .chunks(2)
        .map(|pair| (pair[0] << 4) | pair.get(1).copied().unwrap_or(0))
        .collect()
}

fn unpack_nibbles(data: &[u8], count: usize) -> Result<Vec<u8>, String> {
    if data.len() * 2 < count {
        return Err("CrowQuant block is truncated".into());
    }
    let mut output = Vec::with_capacity(count);
    for byte in data {
        output.push(byte >> 4);
        if output.len() < count {
            output.push(byte & 0x0f);
        }
    }
    Ok(output)
}

pub fn serialize(block: &CrowQuantBlock) -> Vec<u8> {
    let mut output = Vec::with_capacity(33 + block.packed_data.len());
    output.extend_from_slice(&block.scale.to_le_bytes());
    output.extend_from_slice(&block.zero.to_le_bytes());
    output.push(block.bits);
    output.extend_from_slice(&block.dimension.to_le_bytes());
    output.extend_from_slice(&block.seed.to_le_bytes());
    output.extend_from_slice(&block.padded_dimension.to_le_bytes());
    output.extend_from_slice(&(block.packed_data.len() as u32).to_le_bytes());
    output.extend_from_slice(&block.packed_data);
    output
}

pub fn deserialize(data: &[u8]) -> Result<CrowQuantBlock, String> {
    const HEADER: usize = 33;
    if data.len() < HEADER {
        return Err("CrowQuant block header is truncated".into());
    }
    let scale = f64::from_le_bytes(data[0..8].try_into().unwrap());
    let zero = f64::from_le_bytes(data[8..16].try_into().unwrap());
    let bits = data[16];
    let dimension = u32::from_le_bytes(data[17..21].try_into().unwrap());
    let seed = u32::from_le_bytes(data[21..25].try_into().unwrap());
    let padded_dimension = u32::from_le_bytes(data[25..29].try_into().unwrap());
    let data_length = u32::from_le_bytes(data[29..33].try_into().unwrap()) as usize;
    if bits != BITS || seed != SEED || dimension == 0 || padded_dimension == 0 {
        return Err("CrowQuant block settings are unsupported".into());
    }
    if !padded_dimension.is_power_of_two() || padded_dimension < dimension {
        return Err("CrowQuant block dimensions are invalid".into());
    }
    if !scale.is_finite() || !zero.is_finite() || data.len() != HEADER + data_length {
        return Err("CrowQuant block payload is invalid".into());
    }
    if data_length * 2 < padded_dimension as usize {
        return Err("CrowQuant block payload is truncated".into());
    }
    Ok(CrowQuantBlock {
        scale,
        zero,
        bits,
        dimension,
        seed,
        padded_dimension,
        packed_data: data[HEADER..].to_vec(),
    })
}

pub fn compressed_cosine(left: &CrowQuantBlock, right: &CrowQuantBlock) -> Result<f64, String> {
    if left.bits != right.bits
        || left.seed != right.seed
        || left.dimension != right.dimension
        || left.padded_dimension != right.padded_dimension
    {
        return Err("CrowQuant blocks use incompatible settings".into());
    }
    let count = left.padded_dimension as usize;
    let left_indices = unpack_nibbles(&left.packed_data, count)?;
    let right_indices = unpack_nibbles(&right.packed_data, count)?;
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for (left_index, right_index) in left_indices.iter().zip(right_indices.iter()) {
        let left_value = CENTROIDS[*left_index as usize] * left.scale + left.zero;
        let right_value = CENTROIDS[*right_index as usize] * right.scale + right.zero;
        dot += left_value * right_value;
        left_norm += left_value * left_value;
        right_norm += right_value * right_value;
    }
    if left_norm < 1e-24 || right_norm < 1e-24 {
        return Ok(0.0);
    }
    Ok((dot / (left_norm.sqrt() * right_norm.sqrt())).clamp(-1.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_reference_crowquant_golden_block() {
        let block = quantize(&[1., 2., 3., 4., 5., 6., 7., 8.]).unwrap();
        assert!((block.scale - 5.0373604199024715).abs() < 1e-12);
        assert!((block.zero - -0.3535533905932742).abs() < 1e-12);
        assert_eq!(block.packed_data, [0x82, 0x86, 0x7e, 0x84]);
        let serialized = serialize(&block);
        assert_eq!(serialized.len(), 37);
        assert_eq!(
            &serialized[16..],
            &[4, 8, 0, 0, 0, 42, 0, 0, 0, 8, 0, 0, 0, 4, 0, 0, 0, 0x82, 0x86, 0x7e, 0x84]
        );
        assert_eq!(deserialize(&serialized).unwrap(), block);
    }

    #[test]
    fn lexical_vectors_compress_and_rank_the_matching_memory_first() {
        let query = quantize(&vectorize_text("quantum lab notes").unwrap()).unwrap();
        let matching =
            quantize(&vectorize_text("notes from the quantum lab experiment").unwrap()).unwrap();
        let unrelated =
            quantize(&vectorize_text("grocery list apples bread milk").unwrap()).unwrap();
        assert!(
            compressed_cosine(&query, &matching).unwrap()
                > compressed_cosine(&query, &unrelated).unwrap()
        );
        assert_eq!(deserialize(&serialize(&matching)).unwrap(), matching);
    }

    #[test]
    fn corrupt_blocks_are_rejected() {
        let block = serialize(&quantize(&vectorize_text("persistent memory").unwrap()).unwrap());
        assert!(deserialize(&block[..block.len() - 1]).is_err());
        assert!(vectorize_text("---").is_err());
    }
}
