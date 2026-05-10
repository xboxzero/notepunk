use crate::model::Note;
use std::collections::{HashMap, HashSet};

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "but", "with", "you", "your", "are", "was", "were", "this", "that",
    "from", "have", "has", "had", "not", "they", "them", "their", "there", "what", "when",
    "which", "who", "whom", "will", "would", "could", "should", "into", "than", "then", "also",
    "any", "all", "out", "off", "its", "about", "over", "some", "such", "just", "only", "very",
    "much", "more", "most", "few", "other", "another", "where", "while", "before", "after",
    "between", "during", "above", "below", "under", "again", "once", "here", "still", "yet",
    "ever", "never", "often", "sometimes", "always", "let", "yes", "yep", "nope", "ok", "okay",
    "https", "http", "com", "org", "www",
];

pub fn is_stopword(token: &str) -> bool {
    STOPWORDS.binary_search(&token).is_ok()
        || STOPWORDS.contains(&token)
        || token.chars().all(|c| c.is_ascii_digit())
}

pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() >= 3 && !is_stopword(s))
        .map(String::from)
        .collect()
}

pub fn extract_wikilinks(body: &str) -> Vec<String> {
    let mut links = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let start = i + 2;
            let mut j = start;
            let mut closed = false;
            while j + 1 < bytes.len() {
                if bytes[j] == b']' && bytes[j + 1] == b']' {
                    if let Ok(s) = std::str::from_utf8(&bytes[start..j]) {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() {
                            links.push(trimmed.to_string());
                        }
                    }
                    i = j + 2;
                    closed = true;
                    break;
                }
                j += 1;
            }
            if !closed {
                break;
            }
        } else {
            i += 1;
        }
    }
    links
}

pub fn extract_tags(body: &str) -> Vec<String> {
    let mut tags = HashSet::new();
    let mut chars = body.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '#' {
            continue;
        }
        if i > 0 {
            let prev = body[..i].chars().last();
            if let Some(p) = prev {
                if p.is_alphanumeric() || p == '_' {
                    continue;
                }
            }
        }
        let rest = &body[i + 1..];
        let end: usize = rest
            .char_indices()
            .find(|(_, ch)| !(ch.is_alphanumeric() || *ch == '_' || *ch == '-'))
            .map(|(idx, _)| idx)
            .unwrap_or(rest.len());
        if end == 0 {
            continue;
        }
        let tag = &rest[..end];
        if tag.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        tags.insert(tag.to_lowercase());
    }
    let mut out: Vec<String> = tags.into_iter().collect();
    out.sort();
    out
}

pub fn extract_first_image(body: &str) -> Option<String> {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'!' && bytes[i + 1] == b'[' {
            let close = body[i + 2..].find("](")?;
            let url_start = i + 2 + close + 2;
            let url_end = body[url_start..].find(')')?;
            return Some(body[url_start..url_start + url_end].to_string());
        }
        i += 1;
    }
    None
}

pub struct TfIdfModel {
    pub df: HashMap<String, usize>,
    pub n_docs: usize,
}

impl TfIdfModel {
    pub fn build(notes: &[Note]) -> Self {
        let mut df: HashMap<String, usize> = HashMap::new();
        for n in notes {
            let combined = format!("{} {}", n.title, n.body);
            let unique: HashSet<String> = tokenize(&combined).into_iter().collect();
            for tok in unique {
                *df.entry(tok).or_insert(0) += 1;
            }
        }
        Self {
            df,
            n_docs: notes.len(),
        }
    }

    pub fn vector(&self, title: &str, body: &str) -> HashMap<String, f64> {
        let combined = format!("{} {}", title, body);
        let tokens = tokenize(&combined);
        if tokens.is_empty() {
            return HashMap::new();
        }
        let mut tf: HashMap<String, f64> = HashMap::new();
        for t in &tokens {
            *tf.entry(t.clone()).or_insert(0.0) += 1.0;
        }
        let total = tokens.len() as f64;
        let n = self.n_docs as f64;
        let mut vec: HashMap<String, f64> = HashMap::new();
        for (term, count) in tf {
            let df = *self.df.get(&term).unwrap_or(&1) as f64;
            let idf = ((n + 1.0) / (df + 1.0)).ln() + 1.0;
            let weight = (count / total) * idf;
            if weight > 0.0 {
                vec.insert(term, weight);
            }
        }
        vec
    }
}

pub fn cosine(a: &HashMap<String, f64>, b: &HashMap<String, f64>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (small, large) = if a.len() < b.len() { (a, b) } else { (b, a) };
    let mut dot = 0.0;
    for (k, v) in small {
        if let Some(w) = large.get(k) {
            dot += v * w;
        }
    }
    if dot == 0.0 {
        return 0.0;
    }
    let na: f64 = a.values().map(|v| v * v).sum::<f64>().sqrt();
    let nb: f64 = b.values().map(|v| v * v).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na * nb)
}

pub fn body_matches_query(title: &str, body: &str, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    if title.to_lowercase().contains(&q) {
        return true;
    }
    body.to_lowercase().contains(&q)
}
