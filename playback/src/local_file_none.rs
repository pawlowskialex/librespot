//! Local files need Symphonia to play: without the `symphonia` feature none are looked up.

use std::path::PathBuf;

#[derive(Default)]
pub struct LocalFileLookup;

pub fn create_local_file_lookup(directories: &[PathBuf]) -> LocalFileLookup {
    if !directories.is_empty() {
        warn!("Ignoring the local file directories: local files need the symphonia feature");
    }
    LocalFileLookup
}
