use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: cargo xtask <command>

commands:
	reference          clone or verify the pinned Actual checkout in reference/actual
	oracle setup       install the oracle's pinned dependencies (npm ci)
	oracle <args...>   run the Actual oracle CLI (dump, apply, generate)
	fixtures           regenerate fixtures/synthetic with the oracle
	template           regenerate the new-budget template db in veritas-store with the oracle
	bindings           build the core and generate its Swift bindings into VeritasKit
	app                build the macOS app into target/xcode
	app-test           run the macOS app's UI tests
	ci                 run every check CI runs, plus the app build
	perf               fail if a core-measurable R-PERF budget is exceeded on perf-100k
	shadow <zip>       compare a real Actual export with Actual, printing only counts";

const FIXTURE_SPECS: [&str; 3] = ["small", "all-features", "perf-100k"];

type Result<T = ()> = std::result::Result<T, String>;

fn main() -> ExitCode {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let outcome = match args.first().map(String::as_str) {
		Some("reference") => reference(),
		Some("oracle") => oracle(&args[1..]),
		Some("fixtures") => fixtures(),
		Some("bindings") => bindings(),
		Some("template") => template(),
		Some("app") => xcodebuild("build"),
		Some("app-test") => xcodebuild("test"),
		Some("ci") => ci(),
		Some("perf") => perf(),
		Some("shadow") => shadow(&args[1..]),
		_ => Err(USAGE.to_owned()),
	};
	match outcome {
		Ok(()) => ExitCode::SUCCESS,
		Err(message) => {
			eprintln!("{message}");
			ExitCode::FAILURE
		}
	}
}

fn root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.map(Path::to_path_buf)
		.unwrap_or_default()
}

fn run(dir: &Path, program: &str, args: &[&str]) -> Result {
	run_with_env(dir, program, args, &[])
}

fn run_with_env(dir: &Path, program: &str, args: &[&str], env: &[(&str, &str)]) -> Result {
	eprintln!("» {program} {}", args.join(" "));
	let status = Command::new(program)
		.args(args)
		.envs(env.iter().copied())
		.current_dir(dir)
		.status()
		.map_err(|e| format!("could not start {program}: {e}"))?;
	if status.success() {
		Ok(())
	} else {
		Err(format!("{program} {} failed ({status})", args.join(" ")))
	}
}

