use std::sync::RwLock;

use super::{Install, Version};

/// The yt-dlp release the app runs, `None` until one is downloaded. The updater changes it and
/// every run reads it, so a newer release is used from the next run on.
#[derive(Default)]
pub struct ActiveRelease(RwLock<Option<Install>>);

impl ActiveRelease {
    pub fn new(install: Option<Install>) -> Self {
        Self(RwLock::new(install))
    }

    pub fn get(&self) -> Option<Install> {
        self.0.read().expect("active release lock").clone()
    }

    pub fn version(&self) -> Option<Version> {
        self.get().map(|install| install.version)
    }

    pub(crate) fn set(&self, install: Option<Install>) {
        *self.0.write().expect("active release lock") = install;
    }
}
