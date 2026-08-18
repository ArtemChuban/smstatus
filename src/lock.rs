use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};

use crate::error::Result;

pub(crate) enum LockOutcome {
    Acquired(Flock<File>),
    AlreadyRunning(Option<i32>),
}

pub(crate) fn lock_dir() -> Result<PathBuf> {
    if let Some(runtime_dir) = dirs::runtime_dir() {
        return Ok(runtime_dir.join("smstatus"));
    }
    let config_dir = dirs::config_dir().ok_or("could not determine config directory")?;
    Ok(config_dir.join("smstatus").join("run"))
}

pub(crate) fn lock_file_path() -> Result<PathBuf> {
    Ok(lock_dir()?.join("smstatus.lock"))
}

pub(crate) fn log_file_path() -> Result<PathBuf> {
    Ok(lock_dir()?.join("smstatus.log"))
}

pub(crate) fn read_pid(file: &mut File) -> Option<i32> {
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    contents.trim().parse().ok()
}

pub(crate) fn acquire_lock() -> Result<LockOutcome> {
    let path = lock_file_path()?;
    std::fs::create_dir_all(path.parent().ok_or("lock file has no parent directory")?)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)?;

    match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(mut flock) => {
            flock.set_len(0)?;
            flock.seek(SeekFrom::Start(0))?;
            write!(flock, "{}", std::process::id())?;
            flock.flush()?;
            Ok(LockOutcome::Acquired(flock))
        }
        Err((mut file, Errno::EWOULDBLOCK)) => Ok(LockOutcome::AlreadyRunning(read_pid(&mut file))),
        Err((_, err)) => Err(format!("failed to lock {}: {err}", path.display()).into()),
    }
}
