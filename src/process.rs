use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{error::SearchError, paths::AppPaths};

#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl ProcessOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

pub fn resolve_command(name: &str, paths: &AppPaths) -> Option<PathBuf> {
    which::which(name).ok().or_else(|| {
        let candidate = paths.home.join(".local/bin").join(name);
        candidate.is_file().then_some(candidate)
    })
}

pub fn run_capture<I, S>(program: &Path, args: I) -> Result<ProcessOutput, SearchError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program).args(args).output()?;
    Ok(ProcessOutput {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

pub fn run_checked<I, S>(program: &Path, args: I) -> Result<ProcessOutput, SearchError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_capture(program, args)?;
    if output.success() {
        return Ok(output);
    }
    Err(SearchError::Command {
        program: program.display().to_string(),
        code: output.code,
        stderr: output.stderr,
    })
}

pub fn run_inherit<I, S>(program: &Path, args: I) -> Result<(), SearchError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = Command::new(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if status.success() {
        return Ok(());
    }
    Err(SearchError::Command {
        program: program.display().to_string(),
        code: status.code(),
        stderr: "interactive command failed".to_string(),
    })
}
