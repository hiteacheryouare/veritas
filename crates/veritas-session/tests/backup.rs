#![allow(
	clippy::unwrap_used,
	clippy::expect_used,
	clippy::panic,
	reason = "integration tests fail by panicking"
)]

mod support;

use std::fs;
use std::io::{ErrorKind, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// A backup whose bytes say which run wrote it and how long it is, so a
/// reader can tell a whole file from a torn one.
fn backup_bytes(run: u8, len: usize) -> Vec<u8> {
	let mut bytes = Vec::with_capacity(len);
	bytes.extend_from_slice(&(len as u64).to_le_bytes());
	bytes.resize(len, run);
	bytes
}

fn is_whole(bytes: &[u8]) -> bool {
	let Some((header, body)) = bytes.split_first_chunk::<8>() else {
		return false;
	};
	u64::from_le_bytes(*header) as usize == bytes.len() && body.iter().all(|&byte| byte == body[0])
}

#[test]
fn a_reader_copying_the_backup_mid_write_always_gets_a_whole_file() {
	// R-REL-4: a sync client or a restore picks up the backup while the next
	// scheduled backup is replacing it. Every read is a whole old or a whole
	// new backup.
	let folder = TempDir::new("backup-reader");
	let target = folder.path().join(NAME);
	write_backup(folder.path(), NAME, &backup_bytes(0, 2 << 20)).unwrap();
	let done = Arc::new(AtomicBool::new(false));

	let reader = {
		let done = Arc::clone(&done);
		let target = target.clone();
		std::thread::spawn(move || {
			let mut reads = 0;
			let mut runs_seen = std::collections::BTreeSet::new();
			while !done.load(Ordering::Acquire) || reads == 0 {
				let mut bytes = Vec::new();
				fs::File::open(&target)
					.and_then(|mut file| file.read_to_end(&mut bytes))
					.unwrap();
				assert!(
					is_whole(&bytes),
					"read a torn backup of {} bytes",
					bytes.len()
				);
				runs_seen.insert(bytes[8]);
				reads += 1;
			}
			runs_seen
		})
	};
	for run in 1..=40 {
		let len = (1 << 20) + usize::from(run) * 65_537;
		write_backup(folder.path(), NAME, &backup_bytes(run, len)).unwrap();
	}
	done.store(true, Ordering::Release);

	let runs_seen = reader.join().unwrap();
	assert!(!runs_seen.is_empty());
	assert!(is_whole(&fs::read(&target).unwrap()));
	assert_eq!(fs::read(&target).unwrap()[8], 40);
	assert_eq!(folder.entries(), [NAME]);
}

#[test]
fn a_folder_that_disappears_during_scheduled_backups_never_holds_a_torn_file() {
	// R-REL-4: the OneDrive folder is unmounted and remounted while backups
	// run. Each run either lands a whole backup or reports the problem, and
	// whatever the folder holds afterwards is whole.
	let parent = TempDir::new("backup-vanish");
	let folder = parent.path().join("OneDrive");
	fs::create_dir(&folder).unwrap();
	let done = Arc::new(AtomicBool::new(false));

	let unmounter = {
		let done = Arc::clone(&done);
		let folder = folder.clone();
		std::thread::spawn(move || {
			let mut toggles = 0;
			while !done.load(Ordering::Acquire) {
				let _ = fs::remove_dir_all(&folder);
				let _ = fs::create_dir(&folder);
				toggles += 1;
			}
			toggles
		})
	};
	let mut landed = 0;
	let mut reported = 0;
	for run in 0..300_u16 {
		let bytes = backup_bytes((run % 250) as u8, 64 << 10);
		match write_backup(&folder, NAME, &bytes) {
			Ok(path) => {
				landed += 1;
				assert_eq!(path, folder.join(NAME));
			}
			Err(CoreError::Io(message)) => {
				reported += 1;
				assert!(!message.is_empty());
			}
			Err(other) => panic!("run {run}: unexpected error {other:?}"),
		}
	}
	done.store(true, Ordering::Release);
	assert!(unmounter.join().unwrap() > 0);

	fs::create_dir_all(&folder).unwrap();
	for name in support::entries(&folder) {
		assert_eq!(name, NAME, "a partial file was left behind");
		assert!(is_whole(&fs::read(folder.join(&name)).unwrap()));
	}
	assert_eq!(landed + reported, 300);

	let after = write_backup(&folder, NAME, &backup_bytes(7, 4096)).unwrap();
	assert_eq!(fs::read(after).unwrap(), backup_bytes(7, 4096));
	assert_eq!(support::entries(&folder), [NAME]);
}

const FULL_DISK_DIR: &str = "VERITAS_FULL_DISK_DIR";

#[test]
#[ignore = "helper process, spawned under a file size limit by the full disk test"]
fn write_a_backup_that_does_not_fit() {
	let Some(dir) = std::env::var_os(FULL_DISK_DIR) else {
		return;
	};
	match write_backup(Path::new(&dir), NAME, &backup_bytes(2, 4 << 20)) {
		Ok(_) => println!("\nbackup-outcome written"),
		Err(CoreError::Io(message)) => println!("\nbackup-outcome failed {message}"),
		Err(other) => println!("\nbackup-outcome unexpected {other:?}"),
	}
}

#[test]
fn a_backup_that_runs_out_of_space_keeps_the_previous_one() {
	// R-REL-4: the disk fills partway through tonight's backup. A file size
	// limit makes the write fail mid-file the way a full disk does (EFBIG
	// rather than ENOSPC, through the same write path).
	let folder = TempDir::new("backup-full");
	let yesterday = backup_bytes(1, 8 << 10);
	write_backup(folder.path(), NAME, &yesterday).unwrap();

	let output = Command::new("/bin/sh")
		.args(["-c", "ulimit -f 256 && trap '' XFSZ && exec \"$0\" \"$@\""])
		.arg(std::env::current_exe().unwrap())
		.args([
			"--exact",
			"write_a_backup_that_does_not_fit",
			"--ignored",
			"--nocapture",
			"--test-threads=1",
		])
		.env(FULL_DISK_DIR, folder.path())
		.stdin(Stdio::null())
		.output()
		.unwrap();
	let stdout = String::from_utf8_lossy(&output.stdout);
	let outcome = stdout
		.lines()
		.find_map(|line| line.strip_prefix("backup-outcome "))
		.unwrap_or_else(|| panic!("the helper never reported: {stdout}"));

	assert!(outcome.starts_with("failed "), "{outcome}");
	assert!(outcome.contains(&format!(".{NAME}.partial")), "{outcome}");
	assert_eq!(fs::read(folder.path().join(NAME)).unwrap(), yesterday);
	assert_eq!(folder.entries(), [NAME]);
	let leftover = fs::metadata(folder.path().join(format!(".{NAME}.partial")));
	assert_eq!(
		leftover.map(|_| ()).unwrap_err().kind(),
		ErrorKind::NotFound
	);
}
