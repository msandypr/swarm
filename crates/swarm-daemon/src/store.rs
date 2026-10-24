use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadRecordStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadRecord {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub output_path: String,
    pub total_size: Option<u64>,
    pub downloaded_bytes: u64,
    pub status: DownloadRecordStatus,
    pub created_at: u64,
    pub completed_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum StateMutation {
    Add { record: DownloadRecord },
    UpdateStatus {
        id: String,
        status: DownloadRecordStatus,
        downloaded_bytes: u64,
        completed_at: Option<u64>,
    },
    Delete { id: String },
}

pub struct StateStore {
    file_path: PathBuf,
    records: HashMap<String, DownloadRecord>,
    writer: File,
    mutation_count: usize,
}

impl StateStore {
    pub fn open_or_create(dir: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let file_path = dir.join("downloads.jsonl");

        let mut records = HashMap::new();
        let mut mutation_count = 0;

        if file_path.exists() {
            let file = File::open(&file_path)?;
            let reader = BufReader::new(file);

            for line in reader.lines() {
                let line_str = match line {
                    Ok(l) => l,
                    Err(_) => continue,
                };
                if line_str.trim().is_empty() {
                    continue;
                }

                if let Ok(mutation) = serde_json::from_str::<StateMutation>(&line_str) {
                    mutation_count += 1;
                    match mutation {
                        StateMutation::Add { record } => {
                            records.insert(record.id.clone(), record);
                        }
                        StateMutation::UpdateStatus {
                            id,
                            status,
                            downloaded_bytes,
                            completed_at,
                        } => {
                            if let Some(record) = records.get_mut(&id) {
                                record.status = status;
                                record.downloaded_bytes = downloaded_bytes;
                                if completed_at.is_some() {
                                    record.completed_at = completed_at;
                                }
                            }
                        }
                        StateMutation::Delete { id } => {
                            records.remove(&id);
                        }
                    }
                }
            }
        }

        let writer = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;

        let mut store = Self {
            file_path,
            records,
            writer,
            mutation_count,
        };

        // Compact if journal has grown significantly larger than live record count
        if store.mutation_count > 200 && store.mutation_count > store.records.len() * 2 {
            store.compact()?;
        }

        Ok(store)
    }

    pub fn append(&mut self, mutation: StateMutation) -> io::Result<()> {
        match &mutation {
            StateMutation::Add { record } => {
                self.records.insert(record.id.clone(), record.clone());
            }
            StateMutation::UpdateStatus {
                id,
                status,
                downloaded_bytes,
                completed_at,
            } => {
                if let Some(record) = self.records.get_mut(id) {
                    record.status = *status;
                    record.downloaded_bytes = *downloaded_bytes;
                    if completed_at.is_some() {
                        record.completed_at = *completed_at;
                    }
                }
            }
            StateMutation::Delete { id } => {
                self.records.remove(id);
            }
        }

        let line = serde_json::to_string(&mutation)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        writeln!(self.writer, "{}", line)?;
        self.writer.flush()?;
        self.mutation_count += 1;

        if self.mutation_count > 500 && self.mutation_count > self.records.len() * 3 {
            self.compact()?;
        }

        Ok(())
    }

    pub fn get_all(&self) -> Vec<DownloadRecord> {
        let mut list: Vec<DownloadRecord> = self.records.values().cloned().collect();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        list
    }

    pub fn get(&self, id: &str) -> Option<&DownloadRecord> {
        self.records.get(id)
    }

    pub fn compact(&mut self) -> io::Result<()> {
        let tmp_path = self.file_path.with_extension("tmp");
        {
            let mut tmp_file = File::create(&tmp_path)?;
            for record in self.records.values() {
                let mutation = StateMutation::Add {
                    record: record.clone(),
                };
                let line = serde_json::to_string(&mutation)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                writeln!(tmp_file, "{}", line)?;
            }
            tmp_file.flush()?;
        }

        std::fs::rename(&tmp_path, &self.file_path)?;

        self.writer = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;
        self.mutation_count = self.records.len();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_replay_and_compaction() {
        let test_dir = std::env::temp_dir().join("swarm_test_store");
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).unwrap();

        let mut store = StateStore::open_or_create(&test_dir).unwrap();
        assert_eq!(store.get_all().len(), 0);

        let record1 = DownloadRecord {
            id: "id_1".to_string(),
            url: "https://example.com/file1.zip".to_string(),
            filename: "file1.zip".to_string(),
            output_path: "C:\\Downloads\\file1.zip".to_string(),
            total_size: Some(1024),
            downloaded_bytes: 0,
            status: DownloadRecordStatus::Downloading,
            created_at: 1000,
            completed_at: None,
        };

        store.append(StateMutation::Add { record: record1 }).unwrap();
        assert_eq!(store.get_all().len(), 1);

        // Update status
        store.append(StateMutation::UpdateStatus {
            id: "id_1".to_string(),
            status: DownloadRecordStatus::Completed,
            downloaded_bytes: 1024,
            completed_at: Some(1050),
        }).unwrap();

        assert_eq!(store.get("id_1").unwrap().status, DownloadRecordStatus::Completed);
        assert_eq!(store.get("id_1").unwrap().downloaded_bytes, 1024);

        // Compact
        store.compact().unwrap();

        // Reopen from disk
        drop(store);
        let reopened = StateStore::open_or_create(&test_dir).unwrap();
        assert_eq!(reopened.get_all().len(), 1);
        let rec = reopened.get("id_1").unwrap();
        assert_eq!(rec.status, DownloadRecordStatus::Completed);
        assert_eq!(rec.downloaded_bytes, 1024);

        let _ = std::fs::remove_dir_all(&test_dir);
    }
}
