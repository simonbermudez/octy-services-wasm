//! Minimal in-memory dataframe replacing the worker's pandas usage.
//! Pure logic — no spin-sdk imports (natively unit-testable).
//!
//! Semantics intentionally mirror the pandas operations the Python worker
//! used: `pd.DataFrame(list_of_dicts)` (columns = ordered union of keys,
//! missing cells = NaN/Null), `dropna(how='any')`, `drop(columns)`,
//! positional column rename, `insert(0, …, range(len))`, `merge(how='left')`
//! with `_x`/`_y` suffixes on overlapping non-key columns, and
//! `to_csv(index=False)`.

use anyhow::{anyhow, bail, Result};
use serde_json::{Map, Value};

use crate::utils::{py_cell_str, values_equal};

#[derive(Debug, Clone, Default)]
pub struct Frame {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

impl Frame {
    /// `pd.DataFrame(records)` — columns are the union of the record keys in
    /// first-appearance order; records missing a key get Null (NaN).
    pub fn from_records(records: &[Map<String, Value>]) -> Self {
        let mut columns: Vec<String> = Vec::new();
        for record in records {
            for key in record.keys() {
                if !columns.iter().any(|c| c == key) {
                    columns.push(key.clone());
                }
            }
        }
        let rows = records
            .iter()
            .map(|record| {
                columns
                    .iter()
                    .map(|c| record.get(c).cloned().unwrap_or(Value::Null))
                    .collect()
            })
            .collect();
        Self { columns, rows }
    }

