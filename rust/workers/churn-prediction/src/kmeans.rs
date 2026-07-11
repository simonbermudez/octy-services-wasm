//! 1-D KMeans (k-means++ init + Lloyd iterations) — replacement for
//! `sklearn.cluster.KMeans` as used by the churn pipeline (always fitted on a
//! single feature column). Deterministic via a seeded RNG, mirroring the
//! Python's `random_state=42` on the elbow sweep. Pure logic, no spin-sdk.

pub struct SplitMix(u64);

impl SplitMix {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_add(0x9E37_79B9_7F4A_7C15))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn next_index(&mut self, n: usize) -> usize {
        ((self.next_f64() * n as f64) as usize).min(n.saturating_sub(1))
    }
}

/// Returns `(labels, centers, inertia)`.
pub fn kmeans_1d(data: &[f64], k: usize, max_iter: usize, seed: u64) -> (Vec<usize>, Vec<f64>, f64) {
    let n = data.len();
    if n == 0 || k == 0 {
        return (Vec::new(), Vec::new(), 0.0);
    }
    let k = k.min(n);
    let mut rng = SplitMix::new(seed);

    // --- k-means++ initialisation ---
    let mut centers: Vec<f64> = Vec::with_capacity(k);
    centers.push(data[rng.next_index(n)]);
    let mut dist2: Vec<f64> = data
        .iter()
        .map(|x| (x - centers[0]) * (x - centers[0]))
        .collect();
    while centers.len() < k {
        let total: f64 = dist2.iter().sum();
        let next = if total <= 0.0 {
            data[rng.next_index(n)]
        } else {
            let mut target = rng.next_f64() * total;
            let mut chosen = n - 1;
            for (i, d) in dist2.iter().enumerate() {
                target -= d;
                if target <= 0.0 {
                    chosen = i;
                    break;
                }
            }
            data[chosen]
        };
        centers.push(next);
        for (i, x) in data.iter().enumerate() {
            let d = (x - next) * (x - next);
            if d < dist2[i] {
                dist2[i] = d;
            }
        }
    }

    // --- Lloyd iterations ---
    let mut labels = vec![0usize; n];
    for _ in 0..max_iter.max(1) {
        let mut changed = false;
        for (i, x) in data.iter().enumerate() {
            let mut best = 0usize;
            let mut best_d = f64::INFINITY;
            for (c, center) in centers.iter().enumerate() {
                let d = (x - center) * (x - center);
                if d < best_d {
                    best_d = d;
                    best = c;
                }
            }
            if labels[i] != best {
                labels[i] = best;
                changed = true;
            }
        }

        let mut sums = vec![0.0f64; centers.len()];
        let mut counts = vec![0usize; centers.len()];
        for (i, x) in data.iter().enumerate() {
            sums[labels[i]] += x;
            counts[labels[i]] += 1;
        }
        for c in 0..centers.len() {
            if counts[c] > 0 {
                centers[c] = sums[c] / counts[c] as f64;
            } else {
                // Re-seed an empty cluster with the point farthest from its center.
                let far = data
                    .iter()
                    .enumerate()
                    .max_by(|a, b| {
                        let da = (a.1 - centers[labels[a.0]]).abs();
                        let db = (b.1 - centers[labels[b.0]]).abs();
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(_, x)| *x)
                    .unwrap_or(centers[c]);
                centers[c] = far;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let inertia: f64 = data
        .iter()
        .zip(&labels)
        .map(|(x, l)| (x - centers[*l]) * (x - centers[*l]))
        .sum();
    (labels, centers, inertia)
}
