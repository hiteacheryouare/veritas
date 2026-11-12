// swift-tools-version: 6.2
import PackageDescription

// `cargo xtask bindings` generates Sources/VeritasCore/VeritasCore.swift and
// VeritasCoreFFI.xcframework (the core's static library, its C header and module map).
let package = Package(
	name: "VeritasKit",
	platforms: [.macOS("27.0")],
	products: [
		.library(name: "VeritasCore", targets: ["VeritasCore"]),
		.library(name: "VeritasKit", targets: ["VeritasKit"]),
		.library(name: "VeritasDesign", targets: ["VeritasDesign"])
	],
	targets: [
		.binaryTarget(name: "VeritasCoreFFI", path: "VeritasCoreFFI.xcframework"),
		.target(name: "VeritasCore", dependencies: ["VeritasCoreFFI"]),
		.target(name: "VeritasKit", dependencies: ["VeritasCore"]),
		.target(name: "VeritasDesign"),
		.testTarget(name: "VeritasCoreTests", dependencies: ["VeritasCore"]),
		.testTarget(name: "VeritasKitTests", dependencies: ["VeritasKit", "VeritasCore"]),
		.testTarget(name: "VeritasDesignTests", dependencies: ["VeritasDesign"])
	]
)