    /// `pd.DataFrame(records, columns=[…])` — only the listed keys are kept.
    pub fn from_records_with_columns(records: &[Map<String, Value>], columns: &[String]) -> Self {
        let rows = records
            .iter()
            .map(|record| {
                columns
                    .iter()
                    .map(|c| record.get(c).cloned().unwrap_or(Value::Null))
                    .collect()
            })
            .collect();
        Self {
            columns: columns.to_vec(),
            rows,
        }
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn column_index(&self, name: &str) -> Result<usize> {
        self.columns
            .iter()
            .position(|c| c == name)
            .ok_or_else(|| anyhow!("column not found: {name}"))
    }

    pub fn column_values(&self, name: &str) -> Result<Vec<Value>> {
        let idx = self.column_index(name)?;
        Ok(self.rows.iter().map(|row| row[idx].clone()).collect())
    }

    /// `df.dropna(how='any', axis=0, inplace=True)`.
    pub fn dropna(&mut self) {
        self.rows.retain(|row| !row.iter().any(Value::is_null));
    }

    /// `df.drop([...], axis=1)` — like pandas, unknown columns are an error.
    pub fn drop_columns(&mut self, names: &[&str]) -> Result<()> {
        let mut indices = Vec::new();
        for name in names {
            indices.push(self.column_index(name)?);
        }
        indices.sort_unstable();
        indices.reverse();
        for idx in indices {
            self.columns.remove(idx);
            for row in &mut self.rows {
                row.remove(idx);
            }
        }
        Ok(())
    }

    /// `df.columns = [...]` — positional rename; width must match (pandas
    /// raises a length-mismatch ValueError otherwise).
    pub fn set_columns(&mut self, names: &[String]) -> Result<()> {
        if names.len() != self.columns.len() {
            bail!(
                "Length mismatch: Expected axis has {} elements, new values have {} elements",
                self.columns.len(),
                names.len()
            );
        }
        self.columns = names.to_vec();
        Ok(())
    }

    /// `df.insert(0, name, range(0, len(df)))`.
    pub fn insert_range_index(&mut self, name: &str) {
        self.columns.insert(0, name.to_string());
        for (i, row) in self.rows.iter_mut().enumerate() {
            row.insert(0, Value::from(i as i64));
        }
    }

    /// `left.merge(right, how='left', left_on=…, right_on=…)`.
    ///
    /// * When the key columns share a name, a single key column is kept
    ///   (pandas behaviour); otherwise both are kept.
    /// * Overlapping non-key column names get `_x`/`_y` suffixes.
    /// * Null keys never match (NaN != NaN).
    /// * Duplicate right-side matches fan out the left row (cartesian).
    pub fn merge_left(&self, right: &Frame, left_on: &str, right_on: &str) -> Result<Frame> {
        let left_key = self.column_index(left_on)?;
        let right_key = right.column_index(right_on)?;
        let same_key_name = left_on == right_on;

        // Right columns kept in the output (all of them, unless the key
        // column is shared by name).
        let right_kept: Vec<usize> = (0..right.columns.len())
            .filter(|&i| !(same_key_name && i == right_key))
            .collect();

        // Overlap detection for suffixing (pandas suffixes both sides).
        let overlap: Vec<&String> = self
            .columns
            .iter()
            .filter(|c| {
                right_kept
                    .iter()
                    .any(|&ri| &right.columns[ri] == *c)
            })
            .collect();
        let overlaps = |name: &str| overlap.iter().any(|c| c.as_str() == name);

        let mut columns: Vec<String> = self
            .columns
            .iter()
            .map(|c| {
                if overlaps(c) {
                    format!("{c}_x")
                } else {
                    c.clone()
                }
            })
            .collect();
        for &ri in &right_kept {
            let c = &right.columns[ri];
            columns.push(if overlaps(c) { format!("{c}_y") } else { c.clone() });
        }

        let mut rows: Vec<Vec<Value>> = Vec::with_capacity(self.rows.len());
        for left_row in &self.rows {
            let key = &left_row[left_key];
            let mut matched = false;
            if !key.is_null() {
                for right_row in &right.rows {
                    let rk = &right_row[right_key];
                    if !rk.is_null() && values_equal(key, rk) {
                        matched = true;
                        let mut row = left_row.clone();
                        for &ri in &right_kept {
                            row.push(right_row[ri].clone());
                        }
                        rows.push(row);
                    }
                }
            }
            if !matched {
                let mut row = left_row.clone();
                for _ in &right_kept {
                    row.push(Value::Null);
                }
                rows.push(row);
            }
        }

        Ok(Frame { columns, rows })
    }

    /// `df.to_csv(index=False)` — header + rows, `\n` terminated, quoting
    /// only when needed (pandas/csv defaults).
    pub fn to_csv(&self) -> Result<String> {
        let mut writer = csv::WriterBuilder::new()
            .quote_style(csv::QuoteStyle::Necessary)
            .from_writer(Vec::new());
        writer.write_record(&self.columns)?;
        for row in &self.rows {
            let record: Vec<String> = row.iter().map(py_cell_str).collect();
            writer.write_record(&record)?;
        }
        let bytes = writer.into_inner().map_err(|e| anyhow!("csv flush: {e}"))?;
        Ok(String::from_utf8(bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn records_union_and_dropna() {
        let mut frame = Frame::from_records(&[
            record(json!({"a": 1, "b": "x"})),
            record(json!({"a": 2, "c": true})),
        ]);
        assert_eq!(frame.columns, vec!["a", "b", "c"]);
        frame.dropna();
        assert_eq!(frame.len(), 0);
    }

    #[test]
    fn merge_left_same_key() {
        let left = Frame::from_records(&[
            record(json!({"profile_id": "p1", "variable_value": "i1"})),
            record(json!({"profile_id": "p2", "variable_value": null})),
        ]);
        let right = Frame::from_records(&[record(json!({"profile_id": "p1", "rfm": 555}))]);
        let merged = left.merge_left(&right, "profile_id", "profile_id").unwrap();
        assert_eq!(merged.columns, vec!["profile_id", "variable_value", "rfm"]);
        assert_eq!(merged.rows[0][2], json!(555));
        assert_eq!(merged.rows[1][2], Value::Null);
    }

    #[test]
    fn merge_left_different_keys_keeps_both() {
        let left = Frame::from_records(&[record(json!({"variable_value": "i1"}))]);
        let right = Frame::from_records(&[record(json!({"item_id": "i1", "item_name": "n"}))]);
        let merged = left.merge_left(&right, "variable_value", "item_id").unwrap();
        assert_eq!(merged.columns, vec!["variable_value", "item_id", "item_name"]);
    }

    #[test]
    fn csv_python_formatting() {
        let mut frame = Frame::from_records(&[record(json!({
            "id": "a,b",
            "flag": true,
            "cats": ["x", "y"],
            "price": 1.5
        }))]);
        frame.insert_range_index("idx");
        let csv = frame.to_csv().unwrap();
        assert_eq!(csv, "idx,id,flag,cats,price\n0,\"a,b\",True,\"['x', 'y']\",1.5\n");
    }
}
