use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::ScopedJoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use veritas_domain::CoreError;
use veritas_store::{AuditReport, Store};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::canonical::json::{self, Str, Value};
use crate::scratch::ScratchDir;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportOutcome {
	/// The zip is at the requested path. The audit ran first and found no
	/// damage; anything else it found is here.
	Exported(AuditReport),
	/// SQLite found the working copy damaged, so nothing was written.
	Refused(AuditReport),
}

fn io_error(path: &Path, error: &std::io::Error) -> CoreError {
	CoreError::Io(format!("{}: {error}", path.display()))
}

/// Actual's `exportBuffer` copies metadata.json with `resetClock: true`, so
/// the importing device takes a fresh CRDT node id. Everything else stays.
fn export_metadata(metadata_path: &Path) -> Result<Vec<u8>, CoreError> {
	let bytes = std::fs::read(metadata_path).map_err(|error| io_error(metadata_path, &error))?;
	if bytes.len() as u64 > crate::archive::MAX_METADATA_SIZE {
		return Err(CoreError::Invalid(
			"the budget's metadata.json is larger than 1 MB".to_owned(),
		));
	}
	let mut metadata = json::parse(&String::from_utf8_lossy(&bytes))
		.map_err(|reason| CoreError::Invalid(reason.to_owned()))?;
	let Value::Object(entries) = &mut metadata else {
		return Err(CoreError::Invalid(
			"the budget's metadata.json isn't a JSON object".to_owned(),
		));
	};
	match entries
		.iter_mut()
		.find(|(key, _)| key.eq_ascii("resetClock"))
	{
		Some(entry) => entry.1 = Value::Bool(true),
		None => entries.push((Str::from("resetClock"), Value::Bool(true))),
	}
	let mut out = Vec::new();
	json::stringify(&mut out, &metadata).map_err(|error| CoreError::Internal(error.to_string()))?;
	Ok(out)
}

static NEXT_PARTIAL: AtomicU64 = AtomicU64::new(0);

/// A temp name beside `to` that no other writer, in this process or another,
/// is using: the file is written there and renamed over `to`.
pub(crate) fn partial_path(to: &Path) -> Result<PathBuf, CoreError> {
	let name = to
		.file_name()
		.ok_or_else(|| CoreError::Io(format!("{} has no file name", to.display())))?;
	let nanos = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|elapsed| elapsed.as_nanos())
		.unwrap_or_default();
	Ok(to.with_file_name(format!(
		".{}.{}-{nanos}-{}.partial",
		name.to_string_lossy(),
		std::process::id(),
		NEXT_PARTIAL.fetch_add(1, Ordering::Relaxed)
	)))
}

fn write_zip(path: &Path, db_path: &Path, metadata: &[u8]) -> Result<(), CoreError> {
	let file = File::create_new(path).map_err(|error| io_error(path, &error))?;
	let mut zip = ZipWriter::new(BufWriter::new(file));
	let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
	let zip_error =
		|error: zip::result::ZipError| CoreError::Io(format!("{}: {error}", path.display()));
	zip.start_file("db.sqlite", options).map_err(zip_error)?;
	let mut db = File::open(db_path).map_err(|error| io_error(db_path, &error))?;
	std::io::copy(&mut db, &mut zip).map_err(|error| io_error(path, &error))?;
	zip.start_file("metadata.json", options)
		.map_err(zip_error)?;
	zip.write_all(metadata)
		.map_err(|error| io_error(path, &error))?;
	let writer = zip.finish().map_err(zip_error)?;
	let file = writer
		.into_inner()
		.map_err(|error| io_error(path, error.error()))?;
	file.sync_all().map_err(|error| io_error(path, &error))
}

/// Writes an Actual export of the budget to `to`, shaped like
/// Actual's own: `db.sqlite` and `metadata.json` at the zip root. The zip is
/// written beside `to` while the invariant audit runs, and renamed into place
/// only once the audit finds no damage, so nothing unaudited is published and
/// `to` is either the old file or the complete new one.
pub fn export_actual_zip(
	store: &Store,
	metadata_path: &Path,
	to: &Path,
) -> Result<ExportOutcome, CoreError> {
	export(store, None, metadata_path, to)
}

/// [`export_actual_zip`], cloning `snapshot` instead of vacuuming the database
/// again. `snapshot` must be a `VACUUM INTO` copy of the store's committed
/// state with nothing written since, like the session's Export snapshot.
pub fn export_actual_zip_from_snapshot(
	store: &Store,
	snapshot: &Path,
	metadata_path: &Path,
	to: &Path,
) -> Result<ExportOutcome, CoreError> {
	export(store, Some(snapshot), metadata_path, to)
}

fn export(
	store: &Store,
	snapshot: Option<&Path>,
	metadata_path: &Path,
	to: &Path,
) -> Result<ExportOutcome, CoreError> {
	let db = store.db_path()?;
	let scratch = ScratchDir::new("export")?;
	let partial = partial_path(to)?;
	let (audit, written) = std::thread::scope(|scope| {
		let writer = scope.spawn(|| {
			let metadata = export_metadata(metadata_path)?;
			let db_copy = scratch.path().join("db.sqlite");
			veritas_store::export_copy(&db, snapshot, &db_copy)?;
			write_zip(&partial, &db_copy, &metadata)
		});
		let audit = audit(store, &db);
		(audit, joined(writer))
	});
	let published = match audit {
		Ok(audit) if audit.is_corrupt() => Ok(ExportOutcome::Refused(audit)),
		Ok(audit) => written
			.and_then(|()| std::fs::rename(&partial, to).map_err(|error| io_error(to, &error)))
			.map(|()| ExportOutcome::Exported(audit)),
		Err(error) => Err(error),
	};
	if !matches!(published, Ok(ExportOutcome::Exported(_))) {
		let _ = std::fs::remove_file(&partial);
	}
	published
}

/// [`Store::open_audit`], with SQLite's integrity check on a read-only
/// connection of its own so it runs alongside the load. When the check finds
/// damage, what was loaded is thrown away and only the damage is reported.
fn audit(store: &Store, db: &Path) -> Result<AuditReport, CoreError> {
	std::thread::scope(|scope| {
		let integrity =
			scope.spawn(|| veritas_store::integrity_check(&veritas_store::open_read_only(db)?));
		let loaded = store.load();
		let problems = joined(integrity)?;
		if !problems.is_empty() {
			return Ok(AuditReport::from_integrity(problems));
		}
		Ok(veritas_store::audit(&loaded?))
	})
}

fn joined<T>(thread: ScopedJoinHandle<'_, Result<T, CoreError>>) -> Result<T, CoreError> {
	thread
		.join()
		.unwrap_or_else(|_| Err(CoreError::Internal("an export thread panicked".to_owned())))
}
