// swift-tools-version: 6.2
import PackageDescription

let package = Package(
	name: "VeritasMacUI",
	platforms: [.macOS("27.0")],
	products: [
		.library(name: "VeritasMacUI", targets: ["VeritasMacUI"])
	],
	dependencies: [
		.package(path: "../VeritasKit")
	],
	targets: [
		.target(
			name: "VeritasMacUI",
			dependencies: [
				.product(name: "VeritasKit", package: "VeritasKit"),
				.product(name: "VeritasCore", package: "VeritasKit"),
				.product(name: "VeritasDesign", package: "VeritasKit")
			]
		)
	]
)
