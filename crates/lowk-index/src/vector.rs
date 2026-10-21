//! Vector half of the index behind a trait. The shipped implementation is a flat in-memory
//! scan over int8-quantised vectors (one scale per vector): 100k x 512 is 51 MB and a query
//! is a few milliseconds, so no ANN structure is needed at this scale (docs/DECISIONS-M2.md).
//! `sqlite-vec` can be slotted in behind the same trait if the scale ever demands it.
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

pub trait VectorIndex: Send + Sync {
    fn dim(&self) -> usize;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// `id` is the chunk rowid, `file` its file (for filtering). `blob` is an encoded vector.
    fn upsert(&mut self, id: i64, file: i64, blob: &[u8]);
    fn remove(&mut self, id: i64);
    fn clear(&mut self);
    /// Top `k` by cosine (vectors are unit length), restricted to `allow` files when given.
    fn search(&self, query: &[f32], k: usize, allow: Option<&HashSet<i64>>) -> Vec<(i64, f32)>;
}

/// Unit-normalises and quantises: `dim` i8 values followed by the f32 LE scale.
pub fn encode(v: &[f32]) -> Vec<u8> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    let max = v.iter().fold(0f32, |m, x| m.max(x.abs())) / norm;
    let scale = (max / 127.0).max(1e-12);
    let mut out = Vec::with_capacity(v.len() + 4);
    for x in v {
        out.push(((x / norm / scale).round().clamp(-127.0, 127.0) as i8) as u8);
    }
    out.extend_from_slice(&scale.to_le_bytes());
    out
}

pub fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter_mut().for_each(|x| *x /= n);
}

#[derive(Default)]
pub struct FlatI8 {
    dim: usize,
    ids: Vec<i64>,
    files: Vec<i64>,
    scales: Vec<f32>,
    data: Vec<i8>,
    slot: HashMap<i64, usize>,
}

impl FlatI8 {
    pub fn new(dim: usize) -> Self {
        Self {
            dim,
            ..Self::default()
        }
    }
}

#[derive(PartialEq)]
struct F(f32);
impl Eq for F {}
impl PartialOrd for F {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for F {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.partial_cmp(&o.0).unwrap_or(std::cmp::Ordering::Equal)
    }
}

impl FlatI8 {
    /// `q` is the query quantised to i8 with scale `qs`.
    fn scan(
        &self,
        lo: usize,
        hi: usize,
        q: &[i8],
        qs: f32,
        k: usize,
        allow: Option<&HashSet<i64>>,
    ) -> Vec<(f32, i64)> {
        let mut heap: BinaryHeap<Reverse<(F, i64)>> = BinaryHeap::with_capacity(k + 1);
        for s in lo..hi {
            if let Some(a) = allow {
                if !a.contains(&self.files[s]) {
                    continue;
                }
            }
            let row = &self.data[s * self.dim..(s + 1) * self.dim];
            // i8 x i8 -> i32: autovectorises to widening multiply-accumulate (NEON / AVX2)
            let acc: i32 = row
                .iter()
                .zip(q)
                .map(|(a, b)| i32::from(*a) * i32::from(*b))
                .sum();
            let score = acc as f32 * self.scales[s] * qs;
            if heap.len() < k {
                heap.push(Reverse((F(score), self.ids[s])));
            } else if heap.peek().is_some_and(|Reverse((m, _))| score > m.0) {
                heap.pop();
                heap.push(Reverse((F(score), self.ids[s])));
            }
        }
        heap.into_iter().map(|Reverse((F(s), id))| (s, id)).collect()
    }
}

impl VectorIndex for FlatI8 {
    fn dim(&self) -> usize {
        self.dim
    }
    fn len(&self) -> usize {
        self.ids.len()
    }
    fn upsert(&mut self, id: i64, file: i64, blob: &[u8]) {
        if blob.len() != self.dim + 4 {
            return;
        }
        let scale = f32::from_le_bytes(blob[self.dim..].try_into().expect("4 bytes"));
        let row: Vec<i8> = blob[..self.dim].iter().map(|b| *b as i8).collect();
        if let Some(&s) = self.slot.get(&id) {
            self.data[s * self.dim..(s + 1) * self.dim].copy_from_slice(&row);
            self.scales[s] = scale;
            self.files[s] = file;
            return;
        }
        self.slot.insert(id, self.ids.len());
        self.ids.push(id);
        self.files.push(file);
        self.scales.push(scale);
        self.data.extend_from_slice(&row);
    }
    fn remove(&mut self, id: i64) {
        let Some(s) = self.slot.remove(&id) else { return };
        let last = self.ids.len() - 1;
        if s != last {
            let (a, b) = self.data.split_at_mut(last * self.dim);
            a[s * self.dim..(s + 1) * self.dim].copy_from_slice(&b[..self.dim]);
            self.ids[s] = self.ids[last];
            self.files[s] = self.files[last];
            self.scales[s] = self.scales[last];
            self.slot.insert(self.ids[s], s);
        }
        self.ids.pop();
        self.files.pop();
        self.scales.pop();
        self.data.truncate(last * self.dim);
    }
    fn clear(&mut self) {
        *self = Self::new(self.dim);
    }
    fn search(&self, query: &[f32], k: usize, allow: Option<&HashSet<i64>>) -> Vec<(i64, f32)> {
        if query.len() != self.dim || k == 0 || self.ids.is_empty() {
            return vec![];
        }
        let n = self.ids.len();
        let qmax = query.iter().fold(0f32, |m, x| m.max(x.abs())).max(1e-12);
        let qs = qmax / 127.0;
        let qi: Vec<i8> = query
            .iter()
            .map(|x| (x / qs).round().clamp(-127.0, 127.0) as i8)
            .collect();
        let query = &qi;
        let threads = if n < 30_000 {
            1
        } else {
            std::thread::available_parallelism()
                .map(|x| x.get())
                .unwrap_or(1)
                .min(8)
        };
        let mut all: Vec<(f32, i64)> = if threads == 1 {
            self.scan(0, n, query, qs, k, allow)
        } else {
            let step = n.div_ceil(threads);
            std::thread::scope(|sc| {
                let hs: Vec<_> = (0..threads)
                    .map(|t| {
                        let (lo, hi) = (t * step, ((t + 1) * step).min(n));
                        sc.spawn(move || self.scan(lo.min(hi), hi, query, qs, k, allow))
                    })
                    .collect();
                hs.into_iter()
                    .flat_map(|h| h.join().unwrap_or_default())
                    .collect()
            })
        };
        all.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.1.cmp(&b.1))
        });
        all.truncate(k);
        all.into_iter().map(|(s, id)| (id, s)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(seed: u32, dim: usize) -> Vec<f32> {
        (0..dim)
            .map(|i| (((seed * 31 + i as u32 * 17) % 97) as f32) - 48.0)
            .collect()
    }

    #[test]
    fn nearest_is_itself_and_remove_works() {
        let mut ix = FlatI8::new(64);
        for i in 0..200 {
            ix.upsert(i, i % 5, &encode(&v(i as u32, 64)));
        }
        let mut q = v(42, 64);
        normalize(&mut q);
        let r = ix.search(&q, 3, None);
        assert_eq!(r[0].0, 42);
        assert!(r[0].1 > 0.98);
        let allow: HashSet<i64> = [0].into_iter().collect();
        assert!(ix.search(&q, 50, Some(&allow)).iter().all(|(id, _)| id % 5 == 0));
        ix.remove(42);
        assert_ne!(ix.search(&q, 1, None)[0].0, 42);
        assert_eq!(ix.len(), 199);
    }
}
