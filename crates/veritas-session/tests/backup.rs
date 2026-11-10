#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use support::TempDir;
use veritas_domain::CoreError;
use veritas_session::write_backup;

const NAME: &str = "Budget-2026-11-09.zip";

#[test]
fn a_backup_lands_whole_under_its_final_name() {
	// R-REL-4
	let folder = TempDir::new("backup-ok");
	let bytes = b"PK\x03\x04 an exported budget".repeat(10_000);

	let written = write_backup(folder.path(), NAME, &bytes).unwrap();

	assert_eq!(written, folder.path().join(NAME));
	assert_eq!(fs::read(&written).unwrap(), bytes);
	assert_eq!(folder.entries(), [NAME]);
}

#[test]
fn a_newer_backup_replaces_the_old_one_atomically() {
	// R-REL-4
	let folder = TempDir::new("backup-replace");
	write_backup(folder.path(), NAME, b"monday").unwrap();
	fs::write(
		folder.path().join(format!(".{NAME}.partial")),
		b"left by a crash",
	)
	.unwrap();

	write_backup(folder.path(), NAME, b"tuesday").unwrap();

	assert_eq!(fs::read(folder.path().join(NAME)).unwrap(), b"tuesday");
	assert_eq!(folder.entries(), [NAME]);
}

#[test]
fn a_missing_folder_is_reported_and_nothing_is_written() {
	// R-REL-4
	let parent = TempDir::new("backup-missing");
	let folder = parent.path().join("OneDrive");

	let result = write_backup(&folder, NAME, b"budget");

	match result {
		Err(CoreError::Io(message)) => assert!(message.contains("isn't available"), "{message}"),
		other => panic!("expected a file error, got {other:?}"),
	}
	assert!(!folder.exists());
	assert!(parent.entries().is_empty());
}

#[test]
fn a_read_only_folder_is_reported_and_nothing_is_written() {
	// R-REL-4
	let folder = TempDir::new("backup-read-only");
	fs::set_permissions(folder.path(), fs::Permissions::from_mode(0o555)).unwrap();

	let result = write_backup(folder.path(), NAME, b"budget");

	fs::set_permissions(folder.path(), fs::Permissions::from_mode(0o755)).unwrap();
	assert!(matches!(result, Err(CoreError::Io(_))), "{result:?}");
	assert!(folder.entries().is_empty());
}

#[test]
fn an_unavailable_location_is_reported() {
	// R-REL-4: a path that runs through a file, as when a cloud folder is
	// replaced by a placeholder.
	let parent = TempDir::new("backup-unavailable");
	let placeholder = parent.path().join("OneDrive");
	fs::write(&placeholder, b"").unwrap();

	for folder in [placeholder.clone(), placeholder.join("Backups")] {
		let result = write_backup(&folder, NAME, b"budget");
		assert!(
			matches!(result, Err(CoreError::Io(_))),
			"{folder:?}: {result:?}"
		);
	}
	assert_eq!(parent.entries(), ["OneDrive"]);
}

#[test]
fn a_failed_rename_leaves_no_partial_file() {
	// R-REL-4: the temp file is written and flushed, then the rename fails.
	let folder = TempDir::new("backup-rename");
	fs::create_dir(folder.path().join(NAME)).unwrap();
	fs::write(folder.path().join(NAME).join("keep"), b"").unwrap();

	let result = write_backup(folder.path(), NAME, b"budget");

	assert!(matches!(result, Err(CoreError::Io(_))), "{result:?}");
	assert_eq!(folder.entries(), [NAME]);
}

#[test]
fn a_file_name_that_escapes_the_folder_is_rejected() {
	let folder = TempDir::new("backup-name");
	for name in ["", "../Budget.zip", "nested/Budget.zip", ".hidden.zip"] {
		let result = write_backup(folder.path(), name, b"budget");
		assert!(
			matches!(result, Err(CoreError::Invalid(_))),
			"{name:?}: {result:?}"
		);
	}
	assert!(folder.entries().is_empty());
}
