//! Hand-rolled k-means++ / Lloyd's-algorithm clustering and Kneedle elbow
//! detection — Rust ports of `sklearn.cluster.KMeans` and
//! `kneed.KneeLocator`.
//!
//! ## Where this fits in the RFM pipeline
//!
//! `workers/rfm/services/rfm.py` imports `sklearn.cluster.KMeans`,
//! `sklearn.preprocessing.LabelEncoder` and `kneed.KneeLocator` — but never
//! actually calls any of them. The real RFM clustering algorithm (which
//! features get clustered, how `rfm_score` / `segment_description` are
//! derived) runs entirely inside the external SageMaker training container
//! referenced by `Config['RFM_ALGORITHM_DOCKER_PATH']`
//! (`.../octy/rfm_analysis:latest`), which is **not** part of the
//! `workers/rfm` Python source tree and therefore out of scope for a
//! line-for-line port. `training.rs` / `complete.rs` faithfully port the
//! *orchestration* around that opaque container (build dataset, upload,
//! launch/poll the SageMaker job, relay its output).
//!
//! The task brief nonetheless calls for a hand-rolled, tested KMeans +
//! Kneedle implementation. The one place in this codebase family where
//! `KMeans`/`KneeLocator` are concretely invoked (with real parameters) is
//! the sibling `workers/churn_prediction/services/churn_prediction.py`
//! (`_numerical_cluster_encoding`):
//!
//! ```python
//! kmeans = KMeans(n_clusters=num_clusters, max_iter=1000, init='k-means++', random_state=42).fit(df_cluster)
//! cluster_errors.append(kmeans.inertia_)
//! ...
//! elbow = KneeLocator(clusters_df.num_clusters.values, clusters_df.cluster_errors.values,
//!                      S=1.0, curve='convex', direction='decreasing')
//! knee = min(elbow.knee, 5)
//! kmeans = KMeans(n_clusters=knee).fit(...)
//! ```
//!
//! This module ports exactly that pattern — single/multi-feature k-means++
//! with an inertia curve, and a Kneedle knee-finder specialised for
//! `curve='convex', direction='decreasing', S=1.0` (the only combination
//! used anywhere in this codebase) — as a general-purpose, independently
//! tested building block, ready to be wired into a from-scratch RFM/churn
//! training path if the SageMaker container is ever replaced with in-process
//! training.
//!
//! ## Known numerical divergences from scikit-learn / kneed
//!
//! * **RNG**: sklearn seeds NumPy's Mersenne Twister; we use a small
//!   deterministic xorshift64* PRNG seeded the same way (`random_state`).
//!   Same *determinism* (same seed -> same result every run), different
//!   *stream*, so cluster assignments for a given seed will not bit-for-bit
//!   match sklearn's, though converged inertia values should be very close
//!   for well-separated data (k-means++ + Lloyd's is deterministic to the
//!   init sequence, not the RNG implementation itself).
//! * **Convergence tolerance**: sklearn stops on `tol` (relative centroid
//!   movement, default `1e-4`) or `max_iter`. We stop on max_iter or exact
//!   centroid stability (no point changed cluster) — slightly stricter, so
//!   this port may run a few more iterations than sklearn for the same data.
//! * **Kneedle**: only the `convex`/`decreasing`/`S=1.0` case is
//!   implemented (the only case exercised anywhere in this codebase). The
//!   acceptance rule (first local maximum of the difference curve that
//!   isn't beaten by `S * mean(Δx)` before the next local maximum) follows
//!   Satopaa et al. 2011, but interpolation edge cases and plateau handling
//!   may not agree bit-for-bit with the `kneed` package's implementation.

/// A small, dependency-free deterministic PRNG (xorshift64*) — WASI
/// `random_get` availability varies by host, and sklearn's `random_state`
/// contract only promises *reproducibility*, not a specific algorithm, so a
/// self-contained PRNG keeps this module free of extra WASM-portability risk.
struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Self { state: seed.wrapping_mul(0x9E3779B97F4A7C15).max(1) }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    /// Uniform `f64` in `[0, 1)`.
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform index in `[0, n)`.
    fn next_index(&mut self, n: usize) -> usize {
        (self.next_f64() * n as f64) as usize
    }
}

fn squared_dist(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum()
}

/// Result of a k-means fit, mirroring the sklearn attributes used by the
/// Python callers (`.inertia_`, `.predict(...)`).
#[derive(Debug, Clone)]
pub struct KMeansFit {
    pub centroids: Vec<Vec<f64>>,
    pub labels: Vec<usize>,
    pub inertia: f64,
}

