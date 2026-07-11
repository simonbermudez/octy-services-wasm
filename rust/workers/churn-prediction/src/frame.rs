//! Minimal in-memory DataFrame — the pandas subset the churn pipeline uses.
//! Pure logic: no spin-sdk imports, natively unit-testable.
//!
//! Pandas semantics that are deliberately replicated:
//! * `pd.DataFrame(list_of_dicts)` — columns in first-seen key order, missing
//!   keys become nulls.
//! * `df.drop(cols, axis=1)` raises on a missing column (KeyError).
//! * `merge(..., how='outer')` sorts the join keys; `how='inner'` preserves
//!   the left frame's row order.
//! * `groupby` sorts group keys and excludes null keys.
//! * numeric-column detection mirrors `select_dtypes(include=np.number)`
//!   (bools are NOT numeric).
//! * `pd.unique` counts NaN once; `nunique()` excludes nulls.

use serde_json::{Map, Number, Value};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// Nested JSON (dict / list column values, e.g. `platform_info`).
    Json(Value),
}

impl Cell {
    pub fn from_json(v: &Value) -> Cell {
        match v {
            Value::Null => Cell::Null,
            Value::Bool(b) => Cell::Bool(*b),
            Value::Number(n) => match n.as_i64() {
                Some(i) => Cell::Int(i),
                None => Cell::Float(n.as_f64().unwrap_or(f64::NAN)),
            },
            Value::String(s) => Cell::Str(s.clone()),
            other => Cell::Json(other.clone()),
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            Cell::Null => Value::Null,
            Cell::Bool(b) => Value::Bool(*b),
            Cell::Int(i) => Value::Number((*i).into()),
            // NaN → null (pandas `to_json` emits null for NaN).
            Cell::Float(f) => Number::from_f64(*f).map(Value::Number).unwrap_or(Value::Null),
            Cell::Str(s) => Value::String(s.clone()),
            Cell::Json(v) => v.clone(),
        }
    }

