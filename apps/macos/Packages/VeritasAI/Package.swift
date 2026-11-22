// swift-tools-version: 6.2
import PackageDescription

// VeritasAI is the only code that imports FoundationModels. The app never
// links it: Xcode builds it into PlugIns/VeritasAI.bundle, and VeritasKit loads
// that bundle only once the AI setting is on (§10 D5). VeritasAIBridge holds the
// Objective-C protocols both sides speak, and nothing else.
let package = Package(
	name: "VeritasAI",
	platforms: [.macOS("27.0")],
	products: [
		.library(name: "VeritasAIBridge", targets: ["VeritasAIBridge"]),
		.library(name: "VeritasAI", targets: ["VeritasAI"]),
		.library(name: "VeritasAIPlugin", type: .dynamic, targets: ["VeritasAI"])
	],
	targets: [
		.target(name: "VeritasAIBridge"),
		.target(name: "VeritasAI", dependencies: ["VeritasAIBridge"]),
		.testTarget(name: "VeritasAITests", dependencies: ["VeritasAI", "VeritasAIBridge"])
	]
)
