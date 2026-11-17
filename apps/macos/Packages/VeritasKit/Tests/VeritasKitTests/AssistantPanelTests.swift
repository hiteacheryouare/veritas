import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

@MainActor
struct AssistantPanelTests {
	private func panel(_ answerer: ScriptedAnswerer, hasBudget: Bool = true) -> AssistantPanelModel {
		AssistantPanelModel(makeAnswerer: { _ in answerer }) {
			hasBudget ? AssistantContext(queries: RecordingQueries(), format: MoneyFormat(), today: "2026-11-17") : nil
		}
	}

	@Test func aQuestionGetsItsStreamedAnswer() async {
		let answerer = ScriptedAnswerer(parts: ["Net Profit", "Net Profit for October 2026 was 412.50."])
		let panel = panel(answerer)
		#expect(!panel.canAsk)
		panel.draft = "  What was my Net Profit last month?  "
		panel.ask()
		#expect(panel.draft.isEmpty)
		#expect(panel.isAnswering)
		await panel.settle()
		#expect(panel.exchanges == [AssistantExchange(id: panel.exchanges[0].id, question: "What was my Net Profit last month?", answer: "Net Profit for October 2026 was 412.50.", state: .answered)])
		#expect(answerer.questions == ["What was my Net Profit last month?"])
	}

	@Test func aFailureShowsItsMessage() async {
		let panel = panel(ScriptedAnswerer(parts: [], failure: AssistantFailure(message: "The model is busy. Ask again in a moment.")))
		panel.draft = "Spending?"
		panel.ask()
		await panel.settle()
		#expect(panel.exchanges.first?.state == .failed("The model is busy. Ask again in a moment."))
	}

	@Test func anEmptyAnswerSaysSo() async {
		let panel = panel(ScriptedAnswerer(parts: []))
		panel.draft = "Spending?"
		panel.ask()
		await panel.settle()
		#expect(panel.exchanges.first?.state == .failed(AssistantPanelModel.noAnswer))
	}

	@Test func clearForgetsTheConversation() async {
		let answerer = ScriptedAnswerer(parts: ["Yes."])
		let panel = panel(answerer)
		panel.draft = "Anything?"
		panel.ask()
		await panel.settle()
		panel.clear()
		#expect(panel.exchanges.isEmpty)
		#expect(answerer.resets == 1)
	}

	@Test func withNoBudgetOpenNothingIsAsked() {
		let answerer = ScriptedAnswerer(parts: ["Yes."])
		let panel = panel(answerer, hasBudget: false)
		panel.draft = "Anything?"
		panel.ask()
		#expect(panel.exchanges.isEmpty)
		#expect(answerer.questions.isEmpty)
	}
}

@MainActor
final class ScriptedAnswerer: AssistantAnswering {
	let parts: [String]
	let failure: AssistantFailure?
	private(set) var questions: [String] = []
	private(set) var resets = 0

	init(parts: [String], failure: AssistantFailure? = nil) {
		self.parts = parts
		self.failure = failure
	}

	func answer(_ question: String) -> AsyncThrowingStream<String, any Error> {
		questions.append(question)
		let (parts, failure) = (parts, failure)
		return AsyncThrowingStream { continuation in
			for part in parts { continuation.yield(part) }
			continuation.finish(throwing: failure)
		}
	}

	func reset() {
		resets += 1
	}
}
