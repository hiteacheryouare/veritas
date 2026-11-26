import Foundation
import MachO
import Synchronization
import Testing
import VeritasAIBridge
import VeritasCore
@testable import VeritasKit

/// §10 D5: with AI off, nothing AI-related loads. VeritasKit doesn't link
/// FoundationModels; only PlugIns/VeritasAI.bundle does, and it loads when the
/// setting is on or the user turns it on.
///
/// On macOS 27, starting NSApplication loads GenerativeModels, Vision, CoreML
/// and Espresso in any AppKit process, so those are checked only for not
/// growing. FoundationModels is never loaded by AppKit, so it must be absent.
@MainActor
struct AssistantPluginTests {
	static let aiImages = ["FoundationModels", "GenerativeModels", "TokenGeneration", "Vision", "CoreML", "Espresso", "ANECompiler", "libVeritasAIPlugin", "VeritasAI.bundle"]

	/// The AI frameworks and the plug-in loaded in this process.
	static func loadedAIImages() -> Set<String> {
		Set((0..<_dyld_image_count()).compactMap { _dyld_get_image_name($0).map { String(cString: $0) } }
			.filter { path in aiImages.contains { path.contains("/\($0)") } })
	}

	static func foundationModelsIsLoaded() -> Bool {
		loadedAIImages().contains { $0.contains("/FoundationModels.framework/") }
	}

