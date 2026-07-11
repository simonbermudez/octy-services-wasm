//! Port of `kneed.KneeLocator` for the single configuration the Python worker
//! uses: `S=1.0, curve='convex', direction='decreasing'` (elbow of a KMeans
//! inertia curve). Pure logic, no spin-sdk.
//!
//! Returns the x value at the detected knee, or `None` when no knee is found —
//! the Python then crashed comparing `None > 5` (TypeError), which the callers
//! surface as a job error.

pub fn find_knee_convex_decreasing(x: &[f64], y: &[f64], s: f64) -> Option<f64> {
    let n = x.len();
    if n < 3 || y.len() != n {
        return None;
    }

    let (xmin, xmax) = min_max(x)?;
    let (ymin, ymax) = min_max(y)?;
    if xmax == xmin || ymax == ymin {
        return None;
    }
    let x_norm: Vec<f64> = x.iter().map(|v| (v - xmin) / (xmax - xmin)).collect();
    let y_norm: Vec<f64> = y.iter().map(|v| (v - ymin) / (ymax - ymin)).collect();

    // convex + decreasing → flip vertically into a concave increasing curve.
    let y_max = y_norm.iter().cloned().fold(f64::MIN, f64::max);
    let y_t: Vec<f64> = y_norm.iter().map(|v| y_max - v).collect();

    // difference curve
    let y_diff: Vec<f64> = y_t.iter().zip(&x_norm).map(|(a, b)| a - b).collect();

    // local maxima / minima (argrelextrema with >= / <=, endpoints excluded)
    let mut maxima: Vec<usize> = Vec::new();
    let mut minima: Vec<usize> = Vec::new();
    for i in 1..n - 1 {
        if y_diff[i] >= y_diff[i - 1] && y_diff[i] >= y_diff[i + 1] {
            maxima.push(i);
        }
        if y_diff[i] <= y_diff[i - 1] && y_diff[i] <= y_diff[i + 1] {
            minima.push(i);
        }
    }
    if maxima.is_empty() {
        return None;
    }

    // threshold offset: S * mean(diff(x_norm))
    let mean_dx = x_norm
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .sum::<f64>()
        / (n - 1) as f64;

    let mut threshold = 0.0f64;
    let mut threshold_index = maxima[0];
    let mut have_threshold = false;

    for i in maxima[0]..n {
        if maxima.contains(&i) {
            threshold = y_diff[i] - s * mean_dx;
            threshold_index = i;
            have_threshold = true;
        }
        if minima.contains(&i) {
            threshold = 0.0;
            have_threshold = true;
        }
        let j = i + 1;
        if j < n && have_threshold && y_diff[j] < threshold {
            return Some(x[threshold_index]);
        }
    }
    None
}

fn min_max(v: &[f64]) -> Option<(f64, f64)> {
    let mut mn = f64::INFINITY;
    let mut mx = f64::NEG_INFINITY;
    for x in v {
        if x.is_nan() {
            return None;
        }
        mn = mn.min(*x);
        mx = mx.max(*x);
    }
    Some((mn, mx))
}
