import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// R-AI-1 is read-only. The assistant's code can only see `ReadOnlyBudgetQueries`,
/// so these check that protocol stays reads-only and that nothing in the
/// assistant's sources names a way to write.
struct AssistantReadOnlyTests {
	static let packages = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../..")
		.standardizedFileURL
	static let kitAssistant = packages.appending(path: "VeritasKit/Sources/VeritasKit/Assistant")
	static let uiAssistant = packages.appending(path: "VeritasMacUI/Sources/VeritasMacUI/Assistant")

	static let allowedQueries: Set<String> = [
		"netProfit(first:last:)",
		"balanceSheet(asOf:)",
		"cashFlowStatement(first:last:)",
		"budgetMonth(_:today:)",
		"accounts()",
		"goals(today:)",
		"taxSummary(year:)",
		"customReports()",
		"customReport(_:today:)"
	]

	static let forbidden = ["perform(", "Command", ".change(", "BudgetSession", "WorkspaceModel"]

	@Test func theQueryProtocolDeclaresOnlyTheReads() throws {
		let source = try String(contentsOf: Self.kitAssistant.appending(path: "ReadOnlyBudgetQueries.swift"), encoding: .utf8)
		let body = try #require(source.components(separatedBy: "public protocol ReadOnlyBudgetQueries").last?.components(separatedBy: "\n}").first)
		let requirements = body.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty && !$0.hasPrefix("//") && !$0.hasPrefix(": Sendable") }
		let names = Set(requirements.map(Self.signature))
		#expect(requirements.allSatisfy { $0.hasPrefix("func ") }, "only functions: \(requirements)")
		#expect(names == Self.allowedQueries)
	}

	@Test func noAssistantSourceNamesAWayToWrite() throws {
		let files = try Self.sources(in: Self.kitAssistant) + Self.sources(in: Self.uiAssistant)
		#expect(!files.isEmpty)
		for file in files {
			let text = try String(contentsOf: file, encoding: .utf8)
			for word in Self.forbidden {
				#expect(!text.contains(word), "\(file.lastPathComponent) mentions \(word)")
			}
		}
	}

	/// `func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth` reads as `budgetMonth(_:today:)`.
	static func signature(_ line: String) -> String {
		let declaration = line.dropFirst("func ".count)
		let name = declaration.prefix { $0 != "(" }
		let parameters = declaration.drop { $0 != "(" }.dropFirst().prefix { $0 != ")" }
		let labels = parameters.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces).prefix { $0 != " " && $0 != ":" } }
		return "\(name)(\(labels.map { "\($0):" }.joined()))"
	}

	static func sources(in directory: URL) throws -> [URL] {
		guard FileManager.default.fileExists(atPath: directory.path(percentEncoded: false)) else { return [] }
		return try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil).filter { $0.pathExtension == "swift" }
	}
}
