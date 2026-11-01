//! RAG v1 helpers (3.2c): deterministic text chunking and cosine top-k search.
//! Vectors live as JSON blobs in SQLite — brute-force scan, no sqlite-vec,
//! which keeps `scripts/build-multiarch.sh` cross-compiles dependency-free.

/// Split `text` into chunks of at most `max_chars`, preferring paragraph
/// (blank-line) boundaries and falling back to hard slices for oversized
/// paragraphs. Empty fragments are dropped.
pub fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        let mut pieces: Vec<&str> = Vec::new();
        let mut rest = para;
        while rest.chars().count() > max_chars {
            // Cut at the last whitespace before the limit when reasonable.
            let mut cut = 0;
            let mut last_ws = None;
            for (i, ch) in rest.char_indices() {
                if ch.is_whitespace() && i <= max_chars {
                    last_ws = Some(i);
                }
                cut = i;
                if i >= max_chars {
                    break;
                }
            }
            let end = last_ws.unwrap_or(cut + 1).max(1);
            pieces.push(rest[..end].trim());
            rest = rest[end..].trim_start();
        }
        if !rest.is_empty() {
            pieces.push(rest);
        }
        for p in pieces {
            let extra = if cur.is_empty() { 0 } else { 2 };
            if !cur.is_empty() && cur.chars().count() + extra + p.chars().count() > max_chars {
                chunks.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push_str("\n\n");
            }
            cur.push_str(p);
        }
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

/// Cosine similarity of two equal-length vectors; 0.0 on zero norms or
/// length mismatch.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_at_paragraph_boundaries() {
        let text = "alpha beta\n\ngamma delta\n\nepsilon";
        let c = chunk_text(text, 40);
        assert_eq!(c, vec!["alpha beta\n\ngamma delta\n\nepsilon"]);
    }

    #[test]
    fn splits_large_paragraphs_without_losing_words() {
        let text = "word ".repeat(200);
        let c = chunk_text(&text, 100);
        assert!(c.len() > 1);
        assert!(c.iter().all(|x| x.chars().count() <= 100));
        let joined: String = c.join(" ");
        assert!(joined.starts_with("word") && joined.ends_with("word"));
        assert!(!joined.contains("wor d"));
    }

    #[test]
    fn cosine_orders_related_vectors() {
        let q = [1.0, 1.0, 0.0];
        let related = [1.0, 1.0, 0.0];
        let unrelated = [0.0, 0.0, 1.0];
        assert!((cosine(&q, &related) - 1.0).abs() < 1e-6);
        assert!(cosine(&q, &unrelated) < 1e-6);
        assert_eq!(cosine(&[], &[]), 0.0);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
    }
}
