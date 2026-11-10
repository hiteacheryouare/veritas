use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use veritas_domain::CoreError;
use veritas_store::{AuditReport, Store};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::canonical::json::{self, Str, Value};
use crate::scratch::ScratchDir;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportOutcome {
	/// The zip is at the requested path. The audit ran first and found no
	/// damage; anything else it found is here (R-REL-2).
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

fn partial_path(to: &Path) -> Result<PathBuf, CoreError> {
	let name = to
		.file_name()
		.ok_or_else(|| CoreError::Io(format!("{} has no file name", to.display())))?;
	Ok(to.with_file_name(format!(".{}.partial", name.to_string_lossy())))
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

/// R-IO-2: writes an Actual export of the budget to `to`, shaped like
/// Actual's own: `db.sqlite` and `metadata.json` at the zip root. The zip is
/// written beside `to` and renamed into place, so `to` is either the old file
/// or the complete new one. The invariant audit runs first.
pub fn export_actual_zip(
	store: &Store,
	metadata_path: &Path,
	to: &Path,
) -> Result<ExportOutcome, CoreError> {
	let (_, audit) = store.open_audit()?;
	if audit.is_corrupt() {
		return Ok(ExportOutcome::Refused(audit));
	}
	let metadata = export_metadata(metadata_path)?;
	let scratch = ScratchDir::new("export")?;
	let db_path = scratch.path().join("db.sqlite");
	store.export_copy(&db_path)?;

	let partial = partial_path(to)?;
	let _ = std::fs::remove_file(&partial);
	let written = write_zip(&partial, &db_path, &metadata)
		.and_then(|()| std::fs::rename(&partial, to).map_err(|error| io_error(to, &error)));
	if written.is_err() {
		let _ = std::fs::remove_file(&partial);
	}
	written.map(|()| ExportOutcome::Exported(audit))
}
