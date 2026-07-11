//! Feature-encoding ports of `services/churn_prediction.py`:
//! `_numerical_cluster_encoding`, `_numerical_bin_encoding`,
//! `_categorical_encoding` and `_format_column_names`.
//! Pure logic, no spin-sdk.

use crate::frame::{Cell, CellKey, Dtype, Frame};
use crate::{kmeans, knee};
use std::collections::BTreeSet;

/// `_numerical_cluster_encoding` / `_numerical_clustering_encoding`:
/// KMeans-cluster a numeric column, pick k via the kneedle elbow (capped at
/// 5), rank clusters by their mean and replace the column with an appended
/// `{feature}_cluster` label column (low/mid/…/high).
///
/// Returns Ok(false) when skipped (fewer than 30 observations) — the Python
/// silently `return`ed, leaving the numeric column in place.
pub fn numerical_cluster_encoding(
    df: &mut Frame,
    feature: &str,
    ascending: bool,
) -> Result<bool, String> {
    if df.len() < 30 {
        return Ok(false);
    }

    let cells = df.col_cells(feature)?;
    let mut vals: Vec<f64> = Vec::with_capacity(cells.len());
    for c in &cells {
        match c.as_f64() {
            Some(v) if !v.is_nan() => vals.push(v),
            _ => {
                return Err(format!(
                    "cannot cluster column {feature}: non-numeric or NaN value present"
                ))
            }
        }
    }

    let len_unique = df.unique_len_with_null(feature)?;
    eprintln!("Number of unique values in column {feature} : {len_unique}");
    // Python: range(1, 10) — the elbow sweep only tests cluster sizes 1..9
    // when the column has at least that many unique values.
    let k_end = if len_unique < 10 { len_unique } else { 10 };
    if k_end <= 1 {
        return Err(format!(
            "cannot determine clusters for column {feature}: not enough unique values"
        ));
    }
    let ks: Vec<usize> = (1..k_end).collect();
    let xs: Vec<f64> = ks.iter().map(|k| *k as f64).collect();
    let inertias: Vec<f64> = ks
        .iter()
        .map(|k| kmeans::kmeans_1d(&vals, *k, 1000, 42).2)
        .collect();

    let knee_x = knee::find_knee_convex_decreasing(&xs, &inertias, 1.0).ok_or_else(|| {
        format!("KneeLocator found no knee for column {feature} (Python compared None > 5)")
    })?;
    let mut knee = knee_x.round() as usize;
    eprintln!("Original knee: {knee}");
    if knee > 5 {
        eprintln!("Limited knee to : 5");
        knee = 5;
    }
    let label_map: &[&str] = match knee {
        2 => &["low", "high"],
        3 => &["low", "mid", "high"],
        4 => &["low", "mid", "mid-high", "high"],
        5 => &["low", "mid", "mid-high", "high", "top-high"],
        other => {
            // Python NameError: label_map is only defined for 2..=5.
            return Err(format!(
                "label_map undefined for knee={other} while clustering {feature} (Python NameError)"
            ));
        }
    };
    eprintln!("{feature}:");
    eprintln!("creating a K-means cluster with {knee} clusters");

    let (labels, _centers, _) = kmeans::kmeans_1d(&vals, knee, 300, 42);

    // Rank clusters by their column mean (ascending like the Python call sites).
    let mut sums = vec![0.0f64; knee];
    let mut counts = vec![0usize; knee];
    for (l, v) in labels.iter().zip(&vals) {
        sums[*l] += v;
        counts[*l] += 1;
    }
    let mut order: Vec<usize> = (0..knee).collect();
    order.sort_by(|a, b| {
        let ma = if counts[*a] > 0 { sums[*a] / counts[*a] as f64 } else { f64::NAN };
        let mb = if counts[*b] > 0 { sums[*b] / counts[*b] as f64 } else { f64::NAN };
        let cmp = ma.partial_cmp(&mb).unwrap_or(std::cmp::Ordering::Equal);
        if ascending { cmp } else { cmp.reverse() }
    });
    let mut rank = vec![0usize; knee];
    for (r, cluster) in order.iter().enumerate() {
        rank[*cluster] = r;
    }

    let cluster_field = format!("{feature}_cluster");
    let label_cells: Vec<Cell> = labels
        .iter()
        .map(|l| Cell::Str(label_map[rank[*l]].to_string()))
        .collect();
    df.remove_col(feature)?;
    df.add_col(&cluster_field, label_cells)?;
    eprintln!("=============== END CLUSTER FUNC ===============");
    Ok(true)
}

