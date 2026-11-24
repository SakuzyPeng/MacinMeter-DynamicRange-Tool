//! JSON preparation and file export for the desktop adapter.

use crate::jobs::{JobResult, Wake};
use macinmeter::WireEnvelope;
use std::{
    io::{BufWriter, Write},
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

pub type Pending<T> = Receiver<Result<T, String>>;

pub fn start<T: Send + 'static>(
    wake: Wake,
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<Pending<T>, String> {
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("macinmeter-json".to_owned())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(operation))
                .unwrap_or_else(|_| Err("JSON worker panicked".to_owned()));
            let _ = sender.send(result);
            wake();
        })
        .map_err(|error| error.to_string())?;
    Ok(receiver)
}

pub fn poll<T>(pending: &Pending<T>) -> Option<Result<T, String>> {
    match pending.try_recv() {
        Ok(result) => Some(result),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(Err("JSON worker exited".to_owned())),
    }
}

pub fn envelope(result: &JobResult) -> Result<WireEnvelope, String> {
    match result {
        JobResult::Analysis(report) => Ok(WireEnvelope::analysis(report.as_ref().clone())),
        JobResult::Batch { details, .. } => details
            .report()
            .map(WireEnvelope::batch)
            .map_err(|error| error.to_string()),
        JobResult::Error(error) => Ok(WireEnvelope::error(error.clone())),
    }
}

pub fn write_json(path: &Path, result: &JobResult) -> Result<(), String> {
    let envelope = envelope(result)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        serde_json::to_writer_pretty(&mut writer, &envelope).map_err(|error| error.to_string())?;
        writer.flush().map_err(|error| error.to_string())?;
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temporary.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}
