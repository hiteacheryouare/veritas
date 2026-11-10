use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: cargo xtask <command>

commands:
	reference          clone or verify the pinned Actual checkout in reference/actual
	oracle setup       install the oracle's pinned dependencies (npm ci)
	oracle <args...>   run the Actual oracle CLI (dump, apply, generate)
	fixtures           regenerate fixtures/synthetic with the oracle
	bindings           build the core and generate its Swift bindings into VeritasKit
	ci                 run every check CI runs";

const FIXTURE_SPECS: [&str; 3] = ["small", "all-features", "perf-100k"];

type Result<T = ()> = std::result::Result<T, String>;

fn main() -> ExitCode {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let outcome = match args.first().map(String::as_str) {
		Some("reference") => reference(),
		Some("oracle") => oracle(&args[1..]),
		Some("fixtures") => fixtures(),
		Some("bindings") => bindings(),
		Some("ci") => ci(),
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
	eprintln!("» {program} {}", args.join(" "));
	let status = Command::new(program)
		.args(args)
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

const VERITAS_KIT: &str = "apps/macos/Packages/VeritasKit";
const FFI_MODULE_MAP: &str =
	"module VeritasCoreFFI {\n\theader \"VeritasCoreFFI.h\"\n\texport *\n}\n";

fn bindings() -> Result {
	let root = root();
	run(
		&root,
		"cargo",
		&["build", "--quiet", "--release", "--package", "veritas-ffi"],
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
	if oracle_dir().join("node_modules").exists() {
		run(&oracle_dir(), "npm", &["test", "--silent"])?;
	} else {
		eprintln!("skipping oracle tests: run `cargo xtask oracle setup` to include them");
	}
	Ok(())
}
