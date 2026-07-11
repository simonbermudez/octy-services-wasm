//! Local scoring of an XGBoost model saved in XGBoost's native **JSON**
//! format (`Booster.save_model('model.json')`) — the Rust replacement for
//! `joblib.load('trained_churn_prediction_model.pkl').predict_proba(...)`.
//!
//! BREAKING ARTIFACT CHANGE (see the crate README in lib.rs docs): the
//! SageMaker training image must ship the trained booster as
//! `trained_churn_prediction_model.json` (native JSON dump) instead of a
//! joblib pickle — pickles cannot be read outside CPython.
//!
//! Scoring implements gbtree + `binary:logistic`: walk each tree, sum leaf
//! values, add the base margin, apply the sigmoid. Missing values (NaN)
//! follow the recorded default branch. Pure logic, no spin-sdk.

use serde_json::Value;

pub struct Tree {
    left: Vec<i64>,
    right: Vec<i64>,
    split_index: Vec<usize>,
    split_condition: Vec<f64>,
    default_left: Vec<bool>,
}

pub struct XgbModel {
    trees: Vec<Tree>,
    base_margin: f64,
    logistic: bool,
    /// Kept for diagnostics/future validation against `X_pred_cols`; the
    /// scorer itself indexes by position, not by name.
    #[allow(dead_code)]
    pub feature_names: Option<Vec<String>>,
    #[allow(dead_code)]
    pub num_feature: usize,
}

impl XgbModel {
    pub fn parse(bytes: &[u8]) -> Result<XgbModel, String> {
        let root: Value = serde_json::from_slice(bytes)
            .map_err(|e| format!("trained model is not valid XGBoost JSON: {e}"))?;
        let learner = root
            .get("learner")
            .ok_or("XGBoost JSON missing 'learner'")?;

        let objective = learner
            .pointer("/objective/name")
            .and_then(Value::as_str)
            .unwrap_or("binary:logistic");
        let logistic = objective == "binary:logistic" || objective == "reg:logistic";

        let base_score: f64 = learner
            .pointer("/learner_model_param/base_score")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .or_else(|| {
                learner
                    .pointer("/learner_model_param/base_score")
                    .and_then(Value::as_f64)
            })
            .unwrap_or(0.5);
        // For logistic objectives XGBoost stores base_score in probability
        // space and converts it to a margin at load time (ProbToMargin).
        let base_margin = if logistic {
            let p = base_score.clamp(1e-7, 1.0 - 1e-7);
            (p / (1.0 - p)).ln()
        } else {
            base_score
        };

        let num_feature = learner
            .pointer("/learner_model_param/num_feature")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);

        let feature_names = learner
            .get("feature_names")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());

        let trees_json = learner
            .pointer("/gradient_booster/model/trees")
            .and_then(Value::as_array)
            .ok_or("XGBoost JSON missing gradient_booster.model.trees (only gbtree JSON models are supported)")?;

        let mut trees = Vec::with_capacity(trees_json.len());
        for t in trees_json {
            trees.push(Tree {
                left: int_array(t, "left_children")?,
                right: int_array(t, "right_children")?,
                split_index: int_array(t, "split_indices")?
                    .into_iter()
                    .map(|v| v.max(0) as usize)
                    .collect(),
                split_condition: float_array(t, "split_conditions")?,
                default_left: int_array(t, "default_left")?
                    .into_iter()
                    .map(|v| v != 0)
                    .collect(),
            });
        }

        Ok(XgbModel {
            trees,
            base_margin,
            logistic,
            feature_names,
            num_feature,
        })
    }

    /// `predict_proba(x)[:, 1]` for one row. NaN entries take the default branch.
    pub fn predict_proba(&self, x: &[f64]) -> f64 {
        let mut margin = self.base_margin;
        for tree in &self.trees {
            margin += tree.leaf_value(x);
        }
        if self.logistic {
            1.0 / (1.0 + (-margin).exp())
        } else {
            margin
        }
    }
}

impl Tree {
    fn leaf_value(&self, x: &[f64]) -> f64 {
        let mut node = 0usize;
        loop {
            let left = self.left[node];
            if left == -1 {
                // leaf: split_conditions holds the leaf value
                return self.split_condition[node];
            }
            let fidx = self.split_index[node];
            let v = x.get(fidx).copied().unwrap_or(f64::NAN);
            let go_left = if v.is_nan() {
                self.default_left[node]
            } else {
                v < self.split_condition[node]
            };
            node = if go_left {
                left as usize
            } else {
                self.right[node] as usize
            };
        }
    }
}

fn int_array(tree: &Value, key: &str) -> Result<Vec<i64>, String> {
    tree.get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|v| {
                    v.as_i64()
                        .or_else(|| v.as_f64().map(|f| f as i64))
                        .or_else(|| v.as_bool().map(|b| b as i64))
                        .unwrap_or(0)
                })
                .collect()
        })
        .ok_or_else(|| format!("XGBoost tree missing '{key}'"))
}

fn float_array(tree: &Value, key: &str) -> Result<Vec<f64>, String> {
    tree.get(key)
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
        .ok_or_else(|| format!("XGBoost tree missing '{key}'"))
}
