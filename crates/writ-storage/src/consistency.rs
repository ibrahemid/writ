use std::collections::HashSet;

use writ_core::buffer::document::BufferStatus;

use crate::buffer_store::BufferStore;
use crate::errors::StorageResult;

pub struct ConsistencyReport {
    pub orphan_files: Vec<String>,
    pub missing_files: Vec<String>,
}

pub struct ConsistencyChecker {
    store: BufferStore,
}

impl ConsistencyChecker {
    pub fn new(store: BufferStore) -> Self {
        Self { store }
    }

    pub fn check(&self) -> StorageResult<ConsistencyReport> {
        let mut all_buffers = self.store.list_by_status(BufferStatus::Active)?;
        all_buffers.extend(self.store.list_by_status(BufferStatus::History)?);

        let buffers_dir = self.store.buffers_dir();

        let known_filenames: HashSet<String> =
            all_buffers.iter().map(|b| b.filename.clone()).collect();

        let mut orphan_files = Vec::new();
        if buffers_dir.exists() {
            for entry in std::fs::read_dir(buffers_dir)? {
                let entry = entry?;
                let file_name = entry.file_name().to_string_lossy().into_owned();
                if !known_filenames.contains(&file_name) {
                    orphan_files.push(file_name);
                }
            }
        }

        let mut missing_files = Vec::new();
        for buffer in &all_buffers {
            let file_path = buffers_dir.join(&buffer.filename);
            if !file_path.exists() {
                missing_files.push(buffer.id.clone());
            }
        }

        Ok(ConsistencyReport {
            orphan_files,
            missing_files,
        })
    }
}