fn output(dir: &Path, program: &str, args: &[&str]) -> Result<String> {
	let out = Command::new(program)
		.args(args)
		.current_dir(dir)
		.output()
		.map_err(|e| format!("could not start {program}: {e}"))?;
	if !out.status.success() {
		return Err(format!(
			"{program} {} failed in {}: {}",
			args.join(" "),
			dir.display(),
			String::from_utf8_lossy(&out.stderr).trim()
		));
	}
	Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

struct Pin {
	tag: String,
	commit: String,
}

fn pin() -> Result<Pin> {
	let dir = root().join("crates/veritas-interop/manifest");
	let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
	let mut manifests: Vec<PathBuf> = entries
		.filter_map(|entry| entry.ok().map(|entry| entry.path()))
		.filter(|path| path.extension().is_some_and(|ext| ext == "json"))
		.collect();
	let [path] = manifests.as_mut_slice() else {
		return Err(format!(
			"expected exactly one manifest in {}, found {}",
			dir.display(),
			manifests.len()
		));
	};
	let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
	let json: serde_json::Value =
		serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
	let field = |name: &str| {
		json[name]
			.as_str()
			.map(str::to_owned)
			.ok_or_else(|| format!("{} has no string field {name}", path.display()))
	};
	Ok(Pin {
		tag: field("actualTag")?,
		commit: field("actualCommit")?,
	})
}

fn reference() -> Result {
	let pin = pin()?;
	let checkout = root().join("reference/actual");
	if !checkout.exists() {
		let parent = root().join("reference");
		std::fs::create_dir_all(&parent).map_err(|e| format!("{}: {e}", parent.display()))?;
		run(
			&parent,
			"git",
			&[
				"-c",
				"advice.detachedHead=false",
				"clone",
				"--quiet",
				"--depth",
				"1",
				"--branch",
				&pin.tag,
				"https://github.com/actualbudget/actual.git",
				"actual",
			],
		)?;
	}
	let head = output(&checkout, "git", &["rev-parse", "HEAD"])?;
	if head != pin.commit {
		return Err(format!(
			"reference/actual is at {head}, but the manifest pins {} ({}). Delete reference/actual and rerun.",
			pin.commit, pin.tag
		));
	}
	eprintln!("reference/actual is at {} ({head})", pin.tag);
	Ok(())
}

fn oracle_dir() -> PathBuf {
	root().join("tools/actual-oracle")
}

fn oracle(args: &[String]) -> Result {
	let dir = oracle_dir();
	if args.first().map(String::as_str) == Some("setup") {
		return run(&dir, "npm", &["ci", "--no-fund", "--no-audit"]);
	}
	if !dir.join("node_modules").exists() {
		return Err("the oracle isn't set up yet: run `cargo xtask oracle setup`".to_owned());
	}
	let mut node_args = vec!["src/main.ts"];
	node_args.extend(args.iter().map(String::as_str));
	run(&dir, "node", &node_args)
}

fn fixtures() -> Result {
	for spec in FIXTURE_SPECS {
		oracle(&["generate".to_owned(), spec.to_owned()])?;
	}
	Ok(())
}

/// Where veritas-store keeps the db of a brand-new Actual budget. The name
/// carries the Actual version, so a bump makes a new file (docs/actual-bump.md).
const TEMPLATE_DB: &str = "crates/veritas-store/template/actual-26.11.0.sqlite";

fn template() -> Result {
	let root = root();
	let zip_path = root.join("target/template/new-budget.zip");
	let _ = std::fs::remove_file(&zip_path);
	oracle(&[
		"generate".to_owned(),
		"template".to_owned(),
		"--out".to_owned(),
		zip_path.to_string_lossy().into_owned(),
	])?;
	let file =
		std::fs::File::open(&zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
	let mut archive =
		zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", zip_path.display()))?;
	let mut db = archive
		.by_name("db.sqlite")
		.map_err(|e| format!("the template zip has no db.sqlite: {e}"))?;
	let target = root.join(TEMPLATE_DB);
	if let Some(parent) = target.parent() {
		std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
	}
	let mut out =
		std::fs::File::create(&target).map_err(|e| format!("{}: {e}", target.display()))?;
	std::io::copy(&mut db, &mut out).map_err(|e| format!("{}: {e}", target.display()))?;
	drop(out);
	let _ = std::fs::remove_file(&zip_path);
	eprintln!("wrote {TEMPLATE_DB}");
	Ok(())
}

const VERITAS_KIT: &str = "apps/macos/Packages/VeritasKit";
// Its tests also render the new-budget screens into target/screenshots.
const VERITAS_MAC_UI: &str = "apps/macos/Packages/VeritasMacUI";
const MACOS_DEPLOYMENT_TARGET: &str = "27.0";
const XCODE_PROJECT: &str = "apps/macos/Veritas.xcodeproj";
const FFI_MODULE_MAP: &str =
	"module VeritasCoreFFI {\n\theader \"VeritasCoreFFI.h\"\n\texport *\n}\n";

fn bindings() -> Result {
	let root = root();
	// The bundled SQLite is C, and cc targets the SDK's own macOS version
	// unless told otherwise, which is newer than the app's minimum.
	run_with_env(
		&root,
		"cargo",
		&["build", "--quiet", "--release", "--package", "veritas-ffi"],
		&[("MACOSX_DEPLOYMENT_TARGET", MACOS_DEPLOYMENT_TARGET)],
	)?;
	let release = root.join("target/release");
	let generated = root.join("target/uniffi/swift");
	let dylib = release.join("libveritas_ffi.dylib");
	run(
		&root,
		"cargo",
		&[
			"run",
			"--quiet",
			"--release",
			"--package",
			"xtask",
			"--bin",
			"uniffi-bindgen",
			"--",
			"generate",
			"--library",
			&dylib.to_string_lossy(),
			"--language",
			"swift",
			"--out-dir",
			&generated.to_string_lossy(),
		],
	)?;

	let kit = root.join(VERITAS_KIT);
	copy(
		&generated.join("VeritasCore.swift"),
		&kit.join("Sources/VeritasCore/VeritasCore.swift"),
	)?;

	// SwiftPM and Xcode both link a binary target's static library without search
	// paths or unsafe flags, so the core travels as an xcframework.
	let headers = root.join("target/swift/include");
	copy(
		&generated.join("VeritasCoreFFI.h"),
		&headers.join("VeritasCoreFFI.h"),
	)?;
	write(&headers.join("module.modulemap"), FFI_MODULE_MAP)?;
	let xcframework = kit.join("VeritasCoreFFI.xcframework");
	if xcframework.exists() {
		std::fs::remove_dir_all(&xcframework)
			.map_err(|e| format!("{}: {e}", xcframework.display()))?;
	}
	run(
		&root,
		"xcodebuild",
		&[
			"-quiet",
			"-create-xcframework",
			"-library",
			&release.join("libveritas_ffi.a").to_string_lossy(),
			"-headers",
			&headers.to_string_lossy(),
			"-output",
			&xcframework.to_string_lossy(),
		],
	)?;
	eprintln!("Swift bindings are in {VERITAS_KIT}");
	Ok(())
}

fn copy(from: &Path, to: &Path) -> Result {
	if let Some(parent) = to.parent() {
		std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
	}
	std::fs::copy(from, to)
		.map(|_| ())
		.map_err(|e| format!("{} → {}: {e}", from.display(), to.display()))
}

fn write(path: &Path, contents: &str) -> Result {
	if let Some(parent) = path.parent() {
		std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
	}
	std::fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))
}