    /// `pd.isnull` — None and NaN are null; dicts/lists are not.
    pub fn is_null(&self) -> bool {
        match self {
            Cell::Null => true,
            Cell::Float(f) => f.is_nan(),
            _ => false,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Cell::Int(i) => Some(*i as f64),
            Cell::Float(f) => Some(*f),
            Cell::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    #[allow(dead_code)] // part of the Cell API surface; exercised in tests
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Cell::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Grouping / uniqueness key. Numeric 1 == 1.0 (pandas hash semantics);
    /// NaN unifies with null.
    pub fn key(&self) -> CellKey {
        match self {
            Cell::Null => CellKey::Null,
            Cell::Bool(b) => CellKey::Bool(*b),
            Cell::Int(i) => CellKey::Num((*i as f64).to_bits()),
            Cell::Float(f) => {
                if f.is_nan() {
                    CellKey::Null
                } else {
                    let f = if *f == 0.0 { 0.0 } else { *f };
                    CellKey::Num(f.to_bits())
                }
            }
            Cell::Str(s) => CellKey::Str(s.clone()),
            Cell::Json(v) => CellKey::Str(v.to_string()),
        }
    }

    /// Python `str(value)` — used for dummy column names.
    pub fn py_str(&self) -> String {
        match self {
            Cell::Null => "nan".to_string(),
            Cell::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            Cell::Int(i) => i.to_string(),
            Cell::Float(f) => fmt_py_float(*f),
            Cell::Str(s) => s.clone(),
            Cell::Json(v) => v.to_string(),
        }
    }

    /// CSV rendering (pandas `to_csv`: null → empty, bools → True/False).
    pub fn csv(&self) -> String {
        match self {
            Cell::Null => String::new(),
            Cell::Float(f) if f.is_nan() => String::new(),
            Cell::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            Cell::Int(i) => i.to_string(),
            Cell::Float(f) => format!("{f}"),
            Cell::Str(s) => s.clone(),
            Cell::Json(v) => v.to_string(),
        }
    }
}

/// Python `str(float)`-alike: keep one decimal for integral floats.
pub fn fmt_py_float(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_string();
    }
    if f == f.trunc() && f.abs() < 1e16 {
        format!("{:.1}", f)
    } else {
        format!("{f}")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CellKey {
    Bool(bool),
    Num(u64),
    Str(String),
    Null,
}

impl CellKey {
    fn rank(&self) -> u8 {
        match self {
            CellKey::Bool(_) => 0,
            CellKey::Num(_) => 1,
            CellKey::Str(_) => 2,
            CellKey::Null => 3,
        }
    }
}

impl Ord for CellKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (CellKey::Bool(a), CellKey::Bool(b)) => a.cmp(b),
            (CellKey::Num(a), CellKey::Num(b)) => f64::from_bits(*a)
                .partial_cmp(&f64::from_bits(*b))
                .unwrap_or(Ordering::Equal),
            (CellKey::Str(a), CellKey::Str(b)) => a.cmp(b),
            (CellKey::Null, CellKey::Null) => Ordering::Equal,
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

impl PartialOrd for CellKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dtype {
    Bool,
    Int,
    Float,
    Object,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    Inner,
    Outer,
}

#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub cols: Vec<String>,
    /// Row-major cells; every row has `cols.len()` entries.
    pub rows: Vec<Vec<Cell>>,
}

impl Frame {
    pub fn new(cols: Vec<String>) -> Self {
        Self { cols, rows: Vec::new() }
    }

    /// `pd.DataFrame(list_of_dicts)`.
    ///
    /// CAVEAT: pandas preserves each source dict's key insertion order for
    /// column order. The workspace's `serde_json` is not built with the
    /// `preserve_order` feature, so `serde_json::Map` is backed by a
    /// `BTreeMap` — object keys are already alphabetised by the time this
    /// function sees them. Column *order* in the resulting CSV can therefore
    /// differ from the Python. This never affects correctness here: every
    /// downstream consumer (feature lists cached in Mongo, the XGBoost
    /// scorer, `_get_feature_columns`) selects columns by name, not
    /// position.
    pub fn from_records(records: &[Value]) -> Result<Frame, String> {
        let mut cols: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut maps: Vec<&Map<String, Value>> = Vec::with_capacity(records.len());
        for rec in records {
            let obj = rec
                .as_object()
                .ok_or_else(|| "DataFrame records must be JSON objects".to_string())?;
            for key in obj.keys() {
                if seen.insert(key.clone()) {
                    cols.push(key.clone());
                }
            }
            maps.push(obj);
        }
        let rows = maps
            .iter()
            .map(|obj| {
                cols.iter()
                    .map(|c| obj.get(c).map(Cell::from_json).unwrap_or(Cell::Null))
                    .collect()
            })
            .collect();
        Ok(Frame { cols, rows })
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    #[allow(dead_code)] // part of the Frame API surface (pandas `.empty`)
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn col_index(&self, name: &str) -> Option<usize> {
        self.cols.iter().position(|c| c == name)
    }

    fn require_col(&self, name: &str) -> Result<usize, String> {
        self.col_index(name)
            .ok_or_else(|| format!("['{name}'] not found in axis"))
    }

    pub fn col_cells(&self, name: &str) -> Result<Vec<Cell>, String> {
        let idx = self.require_col(name)?;
        Ok(self.rows.iter().map(|r| r[idx].clone()).collect())
    }

    /// `df.drop(names, axis=1)` — errors on any missing column.
    pub fn drop_cols(&mut self, names: &[&str]) -> Result<(), String> {
        let mut idxs: Vec<usize> = Vec::new();
        for name in names {
            let idx = self.require_col(name)?;
            if !idxs.contains(&idx) {
                idxs.push(idx);
            }
        }
        idxs.sort_unstable();
        for (removed, idx) in idxs.into_iter().enumerate() {
            let at = idx - removed;
            self.cols.remove(at);
            for row in &mut self.rows {
                row.remove(at);
            }
        }
        Ok(())
    }

    /// Removes a column and returns its cells.
    pub fn remove_col(&mut self, name: &str) -> Result<Vec<Cell>, String> {
        let idx = self.require_col(name)?;
        self.cols.remove(idx);
        Ok(self.rows.iter_mut().map(|r| r.remove(idx)).collect())
    }

    /// Adds (or replaces) a column at the end / in place.
    pub fn add_col(&mut self, name: &str, cells: Vec<Cell>) -> Result<(), String> {
        if cells.len() != self.rows.len() {
            return Err(format!(
                "column {name} length {} != frame length {}",
                cells.len(),
                self.rows.len()
            ));
        }
        match self.col_index(name) {
            Some(idx) => {
                for (row, cell) in self.rows.iter_mut().zip(cells) {
                    row[idx] = cell;
                }
            }
            None => {
                self.cols.push(name.to_string());
                for (row, cell) in self.rows.iter_mut().zip(cells) {
                    row.push(cell);
                }
            }
        }
        Ok(())
    }

    pub fn rename_col(&mut self, old: &str, new: &str) {
        if let Some(idx) = self.col_index(old) {
            self.cols[idx] = new.to_string();
        }
    }

    /// `df.columns = [...]` — positional rename of *every* column; pandas
    /// raises `ValueError: Length mismatch` when the lengths differ, which
    /// is exactly the failure mode `_get_profile_event_count` hits whenever
    /// the events carry extra `event_properties` keys (preserved here).
    pub fn rename_all(&mut self, names: &[String]) -> Result<(), String> {
        if names.len() != self.cols.len() {
            return Err(format!(
                "Length mismatch: Expected axis has {} elements, new values have {} elements",
                self.cols.len(),
                names.len()
            ));
        }
        self.cols = names.to_vec();
        Ok(())
    }

    /// `_get_dict_keys`: unique keys across a column of JSON-object cells,
    /// sorted (`np.unique`).
    pub fn dict_keys_union_sorted(&self, name: &str) -> Result<Vec<String>, String> {
        let idx = self.require_col(name)?;
        let mut keys: BTreeSet<String> = BTreeSet::new();
        for row in &self.rows {
            if let Cell::Json(Value::Object(obj)) = &row[idx] {
                for k in obj.keys() {
                    keys.insert(k.clone());
                }
            }
        }
        Ok(keys.into_iter().collect())
    }

    /// `_apply_dict_value`: pull `key` out of a JSON-object column, `Cell::Null`
    /// (NaN) when absent or the cell isn't an object.
    pub fn dict_value_col(&self, name: &str, key: &str) -> Result<Vec<Cell>, String> {
        let idx = self.require_col(name)?;
        Ok(self
            .rows
            .iter()
            .map(|row| match &row[idx] {
                Cell::Json(Value::Object(obj)) => {
                    obj.get(key).map(Cell::from_json).unwrap_or(Cell::Null)
                }
                _ => Cell::Null,
            })
            .collect())
    }

    /// `df.to_json(orient='index')` parsed back to a `{"0": {...}, "1": {...}}`
    /// map, in row order.
    pub fn to_json_index(&self) -> Value {
        let mut map = Map::new();
        for (i, row) in self.rows.iter().enumerate() {
            let mut obj = Map::new();
            for (c, cell) in self.cols.iter().zip(row) {
                obj.insert(c.clone(), cell.to_json());
            }
            map.insert(i.to_string(), Value::Object(obj));
        }
        Value::Object(map)
    }

    pub fn null_count(&self, name: &str) -> Result<usize, String> {
        let idx = self.require_col(name)?;
        Ok(self.rows.iter().filter(|r| r[idx].is_null()).count())
    }

    /// `df.dropna(how='any', axis=0)`.
    pub fn dropna_any(&mut self) {
        self.rows.retain(|r| !r.iter().any(Cell::is_null));
    }

    pub fn fillna(&mut self, name: &str, value: Cell) -> Result<(), String> {
        let idx = self.require_col(name)?;
        for row in &mut self.rows {
            if row[idx].is_null() {
                row[idx] = value.clone();
            }
        }
        Ok(())
    }

    /// `astype(int)` on a numeric column.
    pub fn cast_col_int(&mut self, name: &str) -> Result<(), String> {
        let idx = self.require_col(name)?;
        for row in &mut self.rows {
            row[idx] = match &row[idx] {
                Cell::Int(i) => Cell::Int(*i),
                Cell::Float(f) => Cell::Int(*f as i64),
                Cell::Bool(b) => Cell::Int(*b as i64),
                other => {
                    return Err(format!(
                        "cannot cast column {name} value {other:?} to int"
                    ))
                }
            };
        }
        Ok(())
    }

    /// `len(pd.unique(col))` — NaN/None counted once.
    pub fn unique_len_with_null(&self, name: &str) -> Result<usize, String> {
        let idx = self.require_col(name)?;
        let set: HashSet<CellKey> = self.rows.iter().map(|r| r[idx].key()).collect();
        Ok(set.len())
    }

    /// `col.nunique()` — nulls excluded.
    pub fn nunique(&self, name: &str) -> Result<usize, String> {
        let idx = self.require_col(name)?;
        let set: HashSet<CellKey> = self
            .rows
            .iter()
            .filter(|r| !r[idx].is_null())
            .map(|r| r[idx].key())
            .collect();
        Ok(set.len())
    }

    /// Column dtype (pandas inference over the cells).
    pub fn dtype(&self, name: &str) -> Result<Dtype, String> {
        let idx = self.require_col(name)?;
        let mut saw_bool = false;
        let mut saw_int = false;
        let mut saw_float = false;
        let mut saw_object = false;
        let mut saw_null = false;
        for row in &self.rows {
            match &row[idx] {
                Cell::Null => saw_null = true,
                Cell::Bool(_) => saw_bool = true,
                Cell::Int(_) => saw_int = true,
                Cell::Float(f) if f.is_nan() => saw_null = true,
                Cell::Float(_) => saw_float = true,
                Cell::Str(_) | Cell::Json(_) => saw_object = true,
            }
        }
        if saw_object {
            return Ok(Dtype::Object);
        }
        if saw_bool {
            // bool + anything else (numbers or nulls) → object in pandas.
            return Ok(if saw_int || saw_float || saw_null {
                Dtype::Object
            } else {
                Dtype::Bool
            });
        }
        if saw_float || saw_null {
            // NaN promotes ints to float64; an all-null column is float64.
            return Ok(Dtype::Float);
        }
        if saw_int {
            return Ok(Dtype::Int);
        }
        // Empty frame: pandas would say object.
        Ok(Dtype::Object)
    }

    /// `select_dtypes(include=np.number).columns` (bools excluded).
    pub fn numeric_cols(&self) -> Vec<String> {
        self.cols
            .iter()
            .filter(|c| matches!(self.dtype(c), Ok(Dtype::Int) | Ok(Dtype::Float)))
            .cloned()
            .collect()
    }

    /// `pd.merge(left, right, on=..., how=...)`.
    pub fn merge(&self, right: &Frame, on: &str, how: How) -> Result<Frame, String> {
        let li = self.require_col(on)?;
        let ri = right.require_col(on)?;
        let right_keep: Vec<usize> = (0..right.cols.len()).filter(|i| *i != ri).collect();

        let mut cols = self.cols.clone();
        for i in &right_keep {
            cols.push(right.cols[*i].clone());
        }

        let mut rmap: HashMap<CellKey, Vec<usize>> = HashMap::new();
        for (i, row) in right.rows.iter().enumerate() {
            rmap.entry(row[ri].key()).or_default().push(i);
        }

        let mut out = Frame::new(cols);
        let emit = |out: &mut Frame, lrow: Option<&Vec<Cell>>, rrow: Option<&Vec<Cell>>, key: &Cell| {
            let mut row: Vec<Cell> = match lrow {
                Some(l) => l.clone(),
                None => {
                    let mut r = vec![Cell::Null; self.cols.len()];
                    r[li] = key.clone();
                    r
                }
            };
            for i in &right_keep {
                row.push(match rrow {
                    Some(r) => r[*i].clone(),
                    None => Cell::Null,
                });
            }
            out.rows.push(row);
        };

        match how {
            How::Inner => {
                for lrow in &self.rows {
                    if let Some(idxs) = rmap.get(&lrow[li].key()) {
                        for i in idxs {
                            emit(&mut out, Some(lrow), Some(&right.rows[*i]), &lrow[li]);
                        }
                    }
                }
            }
            How::Outer => {
                // pandas sorts the join keys lexicographically on outer merges.
                let mut lmap: BTreeMap<CellKey, Vec<usize>> = BTreeMap::new();
                for (i, row) in self.rows.iter().enumerate() {
                    lmap.entry(row[li].key()).or_default().push(i);
                }
                let mut keys: BTreeSet<CellKey> = lmap.keys().cloned().collect();
                keys.extend(rmap.keys().cloned());
                for key in keys {
                    let lrows = lmap.get(&key);
                    let rrows = rmap.get(&key);
                    match (lrows, rrows) {
                        (Some(ls), Some(rs)) => {
                            for l in ls {
                                for r in rs {
                                    emit(
                                        &mut out,
                                        Some(&self.rows[*l]),
                                        Some(&right.rows[*r]),
                                        &self.rows[*l][li],
                                    );
                                }
                            }
                        }
                        (Some(ls), None) => {
                            for l in ls {
                                emit(&mut out, Some(&self.rows[*l]), None, &self.rows[*l][li]);
                            }
                        }
                        (None, Some(rs)) => {
                            for r in rs {
                                emit(&mut out, None, Some(&right.rows[*r]), &right.rows[*r][ri]);
                            }
                        }
                        (None, None) => {}
                    }
                }
            }
        }
        Ok(out)
    }

    /// Group rows by a key column, sorted keys, null keys excluded.
    fn groups(&self, key: &str) -> Result<(usize, BTreeMap<CellKey, (Cell, Vec<usize>)>), String> {
        let ki = self.require_col(key)?;
        let mut groups: BTreeMap<CellKey, (Cell, Vec<usize>)> = BTreeMap::new();
        for (i, row) in self.rows.iter().enumerate() {
            if row[ki].is_null() {
                continue; // pandas groupby drops null keys
            }
            groups
                .entry(row[ki].key())
                .or_insert_with(|| (row[ki].clone(), Vec::new()))
                .1
                .push(i);
        }
        Ok((ki, groups))
    }

    /// `df.groupby(key)[val].sum().reset_index()` → frame [key, val].
    pub fn groupby_sum(&self, key: &str, val: &str) -> Result<Frame, String> {
        let vi = self.require_col(val)?;
        let (_, groups) = self.groups(key)?;
        let mut out = Frame::new(vec![key.to_string(), val.to_string()]);
        for (_, (kcell, idxs)) in groups {
            let mut sum = 0.0;
            for i in &idxs {
                if let Some(v) = self.rows[*i][vi].as_f64() {
                    if !v.is_nan() {
                        sum += v;
                    }
                }
            }
            out.rows.push(vec![kcell, Cell::Float(sum)]);
        }
        Ok(out)
    }

    /// `df.groupby(key).count().reset_index()` — non-null counts per column.
    pub fn groupby_count(&self, key: &str) -> Result<Frame, String> {
        let (ki, groups) = self.groups(key)?;
        let mut cols = vec![key.to_string()];
        let val_idx: Vec<usize> = (0..self.cols.len()).filter(|i| *i != ki).collect();
        for i in &val_idx {
            cols.push(self.cols[*i].clone());
        }
        let mut out = Frame::new(cols);
        for (_, (kcell, idxs)) in groups {
            let mut row = vec![kcell];
            for c in &val_idx {
                let count = idxs.iter().filter(|i| !self.rows[**i][*c].is_null()).count();
                row.push(Cell::Int(count as i64));
            }
            out.rows.push(row);
        }
        Ok(out)
    }

    /// `df.groupby([key]).agg(most_frequent)` — per column, the value with the
    /// highest count; ties break on first occurrence (collections.Counter).
    pub fn groupby_most_frequent(&self, key: &str) -> Result<Frame, String> {
        let (ki, groups) = self.groups(key)?;
        let mut cols = vec![key.to_string()];
        let val_idx: Vec<usize> = (0..self.cols.len()).filter(|i| *i != ki).collect();
        for i in &val_idx {
            cols.push(self.cols[*i].clone());
        }
        let mut out = Frame::new(cols);
        for (_, (kcell, idxs)) in groups {
            let mut row = vec![kcell];
            for c in &val_idx {
                // first-seen order with counts
                let mut order: Vec<(CellKey, Cell, usize)> = Vec::new();
                for i in &idxs {
                    let cell = &self.rows[*i][*c];
                    let k = cell.key();
                    match order.iter_mut().find(|(ok, _, _)| *ok == k) {
                        Some(entry) => entry.2 += 1,
                        None => order.push((k, cell.clone(), 1)),
                    }
                }
                let best = order
                    .iter()
                    .max_by(|a, b| a.2.cmp(&b.2).then(Ordering::Greater)) // keep first max
                    .map(|(_, cell, _)| cell.clone())
                    .unwrap_or(Cell::Null);
                row.push(best);
            }
            out.rows.push(row);
        }
        Ok(out)
    }

    /// `df.groupby(key)[val].mean()` as a map keyed by group.
    #[allow(dead_code)] // part of the Frame API surface; cluster-mean ranking
                        // in `encode.rs` computes this inline instead.
    pub fn group_means(&self, key: &str, val: &str) -> Result<Vec<(Cell, f64)>, String> {
        let vi = self.require_col(val)?;
        let (_, groups) = self.groups(key)?;
        let mut out = Vec::new();
        for (_, (kcell, idxs)) in groups {
            let vals: Vec<f64> = idxs
                .iter()
                .filter_map(|i| self.rows[*i][vi].as_f64())
                .filter(|v| !v.is_nan())
                .collect();
            let mean = if vals.is_empty() {
                f64::NAN
            } else {
                vals.iter().sum::<f64>() / vals.len() as f64
            };
            out.push((kcell, mean));
        }
        Ok(out)
    }

    /// Keep only the listed columns, in the given order (`df[[...]]`).
    pub fn select(&self, names: &[&str]) -> Result<Frame, String> {
        let idxs: Vec<usize> = names
            .iter()
            .map(|n| self.require_col(n))
            .collect::<Result<_, _>>()?;
        Ok(Frame {
            cols: names.iter().map(|s| s.to_string()).collect(),
            rows: self
                .rows
                .iter()
                .map(|r| idxs.iter().map(|i| r[*i].clone()).collect())
                .collect(),
        })
    }

    /// Row values for a fixed column order, as `f64` (NaN for null/non-numeric
    /// cells) — the feature vector layout XGBoost scoring expects.
    pub fn row_as_f64(&self, row_idx: usize, cols: &[String]) -> Result<Vec<f64>, String> {
        let idxs: Vec<usize> = cols
            .iter()
            .map(|c| self.require_col(c))
            .collect::<Result<_, _>>()?;
        Ok(idxs
            .iter()
            .map(|i| self.rows[row_idx][*i].as_f64().unwrap_or(f64::NAN))
            .collect())
    }

    /// `df.to_csv(index=False)`.
    pub fn to_csv(&self) -> String {
        let mut out = String::new();
        let escape = |s: &str| -> String {
            if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_string()
            }
        };
        out.push_str(
            &self
                .cols
                .iter()
                .map(|c| escape(c))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
        for row in &self.rows {
            out.push_str(
                &row.iter()
                    .map(|c| escape(&c.csv()))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn records_union_columns() {
        let f = Frame::from_records(&[json!({"a": 1, "b": "x"}), json!({"b": "y", "c": 2.5})])
            .unwrap();
        assert_eq!(f.cols, vec!["a", "b", "c"]);
        assert!(f.rows[0][2].is_null());
        assert!(f.rows[1][0].is_null());
    }

    #[test]
    fn outer_merge_sorts_keys() {
        let l = Frame::from_records(&[json!({"k": "b", "v": 1}), json!({"k": "a", "v": 2})]).unwrap();
        let r = Frame::from_records(&[json!({"k": "b", "w": 9})]).unwrap();
        let m = l.merge(&r, "k", How::Outer).unwrap();
        assert_eq!(m.rows[0][0].as_str(), Some("a"));
        assert_eq!(m.rows[1][0].as_str(), Some("b"));
        assert_eq!(m.rows[1][2], Cell::Int(9));
        assert!(m.rows[0][2].is_null());
    }

    #[test]
    fn dtype_rules() {
        let f = Frame::from_records(&[
            json!({"i": 1, "f": 1.5, "b": true, "o": "s", "m": 1}),
            json!({"i": 2, "f": null, "b": false, "o": "t", "m": "x"}),
        ])
        .unwrap();
        assert_eq!(f.dtype("i").unwrap(), Dtype::Int);
        assert_eq!(f.dtype("f").unwrap(), Dtype::Float);
        assert_eq!(f.dtype("b").unwrap(), Dtype::Bool);
        assert_eq!(f.dtype("o").unwrap(), Dtype::Object);
        assert_eq!(f.dtype("m").unwrap(), Dtype::Object);
        let mut numeric = f.numeric_cols();
        numeric.sort();
        assert_eq!(numeric, vec!["f".to_string(), "i".to_string()]);
    }
}
