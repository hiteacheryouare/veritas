import Foundation
import Synchronization
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

	@Test func everyToolIsBuiltFromTheReadsAloneAndCallsOnlyThem() async throws {
		let queries = RecordingQueries()
		let context = AssistantContext(queries: queries, format: MoneyFormat(), today: "2026-11-17")
		let tools = AssistantTools.all(context)
		#expect(tools.count == 8)
		for tool in tools {
			for child in Self.descendants(of: tool) {
				#expect(!(child is any BudgetSession), "\(tool.name) holds a session")
				#expect(!(child is WorkspaceModel), "\(tool.name) holds a workspace")
				#expect(!(child is Command), "\(tool.name) holds a command")
			}
		}
		_ = try? await NetProfitTool(context: context).call(arguments: .init(first: "2026-05", last: "2026-10"))
		_ = try? await BudgetMonthTool(context: context).call(arguments: .init(month: "2026-10"))
		_ = try? await AccountsTool(context: context).call(arguments: .init())
		_ = try? await BalanceSheetTool(context: context).call(arguments: .init(date: "2026-10-31"))
		_ = try? await CashFlowTool(context: context).call(arguments: .init(first: "2026-10", last: "2026-10"))
		_ = try? await GoalsTool(context: context).call(arguments: .init())
		_ = try? await TaxSummaryTool(context: context).call(arguments: .init(year: 2026))
		_ = try? await SavedReportTool(context: context).call(arguments: .init(name: "Spending"))
		#expect(queries.calls.isSubset(of: Self.allowedQueries))
		#expect(queries.calls == Self.allowedQueries.subtracting(["customReport(_:today:)"]), "customReport runs only once customReports finds the name")
	}

	@Test func aMonthTheToolsCantReadIsAskedForAgainWithoutReading() async throws {
		let queries = RecordingQueries()
		let context = AssistantContext(queries: queries, format: MoneyFormat(), today: "2026-11-17")
		#expect(try await NetProfitTool(context: context).call(arguments: .init(first: "October", last: "2026-10")) == AssistantTools.monthHint)
		#expect(try await BudgetMonthTool(context: context).call(arguments: .init(month: "2026-13")) == AssistantTools.monthHint)
		#expect(try await NetProfitTool(context: context).call(arguments: .init(first: "2020-01", last: "2026-10")).contains("24 months or fewer"))
		#expect(queries.calls.isEmpty)
	}

	static func descendants(of value: Any) -> [Any] {
		Mirror(reflecting: value).children.flatMap { [$0.value] + descendants(of: $0.value) }
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


/// Conforms to `ReadOnlyBudgetQueries` and nothing else, and records each call.
final class RecordingQueries: ReadOnlyBudgetQueries {
	struct Unreachable: Error {}

	private let recorded = Mutex<Set<String>>([])

	var calls: Set<String> { recorded.withLock { $0 } }

	private func record(_ name: String) -> Unreachable {
		recorded.withLock { _ = $0.insert(name) }
		return Unreachable()
	}

	func netProfit(first: Month, last: Month) async throws -> NetProfitReport { throw record("netProfit(first:last:)") }
	func balanceSheet(asOf: String) async throws -> BalanceSheet { throw record("balanceSheet(asOf:)") }
	func cashFlowStatement(first: String, last: String) async throws -> CashFlowColumn { throw record("cashFlowStatement(first:last:)") }
	func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth { throw record("budgetMonth(_:today:)") }
	func accounts() async throws -> AccountList { throw record("accounts()") }
	func goals(today: String) async throws -> [GoalSummary] { throw record("goals(today:)") }
	func taxSummary(year: Int32) async throws -> TaxSummary { throw record("taxSummary(year:)") }
	func customReports() async throws -> [CustomReportItem] { throw record("customReports()") }
	func customReport(_ id: String, today: String) async throws -> CustomReportView { throw record("customReport(_:today:)") }
}
