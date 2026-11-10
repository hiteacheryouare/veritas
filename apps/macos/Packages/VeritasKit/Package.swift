// swift-tools-version: 6.2
import PackageDescription

// `cargo xtask bindings` generates Sources/VeritasCore/VeritasCore.swift and
// VeritasCoreFFI.xcframework (the core's static library, its C header and module map).
let package = Package(
	name: "VeritasKit",
	platforms: [.macOS("27.0")],
	products: [
		.library(name: "VeritasCore", targets: ["VeritasCore"])
	],
	targets: [
		.binaryTarget(name: "VeritasCoreFFI", path: "VeritasCoreFFI.xcframework"),
		.target(name: "VeritasCore", dependencies: ["VeritasCoreFFI"]),
		.testTarget(name: "VeritasCoreTests", dependencies: ["VeritasCore"])
	]
)