/// `_numerical_bin_encoding`: `pd.cut(col, bins)` + one-hot with the column
/// name as prefix, then drop the source column. All bins get a dummy column
/// even when empty (pd.cut yields a fixed Categorical).
pub fn numerical_bin_encoding(df: &mut Frame, feature: &str, bins: usize) -> Result<(), String> {
    eprintln!("Creating {bins} bins for column {feature}");
    let cells = df.col_cells(feature)?;
    let non_null: Vec<f64> = cells
        .iter()
        .filter_map(Cell::as_f64)
        .filter(|v| !v.is_nan())
        .collect();
    if non_null.is_empty() {
        return Err(format!("cannot bin column {feature}: no numeric values"));
    }
    let mn = non_null.iter().cloned().fold(f64::INFINITY, f64::min);
    let mx = non_null.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    // pandas edge computation
    let (lo, hi, adjust_first) = if mx == mn {
        let lo = if mn != 0.0 { mn - 0.001 * mn.abs() } else { -0.001 };
        let hi = if mx != 0.0 { mx + 0.001 * mx.abs() } else { 0.001 };
        (lo, hi, false)
    } else {
        (mn, mx, true)
    };
    let mut edges: Vec<f64> = (0..=bins)
        .map(|i| lo + (hi - lo) * i as f64 / bins as f64)
        .collect();
    if adjust_first {
        edges[0] = lo - (hi - lo) * 0.001;
    }

    // bin index per row (None for nulls)
    let bin_of = |v: f64| -> Option<usize> {
        for b in 0..bins {
            if v > edges[b] && v <= edges[b + 1] {
                return Some(b);
            }
        }
        None
    };
    let assignments: Vec<Option<usize>> = cells
        .iter()
        .map(|c| match c.as_f64() {
            Some(v) if !v.is_nan() => bin_of(v),
            _ => None,
        })
        .collect();

    eprintln!("One hot encoding {feature}_bins");
    df.remove_col(feature)?;
    for b in 0..bins {
        let name = format!(
            "{feature}_({}, {}]",
            fmt_edge(edges[b]),
            fmt_edge(edges[b + 1])
        );
        let col: Vec<Cell> = assignments
            .iter()
            .map(|a| Cell::Int((*a == Some(b)) as i64))
            .collect();
        df.add_col(&name, col)?;
    }
    eprintln!("=============== END BIN FUNC ===============");
    Ok(())
}

/// pandas interval-label edge formatting (precision=3).
fn fmt_edge(x: f64) -> String {
    let r = (x * 1000.0).round() / 1000.0;
    let s = format!("{r}");
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("nan") {
        s
    } else {
        format!("{s}.0")
    }
}

/// `_categorical_encoding`: one-hot every object-dtype column not in the stop
/// list (`pd.get_dummies(data=df, columns=dummy_columns)` — dummies appended
/// at the end, categories sorted).
pub fn categorical_encoding(df: &mut Frame, stop_list: &[&str]) -> Result<(), String> {
    let dummy_columns: Vec<String> = df
        .cols
        .iter()
        .filter(|c| {
            !stop_list.contains(&c.as_str())
                && matches!(df.dtype(c), Ok(Dtype::Object))
        })
        .cloned()
        .collect();

    for col in dummy_columns {
        let cells = df.remove_col(&col)?;
        let mut categories: BTreeSet<CellKey> = BTreeSet::new();
        let mut display: Vec<(CellKey, String)> = Vec::new();
        for c in &cells {
            if c.is_null() {
                continue;
            }
            let k = c.key();
            if categories.insert(k.clone()) {
                display.push((k, c.py_str()));
            }
        }
        display.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, label) in display {
            let name = format!("{col}_{label}");
            let dummies: Vec<Cell> = cells
                .iter()
                .map(|c| Cell::Int((!c.is_null() && c.key() == key) as i64))
                .collect();
            df.add_col(&name, dummies)?;
        }
    }
    Ok(())
}

/// `_format_column_names`: replace illegal characters with `_`.
pub fn format_column_names(df: &mut Frame) {
    const ILLEGAL: &[char] = &[
        '\\', '`', '*', '_', '{', '}', '[', ']', '(', ')', '>', '#', '+', '-', '.', '!', '$',
        '\'', ',', ' ',
    ];
    let mut renames: Vec<(String, String)> = Vec::new();
    for col in df.cols.iter_mut() {
        let formatted: String = col
            .chars()
            .map(|ch| if ILLEGAL.contains(&ch) { '_' } else { ch })
            .collect();
        if formatted != *col {
            renames.push((col.clone(), formatted.clone()));
            *col = formatted;
        }
    }
    if !renames.is_empty() {
        eprintln!("Formatting column names : {renames:?}");
    }
}