/// Port of `KMeans(n_clusters=k, max_iter=..., init='k-means++', random_state=seed).fit(data)`.
///
/// `data` is `n_samples` rows of `n_features` columns (row-major), matching
/// the shape of a `df_cluster` single- or multi-column selection.
pub fn kmeans_fit(data: &[Vec<f64>], k: usize, max_iter: usize, seed: u64) -> KMeansFit {
    assert!(k >= 1, "k must be >= 1");
    assert!(!data.is_empty(), "data must be non-empty");
    let n = data.len();
    let k = k.min(n);

    let mut rng = Xorshift64::new(seed);
    let mut centroids = kmeans_plus_plus_init(data, k, &mut rng);
    let mut labels = vec![0usize; n];

    for _ in 0..max_iter.max(1) {
        let mut changed = false;
        // Assignment step.
        for (i, point) in data.iter().enumerate() {
            let mut best = 0usize;
            let mut best_dist = f64::INFINITY;
            for (c, centroid) in centroids.iter().enumerate() {
                let d = squared_dist(point, centroid);
                if d < best_dist {
                    best_dist = d;
                    best = c;
                }
            }
            if labels[i] != best {
                changed = true;
            }
            labels[i] = best;
        }

        // Update step.
        let n_features = data[0].len();
        let mut sums = vec![vec![0.0; n_features]; k];
        let mut counts = vec![0usize; k];
        for (point, &label) in data.iter().zip(&labels) {
            counts[label] += 1;
            for (s, v) in sums[label].iter_mut().zip(point) {
                *s += v;
            }
        }
        for c in 0..k {
            if counts[c] == 0 {
                continue; // keep previous centroid for empty clusters
            }
            for (s, v) in sums[c].iter().zip(centroids[c].iter_mut()) {
                *v = s / counts[c] as f64;
            }
        }

        if !changed {
            break;
        }
    }

    let inertia: f64 = data
        .iter()
        .zip(&labels)
        .map(|(point, &label)| squared_dist(point, &centroids[label]))
        .sum();

    KMeansFit { centroids, labels, inertia }
}

fn kmeans_plus_plus_init(data: &[Vec<f64>], k: usize, rng: &mut Xorshift64) -> Vec<Vec<f64>> {
    let n = data.len();
    let mut centroids = Vec::with_capacity(k);
    centroids.push(data[rng.next_index(n)].clone());

    while centroids.len() < k {
        let mut dist_sq = vec![0.0; n];
        let mut total = 0.0;
        for (i, point) in data.iter().enumerate() {
            let d = centroids
                .iter()
                .map(|c| squared_dist(point, c))
                .fold(f64::INFINITY, f64::min);
            dist_sq[i] = d;
            total += d;
        }

        if total <= 0.0 {
            // All remaining points coincide with existing centroids; pick
            // arbitrarily to keep `k` distinct centroid slots filled.
            centroids.push(data[rng.next_index(n)].clone());
            continue;
        }

        let target = rng.next_f64() * total;
        let mut acc = 0.0;
        let mut chosen = n - 1;
        for (i, &d) in dist_sq.iter().enumerate() {
            acc += d;
            if acc >= target {
                chosen = i;
                break;
            }
        }
        centroids.push(data[chosen].clone());
    }

    centroids
}

