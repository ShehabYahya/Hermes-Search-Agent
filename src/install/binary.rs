use std::env;

use crate::{error::SearchError, fsutil::atomic_copy, paths::AppPaths};

pub fn install_current(paths: &AppPaths) -> Result<(), SearchError> {
    let current = env::current_exe()?;
    if canonical_eq(&current, &paths.binary_path) {
        return Ok(());
    }
    atomic_copy(&current, &paths.binary_path, Some(0o755))
}

fn canonical_eq(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
