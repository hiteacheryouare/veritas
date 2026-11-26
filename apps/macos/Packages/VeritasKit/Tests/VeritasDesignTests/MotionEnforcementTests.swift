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
			(/withTransaction\s*[({]/, "withTransaction"),
			(/Transaction\(\s*animation:/, "Transaction(animation:)"),
			(/\.animation\s*=(?!=)/, "a transaction's animation set by hand"),
			(/\.animation\(/, ".animation(_:value:)"),
			(/\.spring\(/, ".spring"),
			(/\.(?:bouncy|snappy|smooth|interactiveSpring|interpolatingSpring)\b/, "a system spring"),
			(/\.timingCurve\(/, ".timingCurve"),
			(/\.ease(?:In|Out|InOut)\b/, "an ease curve"),
			(/Animation\.linear\b|\.linear\(duration/, "a linear animation"),
			(/\.phaseAnimator(?![A-Za-z])|(?:^|[^A-Za-z.])PhaseAnimator(?![A-Za-z])/, "a phase animator"),
			(/\.keyframeAnimator(?![A-Za-z])|(?:^|[^A-Za-z.])KeyframeAnimator(?![A-Za-z])/, "a keyframe animator"),
			(/NSAnimationContext/, "NSAnimationContext"),
			(/\.animator\(\)/, ".animator()"),
			(/CATransaction|(?:^|[^A-Za-z])CA[A-Za-z]*Animation(?![A-Za-z])/, "Core Animation"),
			(/\.transition\((?!\.identity\))/, ".transition: use .motionTransition"),
			(/\.contentTransition\(\.numericText/, "a digit roll")
		]
	}

	/// A `TimelineView` schedule such as `TimelineView(.animation)` or
	/// `TimelineView(.animation(minimumInterval: 1 / 30))` sets a redraw rate,
	/// not a timing, so only the schedule is left out of the scan.
	static var schedule: Regex<Substring> {
		/TimelineView\(\s*\.[A-Za-z]+(?:\([^()]*\))?\s*\)/
	}

	/// The part of a line the scan reads: nothing of a comment, and the rest
	/// of a line without its `TimelineView` schedule.
	static func scanned(_ line: Substring) -> Substring {
		guard !line.trimmingCharacters(in: .whitespaces).hasPrefix("//") else { return "" }
		return Substring(line.replacing(schedule, with: "TimelineView()"))
	}

	static func violations(in line: Substring) -> [String] {
		let code = scanned(line)
		return forbidden.filter { code.contains($0.pattern) }.map(\.name)
	}

	/// Every Swift file under a source root, failing when the root is missing
	/// or empty, so a moved folder can't quietly drop out of the scan.
	static func swiftFiles(in folder: String) throws -> [URL] {
		let root = packages.appending(path: folder).standardizedFileURL
		let files = try #require(FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil), "\(folder) doesn't resolve")
		let swift = files.compactMap { $0 as? URL }.filter { $0.pathExtension == "swift" }
		try #require(!swift.isEmpty, "\(folder) has no Swift files")
		return swift
	}

	@Test func noScreenWritesItsOwnTiming() throws {
		var found: [String] = []
		for folder in Self.sources {
			for file in try Self.swiftFiles(in: folder) {
				let lines = try String(contentsOf: file, encoding: .utf8).split(separator: "\n", omittingEmptySubsequences: false)
				for (index, line) in lines.enumerated() {
					found += Self.violations(in: line).map { "\(file.lastPathComponent):\(index + 1) uses \($0)" }
				}
			}
		}
		#expect(found.isEmpty, "\(found.joined(separator: "\n"))")
	}

	@Test func onlyTheTimelineScheduleIsExempt() {
		#expect(Self.violations(in: "TimelineView(.animation) { timeline in").isEmpty)
		#expect(Self.violations(in: "TimelineView(.animation(minimumInterval: 1 / 30)) { timeline in").isEmpty)
		#expect(Self.violations(in: "TimelineView(.periodic(from: .now, by: 1)) { _ in dots.animation(.spring(duration: 1), value: x) }") == [".animation(_:value:)", ".spring"])
		#expect(Self.violations(in: "// withAnimation { }").isEmpty)
	}

	@Test func everyWayToSetATimingIsCaught() {
		let dodges = [
			"withTransaction(Transaction(animation: .default)) { x = 1 }",
			".transaction { $0.animation = .default }",
			".phaseAnimator([0, 1]) { content, phase in content }",
			".keyframeAnimator(initialValue: 0) { content, value in content }",
			"let fade = CABasicAnimation(keyPath: \"opacity\")",
			"CATransaction.begin()",
			"let spring = CASpringAnimation()",
			"CABasicAnimation(keyPath: \"position\")",
			"KeyframeAnimator(initialValue: 0) { _ in }"
		]
		for line in dodges {
			#expect(!Self.violations(in: Substring(line)).isEmpty, "\(line)")
		}
		#expect(Self.violations(in: "if transaction.animation == nil { }").isEmpty, "comparing isn't setting")
	}

	@Test func everySourceRootIsScanned() throws {
		for folder in Self.sources {
			#expect(try !Self.swiftFiles(in: folder).isEmpty)
		}
		let budget = Self.packages.appending(path: "VeritasMacUI/Sources/VeritasMacUI/Budget/BudgetScreen.swift")
		#expect(FileManager.default.fileExists(atPath: budget.path))
	}
}
