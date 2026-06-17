use automerge_repo::DocHandle;
use mybriefcase_bookmarks_core::error::CoreError;
use std::path::PathBuf;

pub struct AppState {
    pub doc_handle: DocHandle,
    pub sync_root: PathBuf,
    pub client_id: String,
    pub sse_tx: tokio::sync::broadcast::Sender<()>,
    pub static_version: String,
}

impl AppState {
    fn after_write(&self) -> Result<(), CoreError> {
        crate::repo::export_doc_to_shared(&self.doc_handle, &self.sync_root, &self.client_id)?;
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
