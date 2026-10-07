use sha2::{Digest, Sha256};

use super::{CHUNK_MAX_BYTES, CHUNK_OVERLAP_BYTES, SOURCE_MAX_BYTES};
use crate::{crowquant, storage::NativeMemoryChunk};

pub(super) fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

pub(super) fn chunks(source_id: &str, text: &str) -> Result<Vec<NativeMemoryChunk>, String> {
    if text.len() > SOURCE_MAX_BYTES {
        return Err(format!(
            "Memory source exceeds {SOURCE_MAX_BYTES} UTF-8 bytes"
        ));
    }
    if text.trim().is_empty() {
        return Err("Memory source cannot be empty".into());
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + CHUNK_MAX_BYTES).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        if end < text.len() {
            // Prefer a paragraph boundary in the last half of a full chunk.
            if let Some(offset) = text[start..end].rfind("\n\n") {
                if offset > CHUNK_MAX_BYTES / 2 {
                    end = start + offset + 2;
                }
            }
        }
        let part = &text[start..end];
        let ordinal = chunks.len();
        let content_hash = hash(part);
        let lexical_block = crowquant::vectorize_text(part)
            .and_then(|v| crowquant::quantize(&v))
            .ok()
            .map(|b| crowquant::serialize(&b));
        chunks.push(NativeMemoryChunk {
            id: hash(&format!(
                "crowclaw.chunk.v1\0{source_id}\0{ordinal}\0{content_hash}"
            )),
            source_id: source_id.into(),
            ordinal,
            text: part.into(),
            start_byte: start,
            end_byte: end,
            content_hash,
            lexical_block,
        });
        if end == text.len() {
            break;
        }
        start = end.saturating_sub(CHUNK_OVERLAP_BYTES).max(start + 1);
        while !text.is_char_boundary(start) {
            start += 1;
        }
    }
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_offsets_are_exact_and_rebuild_is_deterministic() {
        let text = "🌒 telescope\n".repeat(500);
        let result = chunks("source", &text).unwrap();
        assert_eq!(result, chunks("source", &text).unwrap());
        assert_eq!(result[0].start_byte, 0);
        assert_eq!(result.last().unwrap().end_byte, text.len());
        for c in &result {
            assert_eq!(&text[c.start_byte..c.end_byte], c.text);
            assert!(c.text.len() <= CHUNK_MAX_BYTES);
        }
        for c in result.windows(2) {
            assert!(c[1].start_byte < c[0].end_byte);
        }
    }
}