/// Stale when missing, or when any Rust source or the lockfile changed after
/// the xcframework was written.
fn bindings_are_stale(root: &Path) -> bool {
	let built = root
		.join(VERITAS_KIT)
		.join("VeritasCoreFFI.xcframework/Info.plist");
	let Ok(built) = std::fs::metadata(built).and_then(|meta| meta.modified()) else {
		return true;
	};
	[root.join("crates"), root.join("Cargo.lock")]
		.iter()
		.filter_map(|path| newest_change(path))
		.any(|changed| changed > built)
}

fn newest_change(path: &Path) -> Option<std::time::SystemTime> {
	let meta = std::fs::metadata(path).ok()?;
	if !meta.is_dir() {
		return meta.modified().ok();
	}
	std::fs::read_dir(path)
		.ok()?
		.filter_map(|entry| entry.ok())
		.filter_map(|entry| newest_change(&entry.path()))
		.max()
}

fn xcodebuild(action: &str) -> Result {
	let root = root();
	if bindings_are_stale(&root) {
		bindings()?;
	}
	let derived_data = root.join("target/xcode");
	run(
		&root,
		"xcodebuild",
		&[
			"-quiet",
			"-project",
			XCODE_PROJECT,
			"-scheme",
			"Veritas",
			"-destination",
			"platform=macOS",
			"-derivedDataPath",
			&derived_data.to_string_lossy(),
			action,
		],
	)?;
	if action == "build" {
		eprintln!(
			"Veritas.app is in {}",
			derived_data.join("Build/Products/Debug").display()
		);
	}
	Ok(())
}

fn ci() -> Result {
	let root = root();
	run(&root, "cargo", &["fmt", "--all", "--check"])?;
	run(
		&root,
		"cargo",
		&[
			"clippy",
			"--workspace",
			"--all-targets",
			"--",
			"-D",
			"warnings",
		],
	)?;
	run(&root, "cargo", &["test", "--workspace", "--quiet"])?;
	run(&root, "node", &["--test", ".github/scripts/*.test.ts"])?;
	bindings()?;
	run(&root.join(VERITAS_KIT), "swift", &["test", "--quiet"])?;
	run(&root.join(VERITAS_MAC_UI), "swift", &["test", "--quiet"])?;
	xcodebuild("build")?;
	if oracle_dir().join("node_modules").exists() {
		run(&oracle_dir(), "npm", &["test", "--silent"])?;
	} else {
		eprintln!("skipping oracle tests: run `cargo xtask oracle setup` to include them");
	}
	Ok(())
}

/// A scratch folder for the shadow run, readable only by its owner and
/// removed when this value drops, on success, error or panic. An interrupted
/// run's folder is swept by the next one.
struct ShadowScratch(PathBuf);

impl ShadowScratch {
	const PREFIX: &str = "veritas-shadow-";

	fn new() -> Result<ShadowScratch> {
		use std::os::unix::fs::PermissionsExt;
		let path = std::env::temp_dir().join(format!("{}{}", Self::PREFIX, std::process::id()));
		std::fs::create_dir(&path)
			.map_err(|_| "shadow: couldn't make a scratch folder".to_owned())?;
		let scratch = ShadowScratch(path);
		std::fs::set_permissions(&scratch.0, std::fs::Permissions::from_mode(0o700))
			.map_err(|_| "shadow: couldn't lock down the scratch folder".to_owned())?;
		std::fs::create_dir(scratch.0.join("tmp"))
			.map_err(|_| "shadow: couldn't make a scratch folder".to_owned())?;
		Ok(scratch)
	}