/// Kneedle knee detection specialised for `curve='convex',
/// direction='decreasing'` (the only combination used in this codebase's
/// `KneeLocator(..., S=1.0, curve='convex', direction='decreasing')` calls).
/// `x` must be sorted ascending. Returns `None` if no knee is found (e.g.
/// fewer than 3 points, or a monotonically flat curve).
pub fn kneedle_convex_decreasing(x: &[f64], y: &[f64], sensitivity_s: f64) -> Option<f64> {
    if x.len() != y.len() || x.len() < 3 {
        return None;
    }
    let n = x.len();

    let x_min = x.iter().cloned().fold(f64::INFINITY, f64::min);
    let x_max = x.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let y_min = y.iter().cloned().fold(f64::INFINITY, f64::min);
    let y_max = y.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if (x_max - x_min).abs() < f64::EPSILON || (y_max - y_min).abs() < f64::EPSILON {
        return None;
    }

    let x_norm: Vec<f64> = x.iter().map(|v| (v - x_min) / (x_max - x_min)).collect();
    // Convex-decreasing -> flip vertically to get the canonical
    // concave-increasing shape Kneedle's difference curve expects.
    let y_flipped: Vec<f64> = y.iter().map(|v| 1.0 - (v - y_min) / (y_max - y_min)).collect();

    let diff: Vec<f64> = x_norm.iter().zip(&y_flipped).map(|(xi, yi)| yi - xi).collect();

    // Local maxima (interior points >= both neighbours, strictly greater
    // than at least one, to admit short plateaus).
    let mut maxima: Vec<usize> = Vec::new();
    for i in 1..n - 1 {
        if diff[i] >= diff[i - 1] && diff[i] >= diff[i + 1] && (diff[i] > diff[i - 1] || diff[i] > diff[i + 1]) {
            maxima.push(i);
        }
    }
    if maxima.is_empty() {
        return None;
    }

    let avg_step = {
        let mut steps: Vec<f64> = x_norm.windows(2).map(|w| w[1] - w[0]).collect();
        steps.retain(|s| s.is_finite());
        if steps.is_empty() {
            return None;
        }
        steps.iter().sum::<f64>() / steps.len() as f64
    };
    let threshold_step = sensitivity_s * avg_step;

    for (idx, &m) in maxima.iter().enumerate() {
        let threshold_value = diff[m] - threshold_step;
        let next_maximum = maxima.get(idx + 1).copied().unwrap_or(n);
        let mut survives = true;
        for j in (m + 1)..next_maximum.min(n) {
            if diff[j] < threshold_value {
                survives = false;
                break;
            }
        }
        if survives {
            return Some(x[m]);
        }
    }

    // Fall back to the last local maximum, matching Kneedle's behaviour of
    // always returning *a* knee once local maxima exist.
    maxima.last().map(|&i| x[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kmeans_separates_two_clusters() {
        let data: Vec<Vec<f64>> = vec![
            vec![0.0], vec![0.2], vec![-0.1], vec![0.1],
            vec![10.0], vec![10.2], vec![9.9], vec![10.1],
        ];
        let fit = kmeans_fit(&data, 2, 100, 42);
        // The two low-value points and two high-value points must each be
        // assigned to the same cluster as their neighbours.
        assert_eq!(fit.labels[0], fit.labels[1]);
        assert_eq!(fit.labels[0], fit.labels[2]);
        assert_eq!(fit.labels[0], fit.labels[3]);
        assert_eq!(fit.labels[4], fit.labels[5]);
        assert_eq!(fit.labels[4], fit.labels[6]);
        assert_eq!(fit.labels[4], fit.labels[7]);
        assert_ne!(fit.labels[0], fit.labels[4]);
        assert!(fit.inertia < 1.0, "inertia should be small for tight clusters, got {}", fit.inertia);
    }

    #[test]
    fn kmeans_is_deterministic_for_seed() {
        let data: Vec<Vec<f64>> = (0..20).map(|i| vec![(i % 5) as f64 * 10.0]).collect();
        let a = kmeans_fit(&data, 5, 300, 42);
        let b = kmeans_fit(&data, 5, 300, 42);
        assert_eq!(a.labels, b.labels);
        assert!((a.inertia - b.inertia).abs() < 1e-9);
    }

    #[test]
    fn kmeans_single_cluster_is_the_mean() {
        let data = vec![vec![1.0], vec![2.0], vec![3.0]];
        let fit = kmeans_fit(&data, 1, 10, 1);
        assert!((fit.centroids[0][0] - 2.0).abs() < 1e-9);
    }

    #[test]
    fn kneedle_finds_elbow_in_classic_curve() {
        // Classic elbow: sharp drop then near-flat tail.
        let x: Vec<f64> = (1..=9).map(|v| v as f64).collect();
        let y = vec![100.0, 40.0, 20.0, 15.0, 13.0, 12.0, 11.5, 11.2, 11.0];
        let knee = kneedle_convex_decreasing(&x, &y, 1.0).expect("expected a knee");
        // The elbow sits around cluster count 3-4 for this curve shape.
        assert!((3.0..=4.0).contains(&knee), "knee={knee}");
    }

    #[test]
    fn kneedle_returns_none_for_flat_curve() {
        let x: Vec<f64> = (1..=5).map(|v| v as f64).collect();
        let y = vec![10.0; 5];
        assert_eq!(kneedle_convex_decreasing(&x, &y, 1.0), None);
    }

    #[test]
    fn kneedle_requires_at_least_three_points() {
        assert_eq!(kneedle_convex_decreasing(&[1.0, 2.0], &[1.0, 2.0], 1.0), None);
    }
}
