//! CSV encoding for the training dataset, replacing
//! `pandas.DataFrame.to_csv(index=False)`.

use crate::util::dt_to_csv_field;
use chrono::{DateTime, Utc};

pub struct TrainingRow {
    pub profile_id: String,
    pub item_price: f64,
    pub created_at: DateTime<Utc>,
}

/// `training_df.to_csv(index=False)` — columns `profile_id,item_price,created_at`.
pub fn training_rows_to_csv(rows: &[TrainingRow]) -> Result<String, csv::Error> {
    let mut writer = csv::WriterBuilder::new().has_headers(true).from_writer(Vec::new());
    writer.write_record(["profile_id", "item_price", "created_at"])?;
    for row in rows {
        writer.write_record(&[
            row.profile_id.clone(),
            format_price(row.item_price),
            dt_to_csv_field(row.created_at),
        ])?;
    }
    let bytes = writer.into_inner().map_err(|e| e.into_error())?;
    Ok(String::from_utf8(bytes).expect("csv writer produces valid utf8"))
}

/// pandas renders whole-number floats as e.g. `9.0`; mimic that instead of
/// Rust's default `9` for integral values, and otherwise use a plain decimal
/// (non-scientific) representation.
fn format_price(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn writes_header_and_rows() {
        let rows = vec![TrainingRow {
            profile_id: "p1".to_string(),
            item_price: 9.0,
            created_at: Utc.with_ymd_and_hms(2025, 1, 1, 12, 0, 0).unwrap(),
        }];
        let csv = training_rows_to_csv(&rows).unwrap();
        assert_eq!(csv, "profile_id,item_price,created_at\np1,9.0,2025-01-01 12:00:00\n");
    }
}
