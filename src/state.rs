use automerge_repo::DocHandle;
use mybriefcase_bookmarks_core::error::CoreError;
use mybriefcase_bookmarks_core::repo::DebouncedExporter;
use std::path::PathBuf;

pub struct AppState {
    pub doc_handle: DocHandle,
    pub sync_root: PathBuf,
    pub client_id: String,
    pub sse_tx: tokio::sync::broadcast::Sender<()>,
    pub static_version: String,
    pub exporter: DebouncedExporter,
}

impl AppState {
    fn after_write(&self) -> Result<(), CoreError> {
        self.exporter.export_now(
            &self.doc_handle,
            std::time::Instant::now(),
            std::time::SystemTime::now(),
        )?;
        let _ = self.sse_tx.send(());
        Ok(())
    }

    /// # Errors
    /// Returns the error from `f` if the mutation fails, or the export error if export fails.
    pub fn mutate<T>(
        &self,
        f: impl FnOnce(&DocHandle) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let result = f(&self.doc_handle)?;
        self.after_write()?;
        Ok(result)
    }
}
