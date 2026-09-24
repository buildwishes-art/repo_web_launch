//! Registry repo terinstal, dipersist ke `repos.json`.

use std::{
    fs,
    io::ErrorKind,
    path::PathBuf,
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::{fsutil, validation};
use crate::engine::types::{ScanReport, SourceKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRecord {
    pub id: String,
    pub name: String,
    pub source_kind: SourceKind,
    pub source: String,
    /// Nama folder relatif terhadap workspace (bukan path absolut → portabel & tidak bisa
    /// dimanipulasi untuk menunjuk ke luar workspace).
    pub dir_name: String,
    pub stack: String,
    pub created_at: i64,
    /// Hasil scan keamanan terakhir. `None` untuk repo dari versi lama → dipindai saat Start.
    #[serde(default)]
    pub scan: Option<ScanReport>,
}

pub struct Registry {
    file: PathBuf,
    repos: Vec<RepoRecord>,
}

impl Registry {
    pub fn load(file: PathBuf) -> Result<Self> {
        let repos: Vec<RepoRecord> = match fs::read(&file) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(list) => list,
                Err(_) => {
                    // File rusak: simpan cadangan, mulai dari kosong (repo di disk tetap aman).
                    let _ = fs::rename(&file, file.with_extension("json.corrupt"));
                    Vec::new()
                }
            },
            Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        // Tolak entri yang dir_name-nya dimanipulasi, mis. "../../Windows".
        let repos = repos
            .into_iter()
            .filter(|r| validation::sanitize_repo_name(&r.dir_name).is_ok_and(|s| s == r.dir_name))
            .collect();
        Ok(Self { file, repos })
    }

    pub fn save(&self) -> Result<()> {
        fsutil::write_atomic(&self.file, &serde_json::to_vec_pretty(&self.repos)?)
    }

    pub fn list(&self) -> &[RepoRecord] {
        &self.repos
    }

    pub fn get(&self, id: &str) -> Option<&RepoRecord> {
        self.repos.iter().find(|r| r.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut RepoRecord> {
        self.repos.iter_mut().find(|r| r.id == id)
    }

    pub fn insert(&mut self, record: RepoRecord) {
        self.repos.push(record);
    }

    pub fn remove(&mut self, id: &str) -> Option<RepoRecord> {
        let idx = self.repos.iter().position(|r| r.id == id)?;
        Some(self.repos.remove(idx))
    }
}
