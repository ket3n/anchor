//! Language-agnostic text utilities: identifier-aware tokens, SimHash, format detection.

use std::path::Path;

/// Split identifiers so `verifyToken`, `verify_token`, `a/b/verify-token.rs` all yield
/// `verify token`. Keeps the original token too. Works on any Unicode script.
pub fn identifier_expand(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for raw in s.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        if raw.is_empty() {
            continue;
        }
        out.push_str(raw);
        out.push(' ');
        let parts = split_ident(raw);
        if parts.len() > 1 {
            for p in parts {
                out.push_str(&p);
                out.push(' ');
            }
        }
    }
    out
}

pub fn split_ident(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for chunk in s.split('_') {
        let mut cur = String::new();
        let chars: Vec<char> = chunk.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            let boundary = i > 0
                && c.is_uppercase()
                && (chars[i - 1].is_lowercase()
                    || (i + 1 < chars.len() && chars[i + 1].is_lowercase() && chars[i - 1].is_uppercase()));
            if boundary && !cur.is_empty() {
                parts.push(cur.to_lowercase());
                cur = String::new();
            }
            cur.push(c);
        }
        if !cur.is_empty() {
            parts.push(cur.to_lowercase());
        }
    }
    parts.retain(|p| !p.is_empty());
    parts
}

/// Query terms: identifier-aware, lowercase, deduped, stop-words removed (tiny list,
/// only used to cut noise; recall never depends on it).
pub fn query_terms(s: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "a", "an", "and", "or", "to", "of", "in", "on", "for", "is", "it", "this", "that",
        "with", "be", "as", "at", "by", "we", "i", "you", "can", "do", "add", "make", "please",
        "code", "from", "into", "use", "using", "should", "would", "could", "will", "are", "was",
    ];
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for t in identifier_expand(s).split_whitespace() {
        let t = t.to_lowercase();
        if t.chars().count() < 2 || STOP.contains(&t.as_str()) {
            continue;
        }
        if seen.insert(t.clone()) {
            out.push(t);
        }
    }
    out
}

/// Raw identifier-like tokens (case preserved) for exact symbol seeding.
pub fn raw_identifiers(s: &str) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    s.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':' || c == '.' || c == '/'))
        .flat_map(|t| {
            let t = t.trim_matches(|c: char| c == '.' || c == ':' || c == '/');
            let mut v = vec![t.to_string()];
            // last segment of a path / qualified name
            if let Some(last) = t.rsplit([':', '.', '/']).next() {
                v.push(last.to_string());
            }
            v
        })
        .filter(|t| t.chars().count() >= 3)
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

/// 64-bit SimHash over word shingles — near-duplicate detection without a model.
pub fn simhash(s: &str) -> u64 {
    let toks: Vec<String> = query_terms(s);
    if toks.is_empty() {
        return 0;
    }
    let mut v = [0i32; 64];
    let mut feats: Vec<String> = toks.clone();
    for w in toks.windows(2) {
        feats.push(format!("{} {}", w[0], w[1]));
    }
    for f in feats {
        let h = blake3::hash(f.as_bytes());
        let b = h.as_bytes();
        let x = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        for (i, slot) in v.iter_mut().enumerate() {
            if (x >> i) & 1 == 1 {
                *slot += 1;
            } else {
                *slot -= 1;
            }
        }
    }
    let mut out = 0u64;
    for (i, slot) in v.iter().enumerate() {
        if *slot > 0 {
            out |= 1 << i;
        }
    }
    out
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Rough token estimate (no tokenizer model needed).
pub fn estimate_tokens(s: &str) -> usize {
    (s.chars().count() / 4).max(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Rust,
    Python,
    TypeScript,
    Tsx,
    JavaScript,
    Markdown,
    Json,
    Yaml,
    Toml,
    Text,
    Binary,
}

impl Format {
    pub fn name(&self) -> &'static str {
        match self {
            Format::Rust => "code/rust",
            Format::Python => "code/python",
            Format::TypeScript => "code/typescript",
            Format::Tsx => "code/tsx",
            Format::JavaScript => "code/javascript",
            Format::Markdown => "markdown",
            Format::Json => "json",
            Format::Yaml => "yaml",
            Format::Toml => "toml",
            Format::Text => "text",
            Format::Binary => "binary",
        }
    }
    pub fn is_code(&self) -> bool {
        matches!(
            self,
            Format::Rust | Format::Python | Format::TypeScript | Format::Tsx | Format::JavaScript
        )
    }
}

/// Detect format from content first (binary sniff, trial JSON parse, shebang),
/// then the extension.
pub fn detect_format(path: &Path, bytes: &[u8]) -> Format {
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0u8) || std::str::from_utf8(head).is_err() && std::str::from_utf8(bytes).is_err() {
        return Format::Binary;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "rs" => return Format::Rust,
        "py" | "pyi" => return Format::Python,
        "ts" | "mts" | "cts" => return Format::TypeScript,
        "tsx" => return Format::Tsx,
        "js" | "mjs" | "cjs" | "jsx" => return Format::JavaScript,
        "md" | "markdown" | "mdx" => return Format::Markdown,
        "json" => return Format::Json,
        "yaml" | "yml" => return Format::Yaml,
        "toml" => return Format::Toml,
        _ => {}
    }
    let text = String::from_utf8_lossy(head);
    let first = text.lines().next().unwrap_or("");
    if first.starts_with("#!") {
        if first.contains("python") {
            return Format::Python;
        }
        if first.contains("node") {
            return Format::JavaScript;
        }
    }
    let trimmed = text.trim_start();
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_slice::<serde_json::Value>(bytes).is_ok()
    {
        return Format::Json;
    }
    if trimmed.starts_with("# ") || trimmed.contains("\n## ") {
        return Format::Markdown;
    }
    Format::Text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splits_identifiers() {
        assert_eq!(split_ident("verifyToken"), vec!["verify", "token"]);
        assert_eq!(split_ident("verify_token"), vec!["verify", "token"]);
        assert_eq!(split_ident("HTTPClient"), vec!["http", "client"]);
        assert!(identifier_expand("retryWithBackoff").contains("backoff"));
    }
    #[test]
    fn simhash_near_dup() {
        let a = simhash("Jitter in retry_with_backoff prevents retry storms when many workers fail");
        let b = simhash("Jitter in retry_with_backoff prevents retry storms when many workers fail together");
        let c = simhash("Database migrations must run inside a transaction");
        assert!(hamming(a, b) < hamming(a, c));
    }
}
