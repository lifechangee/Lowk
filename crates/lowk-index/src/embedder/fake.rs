//! Deterministic embedder for tests and benchmarks: hashed bag of stems and stem bigrams.
//! It has no semantics beyond word overlap; tests that need "semantic" behaviour use
//! `with_synonyms` to fold listed words onto one concept.
use super::Embedder;
use crate::text::words;
use lowk_types::Result;
use std::collections::HashMap;

pub struct HashEmbedder {
    cloud: bool,
    dim: usize,
    syn: HashMap<String, String>,
    id: String,
}

impl HashEmbedder {
    pub fn new(dim: usize) -> Self {
        Self {
            cloud: false,
            dim,
            syn: HashMap::new(),
            id: format!("hash-test-{dim}"),
        }
    }
    /// Behaves like a cloud embedder for policy tests (`is_cloud`), and records what it was asked.
    pub fn pretend_cloud(mut self) -> Self {
        self.cloud = true;
        self.id.push_str("-cloud");
        self
    }
    /// Words (stems are taken) in one group map to the same concept: a poor man's semantics.
    pub fn with_synonyms(mut self, groups: &[&[&str]]) -> Self {
        for g in groups {
            let canon = crate::text::stem_text(g[0]);
            for w in *g {
                self.syn.insert(crate::text::stem_text(w), canon.clone());
            }
        }
        self.id.push_str("-syn");
        self
    }
    fn vec(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0f32; self.dim];
        let stems: Vec<String> = words(text)
            .into_iter()
            .map(|w| self.syn.get(&w.stem).cloned().unwrap_or(w.stem))
            .collect();
        let mut add = |s: &str, w: f32| {
            let h = blake3::hash(s.as_bytes());
            let b = h.as_bytes();
            let i = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize % self.dim;
            v[i] += if b[4] & 1 == 0 { w } else { -w };
        };
        for s in &stems {
            add(s, 1.0);
        }
        for p in stems.windows(2) {
            add(&format!("{} {}", p[0], p[1]), 0.5);
        }
        v
    }
}

impl Embedder for HashEmbedder {
    fn is_cloud(&self) -> bool {
        self.cloud
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn dim(&self) -> usize {
        self.dim
    }
    fn format_doc(&self, _title: &str, text: &str) -> String {
        text.to_string()
    }
    fn format_query(&self, q: &str) -> String {
        q.to_string()
    }
    fn embed_docs(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| self.vec(t)).collect())
    }
}
