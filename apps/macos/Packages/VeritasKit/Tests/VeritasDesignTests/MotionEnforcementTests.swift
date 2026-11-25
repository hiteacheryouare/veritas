import Foundation
import Testing

/// Screens name a `Motion` and never write their own timing (design/motion.md,
/// enforcement). This reads every Swift file outside VeritasDesign and fails
/// on a hand-rolled animation.
struct MotionEnforcementTests {
	static let packages = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../..")
		.standardizedFileURL

	static let sources = ["VeritasMacUI/Sources", "VeritasKit/Sources/VeritasKit", "../Veritas"]

	/// What a screen may not write. `Motion.perform`, `.motion(_:value:)`,
	/// `.motionTransition(_:)` and the other VeritasDesign modifiers replace them.
	static var forbidden: [(pattern: Regex<Substring>, name: String)] {
		[
			(/withAnimation\s*[({]/, "withAnimation"),
			(/\.animation\(/, ".animation(_:value:)"),
			(/\.spring\(/, ".spring"),
			(/\.(?:bouncy|snappy|smooth|interactiveSpring|interpolatingSpring)\b/, "a system spring"),
			(/\.timingCurve\(/, ".timingCurve"),
			(/\.ease(?:In|Out|InOut)\b/, "an ease curve"),
			(/Animation\.linear\b|\.linear\(duration/, "a linear animation"),
			(/NSAnimationContext/, "NSAnimationContext"),
			(/\.animator\(\)/, ".animator()"),
			(/\.transition\((?!\.identity\))/, ".transition: use .motionTransition"),
			(/\.contentTransition\(\.numericText/, "a digit roll")
		]
	}

	/// Comments aren't code, and a `TimelineView` schedule such as
	/// `TimelineView(.animation)` sets a redraw rate, not a timing.
	static func isScanned(_ line: Substring) -> Bool {
		!line.trimmingCharacters(in: .whitespaces).hasPrefix("//") && !line.contains("TimelineView(")
	}

	@Test func noScreenWritesItsOwnTiming() throws {
		var found: [String] = []
		for folder in Self.sources {
			let root = Self.packages.appending(path: folder).standardizedFileURL
			guard let files = FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil) else { continue }
			for case let file as URL in files where file.pathExtension == "swift" {
				let lines = try String(contentsOf: file, encoding: .utf8).split(separator: "\n", omittingEmptySubsequences: false)
				for (index, line) in lines.enumerated() where Self.isScanned(line) {
					for rule in Self.forbidden where line.contains(rule.pattern) {
						found.append("\(file.lastPathComponent):\(index + 1) uses \(rule.name)")
					}
				}
			}
		}
		#expect(found.isEmpty, "\(found.joined(separator: "\n"))")
	}

	@Test func timelineSchedulesAreNotTimings() {
		#expect(!Self.isScanned("TimelineView(.animation(minimumInterval: 1 / 30)) { timeline in"))
		#expect(Self.isScanned("content.animation(.easeOut, value: x)"))
		#expect(!Self.isScanned("// withAnimation { }"))
	}

	@Test func theScanReadsTheScreens() throws {
		let budget = Self.packages.appending(path: "VeritasMacUI/Sources/VeritasMacUI/Budget/BudgetScreen.swift")
		#expect(FileManager.default.fileExists(atPath: budget.path), "the scan must point at the real sources")
	}
}