	/// Folders left by a run whose process is gone.
	fn sweep() {
		let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
			return;
		};
		for entry in entries.flatten() {
			let name = entry.file_name();
			let Some(pid) = name.to_str().and_then(|n| n.strip_prefix(Self::PREFIX)) else {
				continue;
			};
			let alive = Command::new("kill")
				.args(["-0", pid])
				.stderr(std::process::Stdio::null())
				.status()
				.is_ok_and(|status| status.success());
			if !alive {
				let _ = std::fs::remove_dir_all(entry.path());
			}
		}
	}
}

impl Drop for ShadowScratch {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

/// The shadow run (tools/shadow) on `args[0]`, an Actual export. Every file
/// it makes, its own and the oracle's (through `TMPDIR`), goes in a scratch
/// folder deleted afterwards; it prints counts only, and its stderr, which a
/// crash could fill with budget data, is dropped. The export's path is never
/// echoed either.
fn shadow(args: &[String]) -> Result {
	let [zip] = args else {
		return Err("usage: cargo xtask shadow <path to an Actual export .zip>".to_owned());
	};
	let zip =
		std::fs::canonicalize(zip).map_err(|_| "shadow: the export isn't readable".to_owned())?;
	let root = root();
	run(
		&root,
		"cargo",
		&["build", "--release", "--quiet", "-p", "veritas-shadow"],
	)?;
	ShadowScratch::sweep();
	let scratch = ShadowScratch::new()?;
	let status = Command::new("cargo")
		.args(["run", "--release", "--quiet", "-p", "veritas-shadow", "--"])
		.arg(&zip)
		.env("VERITAS_SHADOW_DIR", &scratch.0)
		.env("TMPDIR", scratch.0.join("tmp"))
		.current_dir(&root)
		.stdin(std::process::Stdio::null())
		.stderr(std::process::Stdio::null())
		.status()
		.map_err(|_| "shadow: couldn't start".to_owned())?;
	drop(scratch);
	if status.success() {
		Ok(())
	} else {
		Err("shadow: not every check passed".to_owned())
	}
}

/// A core-measurable R-PERF budget: criterion benches whose id starts with
/// `prefix`, and the most their median may take.
struct Budget {
	id: &'static str,
	prefix: &'static str,
	most_ms: f64,
}

/// One frame at 120 Hz (R-PERF-3).
const FRAME_MS: f64 = 1000.0 / 120.0;

const BUDGETS: [Budget; 8] = [
	Budget {
		id: "R-PERF-1",
		prefix: "perf-100k/R-PERF-1 open until editable",
		most_ms: 400.0,
	},
	Budget {
		id: "R-PERF-2",
		prefix: "perf-100k/R-PERF-2 budget cell edit until the month reads back",
		most_ms: 16.0,
	},
	Budget {
		id: "R-PERF-2",
		prefix: "R-PERF-2 budget edit in the first month",
		most_ms: 16.0,
	},
	Budget {
		id: "R-PERF-2",
		prefix: "R-PERF-2 budget edit in the current month",
		most_ms: 16.0,
	},
	Budget {
		id: "R-PERF-2",
		prefix: "R-PERF-2 set one envelope to its 3-month average",
		most_ms: 16.0,
	},
	Budget {
		id: "R-PERF-3",
		prefix: "perf-100k/R-PERF-3 scrolling a register",
		most_ms: FRAME_MS,
	},
	Budget {
		id: "R-PERF-5",
		prefix: "perf-100k/R-PERF-5 import an Actual zip",
		most_ms: 3000.0,
	},
	Budget {
		id: "R-PERF-5",
		prefix: "perf-100k/R-PERF-5 export an Actual zip",
		most_ms: 3000.0,
	},
];

/// Reports with nothing cached, classified afresh each time. Shown against
/// R-PERF-4's 100 ms and 500 ms, but the gate is `reports_perf`, which times
/// the app's own path (see docs/acceptance.md, Performance).
const COLD_REPORTS: [(&str, f64); 7] = [
	("perf-100k/net profit, one year", 100.0),
	("perf-100k/balance sheet", 100.0),
	("perf-100k/owner's equity, one year", 100.0),
	("perf-100k/cash flow, one year", 100.0),
	("perf-100k/net worth, every month", 100.0),
	("perf-100k/tax summary, one year", 100.0),
	("perf-100k/sankey, one year", 500.0),
];

/// The perf gate (PLAN §10: every R-PERF budget passes in CI on perf-100k).
/// Criterion runs each bench for many samples after a warm-up, and the gate
/// reads its median, which one slow sample on a shared runner doesn't move.
/// R-PERF-4 is the release-mode reports_perf and dashboard_perf tests, which
/// assert their own budgets. Fails if any median is over its budget. It also
/// runs the formula stack-margin tests in release, the build the app ships,
/// on a 512 KiB stack like Swift's secondary threads.
fn perf() -> Result {
	let root = root();
	let stack = run(
		&root,
		"cargo",
		&[
			"test",
			"--release",
			"-p",
			"veritas-reports",
			"--test",
			"formula",
			"small_stack_",
		],
	);
	// R-PERF-4 (PLAN line 245: "Typical report ≤ 100 ms; year-long Sankey or forecast ≤ 500 ms")
	// asserts its own budgets: each report, each card and the default dashboard within 100 ms,
	// the page with every card within 500 ms. A failure is reported with the benches below.
	let reports = run(
		&root,
		"cargo",
		&[
			"test",
			"--release",
			"--no-fail-fast",
			"-p",
			"veritas-ffi",
			"--test",
			"reports_perf",
			"--test",
			"dashboard_perf",
			"--",
			"--include-ignored",
			"--nocapture",
			"--test-threads=1",
		],
	);
	run(
		&root,
		"cargo",
		&[
			"bench",
			"-p",
			"veritas-session",
			"--bench",
			"open_import_export",
			"--",
			"--noplot",
		],
	)?;
	run(
		&root,
		"cargo",
		&[
			"bench",
			"-p",
			"veritas-engine",
			"--bench",
			"sheet",
			"--",
			"--noplot",
			"R-PERF-2",
		],
	)?;
	run(
		&root,
		"cargo",
		&[
			"bench",
			"-p",
			"veritas-reports",
			"--bench",
			"perf",
			"--",
			"--noplot",
			"perf-100k/",
		],
	)?;

	let target =
		std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
	let medians = criterion_medians(&target.join("criterion"));
	let mut over = Vec::new();
	if stack.is_err() {
		over.push(
			"formula stack margin: a small_stack_ test failed in release (see its output above)"
				.to_owned(),
		);
	}
	if reports.is_err() {
		over.push(
			"R-PERF-4: reports_perf or dashboard_perf is over budget (see its output above)"
				.to_owned(),
		);
	}
	for budget in &BUDGETS {
		let found: Vec<&(String, f64)> = medians
			.iter()
			.filter(|(id, _)| id.starts_with(budget.prefix))
			.collect();
		if found.is_empty() {
			over.push(format!(
				"{}: no bench result for \"{}\"",
				budget.id, budget.prefix
			));
		}
		for (id, ms) in found {
			let verdict = if *ms <= budget.most_ms { "ok" } else { "OVER" };
			eprintln!(
				"{verdict:>4}  {}  {id}: median {ms:.2} ms, budget {:.2} ms",
				budget.id, budget.most_ms
			);
			if *ms > budget.most_ms {
				over.push(format!(
					"{} {id}: {ms:.2} ms > {:.2} ms",
					budget.id, budget.most_ms
				));
			}
		}
	}
	for (prefix, most) in COLD_REPORTS {
		for (id, ms) in medians.iter().filter(|(id, _)| id == prefix) {
			let note = if *ms <= most {
				""
			} else {
				" (over, with nothing cached)"
			};
			eprintln!("cold  R-PERF-4  {id}: median {ms:.2} ms, budget {most:.0} ms{note}");
		}
	}
	if over.is_empty() {
		Ok(())
	} else {
		Err(format!("R-PERF budgets exceeded:\n{}", over.join("\n")))
	}
}

/// Every criterion bench's full id and median in milliseconds, from the
/// `benchmark.json` and `estimates.json` it writes for its newest run.
fn criterion_medians(dir: &Path) -> Vec<(String, f64)> {
	let mut found = Vec::new();
	let Ok(entries) = std::fs::read_dir(dir) else {
		return found;
	};
	for entry in entries.flatten() {
		let path = entry.path();
		if path.is_dir() {
			found.extend(criterion_medians(&path));
			continue;
		}
		if path.file_name().and_then(|n| n.to_str()) != Some("benchmark.json")
			|| path
				.parent()
				.and_then(Path::file_name)
				.and_then(|n| n.to_str())
				!= Some("new")
		{
			continue;
		}
		let read = |file: &Path| -> Option<serde_json::Value> {
			serde_json::from_slice(&std::fs::read(file).ok()?).ok()
		};
		let (Some(benchmark), Some(estimates)) =
			(read(&path), read(&path.with_file_name("estimates.json")))
		else {
			continue;
		};
		if let (Some(id), Some(ns)) = (
			benchmark["full_id"].as_str(),
			estimates["median"]["point_estimate"].as_f64(),
		) {
			found.push((id.to_owned(), ns / 1_000_000.0));
		}
	}
	found
}