	/// The plug-in as SwiftPM builds it: `swift build --product VeritasAIPlugin` in Packages/VeritasAI.
	static let builtPlugin: URL? = {
		let package = URL(filePath: #filePath).deletingLastPathComponent().appending(path: "../../../VeritasAI/.build").standardizedFileURL
		let environment = ProcessInfo.processInfo.environment["VERITAS_AI_PLUGIN"].map { URL(filePath: $0) }
		let candidates = [environment, package.appending(path: "out/Products/Debug/libVeritasAIPlugin.dylib"), package.appending(path: "debug/libVeritasAIPlugin.dylib")]
		return candidates.compactMap(\.self).first { FileManager.default.fileExists(atPath: $0.path(percentEncoded: false)) }
	}()

	private func withDefaults(_ body: (UserDefaults) async throws -> Void) async rethrows {
		let suite = "veritas-plugin-\(UUID().uuidString)"
		let defaults = UserDefaults(suiteName: suite)!
		defer { defaults.removePersistentDomain(forName: suite) }
		try await body(defaults)
	}

	/// One test, so the check with AI off always runs before anything loads the plug-in.
	@Test func withAIOffNothingAIRelatedLoadsAndTurningItOnLoadsThePlugin() async throws {
		try await withDefaults { defaults in
			let before = Self.loadedAIImages()
			#expect(!Self.foundationModelsIsLoaded(), "nothing in VeritasKit or its tests links FoundationModels")
			let plugin = AssistantPlugin(url: Self.builtPlugin)
			let settings = AssistantSettings(defaults: defaults, availability: PluginAvailability(plugin: plugin)) { PluginAssistant(context: $0, plugin: plugin) }
			#expect(!settings.offersPanel)
			#expect(settings.canTurnOn)
			settings.refresh()
			let panel = AssistantPanelModel(makeAnswerer: settings.makeAnswerer) { nil }
			panel.draft = "What did I spend?"
			panel.ask()
			#expect(Self.loadedAIImages() == before, "a launch with AI off, Settings and the panel load nothing AI-related")
			#expect(!Self.foundationModelsIsLoaded())

			guard Self.builtPlugin != nil else { return }
			settings.isOn = true
			#expect(Self.foundationModelsIsLoaded(), "turning it on loads the plug-in, and FoundationModels with it")
			#expect(Self.loadedAIImages().contains { $0.contains("libVeritasAIPlugin") })
			#expect(plugin.load() != nil, "the plug-in's principal class answers to the bridge's protocol")
			#expect(settings.availability != nil)
			guard settings.availability == .available else { return }
			#expect(settings.isOn)
			try await RealBudget.with { budget in
				let before = try budget.transactionRows()
				let context = AssistantContext(queries: try #require(budget.workspace.readOnlyQueries), format: budget.workspace.format, today: "2026-11-17")
				var answer = ""
				for try await partial in settings.makeAnswerer(context).answer("What was my Net Profit in October 2026?") {
					answer = partial
				}
				#expect(!answer.isEmpty, "the model answered through the plug-in")
				#expect(try budget.transactionRows() == before, "and changed nothing")
			}
		}
	}

	@Test func withoutThePluginTheModelReadsAsUnavailable() {
		let plugin = AssistantPlugin(url: URL(filePath: "/nonexistent/VeritasAI.bundle"))
		#expect(plugin.load() == nil)
		#expect(PluginAvailability(plugin: plugin).availability == .unavailable(.unknown))
	}

	@Test func theAnswerStreamsAndTheToolsReadThroughTheQueries() async throws {
		let queries = RecordingQueries()
		let context = AssistantContext(queries: queries, format: MoneyFormat(), today: "2026-11-17")
		let fake = FakePlugin()
		let assistant = PluginAssistant(context: context, plugin: AssistantPlugin(loaded: fake))
		var parts: [String] = []
		do {
			for try await partial in assistant.answer("How are my goals?") { parts.append(partial) }
			Issue.record("the fake fails once its tool can't read")
		} catch let failure as AssistantFailure {
			#expect(failure.message == ErrorPresentation.coreMessage(RecordingQueries.Unreachable()), "the reader's failure reaches the panel in the core's words")
		}
		#expect(parts == ["Checking", "Checking goals"])
		#expect(queries.calls == ["goals(today:)"])
		let conversation = try #require(fake.conversations.first)
		#expect(conversation.instructions.contains("Today is 2026-11-17."))
		#expect(conversation.tools.map { $0[AIPluginTool.name] as? String } == AssistantTools.all(context).map(\.name))
		let tax = try #require(conversation.tools.first { $0[AIPluginTool.name] as? String == "taxSummary" })
		let year = try #require((tax[AIPluginTool.arguments] as? [[String: Any]])?.first)
		#expect(year[AIPluginTool.type] as? String == AIPluginTool.text, "the core reads the year's words (AI-6.15)")
		assistant.reset()
		#expect(conversation.resets == 1)
	}

	@Test func hostileArgumentsGetAHintAndReadNothing() async throws {
		let queries = RecordingQueries()
		let context = AssistantContext(queries: queries, format: MoneyFormat(), today: "2026-11-17")
		let reader = ToolReader(AssistantTools.all(context))
		func read(_ tool: String, _ arguments: [String: Any]) async -> String? {
			await withCheckedContinuation { continuation in
				reader.read(tool: tool, arguments: arguments) { text, failure in continuation.resume(returning: text ?? failure) }
			}
		}
		for year: Any in [Int.max, Int.min, 2026, 2026.5, "1899", "2201", "99999", "2027", "2024 to 2025", "so far this year", "October 2026"] {
			#expect(await read("taxSummary", ["year": year]) == AssistantPeriod.yearHint, "\(year)")
		}
		#expect(await read("taxSummary", [:]) == AssistantPeriod.yearHint, "a missing year")
		let periods = [
			"-999-01", "0000-01", "99999-01", "2026-00", "2026-13", "1899-12", "2201-01", "x-y", "2026-02-30", "-999-01-01",
			"2026-10-99999999999999999999", "1899", "next fortnight", String(repeating: "9", count: 4_000), String(repeating: "2026-", count: 1_000)
		]
		for period in periods {
			for tool in ["spending", "income", "netProfit", "cashFlow", "budgetMonth"] {
				#expect(await read(tool, ["period": period]) == AssistantPeriod.hint, "\(tool) \(period.prefix(40))")
			}
			#expect(await read("categorySpending", ["name": "Groceries", "period": period]) == AssistantPeriod.hint)
			#expect(await read("budget", ["name": "Groceries", "period": period]) == AssistantPeriod.hint)
		}
		#expect(await read("categorySpending", ["name": 42, "period": 2026]) == AssistantArguments.nameHint, "numbers where text belongs read as missing")
		for date in ["2026-02-30", "-999-01-01", "2026-10-00", "2026-10-99999999999999999999", "2026-12-31", "soon"] {
			#expect(await read("balanceSheet", ["date": date]) == AssistantPeriod.balanceDateHint, "\(date)")
		}
		#expect(await read("noSuchTool", [:]) == "There's no noSuchTool tool.")
		#expect(queries.calls.isEmpty, "no hostile argument reached a read: \(queries.calls)")
		#expect(try await TaxSummaryTool(context: context).call(arguments: .init(year: "next year")) == AssistantPeriod.yearHint)
		#expect(queries.calls.isEmpty)
	}

	@Test func theReaderPassesOnlyTheArgumentsEachToolTakes() {
		let tool = NetProfitTool(context: AssistantContext(queries: RecordingQueries(), format: MoneyFormat(), today: "2026-11-17"))
		let input = ToolReader.input(["period": "last month", "extra": "ignored", "month": 3], for: tool)
		#expect(input.values == ["period": .text("last month")])
		let tax = TaxSummaryTool(context: AssistantContext(queries: RecordingQueries(), format: MoneyFormat(), today: "2026-11-17"))
		#expect(ToolReader.input(["year": "2026"], for: tax).values == ["year": .text("2026")])
		#expect(ToolReader.input(["year": 2026], for: tax).values.isEmpty, "a value of the wrong kind is left out")
	}
}

/// Stands in for VeritasAI.bundle: one conversation that streams two parts,
/// calls the goals tool, and fails with the reader's message.
final class FakePlugin: NSObject, AIPluginEntry, @unchecked Sendable {
	private(set) var conversations: [FakeConversation] = []

	var availability: Int { AIPluginAvailability.available.rawValue }

	func conversation(instructions: String, tools: [[String: Any]], reader: any AIPluginReader) -> any AIPluginConversation {
		let conversation = FakeConversation(instructions: instructions, tools: tools, reader: reader)
		conversations.append(conversation)
		return conversation
	}
}

final class FakeConversation: NSObject, AIPluginConversation, @unchecked Sendable {
	let instructions: String
	let tools: [[String: Any]]
	let reader: any AIPluginReader
	private(set) var resets = 0

	init(instructions: String, tools: [[String: Any]], reader: any AIPluginReader) {
		self.instructions = instructions
		self.tools = tools
		self.reader = reader
	}

	func respond(to question: String, partial: @escaping @Sendable (String) -> Void, finished: @escaping @Sendable (String?) -> Void) -> Progress {
		partial("Checking")
		partial("Checking goals")
		reader.read(tool: "goals", arguments: [:]) { _, failure in
			finished(failure ?? "the goals tool read nothing")
		}
		return Progress(totalUnitCount: 1)
	}

	func reset() {
		resets += 1
	}
}
