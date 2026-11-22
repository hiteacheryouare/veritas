import Foundation
import FoundationModels
import Synchronization
import Testing
@testable import VeritasAI
import VeritasAIBridge

/// The plug-in's side of the bridge: what the app finds once it loads the
/// bundle, the tools it builds from the app's descriptions, and its failures.
struct VeritasAIPluginTests {
	@Test func theAppFindsThePrincipalClassByItsObjectiveCName() throws {
		let principal = try #require(NSClassFromString("VeritasAIPlugin") as? NSObject.Type)
		let entry = try #require(principal.init() as? any AIPluginEntry)
		#expect(AIPluginAvailability(rawValue: entry.availability) != nil)
	}

	@Test func availabilityMapsEveryReason() {
		#expect(VeritasAIPlugin.availability(.available) == .available)
		#expect(VeritasAIPlugin.availability(.unavailable(.deviceNotEligible)) == .deviceNotEligible)
		#expect(VeritasAIPlugin.availability(.unavailable(.appleIntelligenceNotEnabled)) == .appleIntelligenceNotEnabled)
		#expect(VeritasAIPlugin.availability(.unavailable(.modelNotReady)) == .modelNotReady)
	}

	@Test func aToolDescribedByTheAppReadsThroughTheApp() async throws {
		let reader = RecordingReader(reply: "Tax summary for 2026")
		let description: [String: Any] = [
			AIPluginTool.name: "taxSummary",
			AIPluginTool.description: "One calendar year's tax buckets.",
			AIPluginTool.arguments: [
				[AIPluginTool.name: "year", AIPluginTool.description: "Calendar year", AIPluginTool.type: AIPluginTool.integer, AIPluginTool.minimum: 1900, AIPluginTool.maximum: 2200],
				[AIPluginTool.name: "note", AIPluginTool.description: "Anything", AIPluginTool.type: AIPluginTool.text]
			]
		]
		let tool = try #require(ReadingTool(description, reader: SendableReader(reader)))
		#expect(tool.name == "taxSummary")
		#expect(tool.arguments.map(\.name) == ["year", "note"])
		#expect(tool.arguments.first?.integerRange == 1900...2200)
		let text = try await tool.call(arguments: GeneratedContent(properties: ["year": 2026, "note": "hi"]))
		#expect(text == "Tax summary for 2026")
		let call = try #require(reader.calls.first)
		#expect(call.tool == "taxSummary")
		#expect(call.arguments["year"] as? Int == 2026)
		#expect(call.arguments["note"] as? String == "hi")
	}

	@Test func aDescriptionWithoutANameIsLeftOut() {
		#expect(ReadingTool([AIPluginTool.description: "No name"], reader: SendableReader(RecordingReader(reply: ""))) == nil)
	}

	@Test func aReadTheAppRefusesReadsAsAPlainSentence() async throws {
		let reader = RecordingReader(failure: "The disk is full.")
		let tool = try #require(ReadingTool([AIPluginTool.name: "accounts", AIPluginTool.description: "Every account."], reader: SendableReader(reader)))
		do {
			_ = try await tool.call(arguments: GeneratedContent(properties: [:]))
			Issue.record("the read should fail")
		} catch {
			let wrapped = LanguageModelSession.ToolCallError(tool: tool, underlyingError: error)
			#expect(ModelFailure.message(for: wrapped) == "Couldn't read the budget. The disk is full.")
		}
	}
}

final class RecordingReader: NSObject, AIPluginReader, @unchecked Sendable {
	struct Call: @unchecked Sendable {
		let tool: String
		let arguments: [String: Any]
	}

	private let reply: String?
	private let failure: String?
	private let recorded = Mutex<[Call]>([])

	var calls: [Call] { recorded.withLock { $0 } }

	init(reply: String? = nil, failure: String? = nil) {
		self.reply = reply
		self.failure = failure
	}

	func read(tool: String, arguments: [String: Any], reply: @escaping @Sendable (String?, String?) -> Void) {
		let call = Call(tool: tool, arguments: arguments)
		recorded.withLock { $0.append(call) }
		reply(self.reply, failure)
	}
}
