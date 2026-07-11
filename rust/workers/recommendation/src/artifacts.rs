//! Model artifact parsing — replaces the `joblib.load(...pkl)` calls in
//! `RecommenderCompleteTrainingJob._get_job_artifacts`.
//!
//! BREAKING ARTIFACT FORMAT CHANGE
//! -------------------------------
//! The Python worker unpickled a LightFM model
//! (`trained_recommender_model.pkl`) and read its user/item representations
//! via `get_user_representations()` / `get_item_representations()`.
//! Pickle/joblib cannot be read from Rust, so the training image's
//! `model.tar.gz` must now contain a JSON export of those representations:
//!
//!   trained_recommender_model.json
//!   {
//!     "users_biases":     [f64, …],           // get_user_representations()[0]
//!     "users_embeddings": [[f64, …], …],       // get_user_representations()[1]
//!     "items_biases":     [f64, …],           // get_item_representations()[0]
//!     "items_embeddings": [[f64, …], …]        // get_item_representations()[1]
//!   }
//!
//! `model_meta_data.json` is unchanged. `lfm_item_features.pkl` /
//! `lfm_profile_features.pkl` were downloaded and unpickled by the Python
//! worker but never used, so they are ignored here (no replacement needed).
//! A tarball still containing only the `.pkl` model fails with an explicit
//! error rather than a silent mis-read. Pure logic — no spin-sdk.

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::tar_gz::{entry_basename, TarEntry};

/// The LightFM user/item representations the prediction pass needs.
#[derive(Debug, Deserialize)]
pub struct LfmModel {
    pub users_biases: Vec<f64>,
    pub users_embeddings: Vec<Vec<f64>>,
    pub items_biases: Vec<f64>,
    pub items_embeddings: Vec<Vec<f64>>,
}

pub struct JobArtifacts {
    pub model_meta: Value,
    pub model: LfmModel,
}

pub fn parse_job_artifacts(files: &[TarEntry]) -> Result<JobArtifacts> {
    let mut model_meta: Option<Value> = None;
    let mut model: Option<LfmModel> = None;
    let mut legacy_pickle = false;

    for file in files {
        match entry_basename(&file.name) {
            "model_meta_data.json" => {
                model_meta = Some(serde_json::from_slice(&file.data)?);
            }
            "trained_recommender_model.json" => {
                model = Some(
                    serde_json::from_slice(&file.data)
                        .map_err(|e| anyhow!("invalid trained_recommender_model.json: {e}"))?,
                );
            }
            "trained_recommender_model.pkl" => legacy_pickle = true,
            // lfm_item_features.pkl / lfm_profile_features.pkl: unused by the
            // Python prediction pass — intentionally ignored.
            _ => {}
        }
    }

    let Some(model) = model else {
        if legacy_pickle {
            bail!(
                "model.tar.gz contains the legacy joblib artifact \
                 trained_recommender_model.pkl; the Rust worker requires the \
                 training image to export trained_recommender_model.json \
                 (see artifacts.rs for the schema)"
            );
        }
        bail!("model.tar.gz is missing trained_recommender_model.json");
    };
    let model_meta = model_meta
        .ok_or_else(|| anyhow!("model.tar.gz is missing model_meta_data.json"))?;

    Ok(JobArtifacts { model_meta, model })
}
