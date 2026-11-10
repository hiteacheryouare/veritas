// swift-tools-version: 6.2
import PackageDescription

// `cargo xtask bindings` generates Sources/VeritasCore/VeritasCore.swift and
// Sources/VeritasCoreFFI/VeritasCoreFFI.h, and puts the core's static library in
// target/swift at the repo root.
let rustLibrary = Context.packageDirectory + "/../../../../target/swift"

let package = Package(
	name: "VeritasKit",
	platforms: [.macOS("27.0")],
	products: [
		.library(name: "VeritasCore", targets: ["VeritasCore"])
	],
	targets: [
		.systemLibrary(name: "VeritasCoreFFI"),
		.target(
			name: "VeritasCore",
			dependencies: ["VeritasCoreFFI"],
			linkerSettings: [.unsafeFlags(["-L", rustLibrary])]
		),
		.testTarget(name: "VeritasCoreTests", dependencies: ["VeritasCore"])
	]
)
