use writ_core::buffer::document::BufferStatus;

use crate::buffer_store::BufferStore;
use crate::errors::StorageResult;

/// What the startup check found that does not line up.
pub struct ConsistencyReport {
    /// Files still sitting in the retired mirror directory. After the notes
    /// migration this is empty; anything left is text the migration could not
    /// place, kept rather than deleted.
    pub orphan_files: Vec<String>,
    /// Ids of notes whose file is missing: either the row never reached one,
    /// or the path it names is gone.
    pub missing_files: Vec<String>,
}

/// Compares the database against the files the notes live in and reports
/// anything that does not line up.
///
/// This is used at startup to detect corruption from partial writes, manual
/// file edits, or an interrupted migration. The check is read-only; callers
/// decide how to repair.
pub struct ConsistencyChecker<'a> {
    store: &'a BufferStore,
}

impl<'a> ConsistencyChecker<'a> {
    /// Borrows an existing [`BufferStore`] for inspection. Borrowing (rather
    /// than owning) lets the boot path run the check and keep using the store.
    pub fn new(store: &'a BufferStore) -> Self {
        Self { store }
    }

    /// Runs the check and returns a [`ConsistencyReport`].
    pub fn check(&self) -> StorageResult<ConsistencyReport> {
        let mut all_buffers = self.store.list_by_status(BufferStatus::Active)?;
        all_buffers.extend(self.store.list_by_status(BufferStatus::History)?);

        let buffers_dir = self.store.buffers_dir();

        let mut orphan_files = Vec::new();
        if buffers_dir.exists() {
            for entry in std::fs::read_dir(buffers_dir)? {
                let entry = entry?;
                orphan_files.push(entry.file_name().to_string_lossy().into_owned());
            }
        }

        let mut missing_files = Vec::new();
        for buffer in &all_buffers {
            let present = buffer
                .source_path
                .as_deref()
                .is_some_and(|path| std::path::Path::new(path).exists());
            if !present {
                missing_files.push(buffer.id.clone());
            }
        }

        Ok(ConsistencyReport {
            orphan_files,
            missing_files,
        })
    }
}
