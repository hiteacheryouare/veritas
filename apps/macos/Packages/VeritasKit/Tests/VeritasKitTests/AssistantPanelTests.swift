import Foundation
import Synchronization
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

/// Closing the panel or switching budgets stops a slow answer mid-stream.
@MainActor
struct AssistantPanelCancelTests {
	private func context() -> AssistantContext? {
		AssistantContext(queries: RecordingQueries(), format: MoneyFormat(), today: "2026-11-18")
	}

	private func waitUntil(_ condition: () -> Bool) async {
		for _ in 0..<200 where !condition() {
			try? await Task.sleep(for: .milliseconds(10))
		}
	}

	@Test func closingThePanelStopsTheAnswerAndNoMoreTextLands() async throws {
		let answerer = SlowAnswerer()
		let panel = AssistantPanelModel(makeAnswerer: { _ in answerer }, context: context)
		panel.draft = "What changed?"
		panel.ask()
		await waitUntil { panel.exchanges.first?.answer == "Net" }
		try #require(panel.exchanges.first?.answer == "Net")
		panel.stop()
		await panel.settle()
		await waitUntil { answerer.log.terminated }
		#expect(answerer.log.terminated, "the model's stream is cancelled")
		let yielded = answerer.log.yielded
		try await Task.sleep(for: .milliseconds(150))
		#expect(answerer.log.yielded == yielded, "nothing more is produced")
		#expect(panel.exchanges.first?.answer == "Net", "no more text lands")
		#expect(panel.exchanges.first?.state == .failed(AssistantPanelModel.stopped))
		#expect(!panel.isAnswering)
	}

	@Test func switchingBudgetsDropsThePanelAndStopsTheAnswer() async throws {
		let answerer = SlowAnswerer()
		var panel: AssistantPanelModel? = AssistantPanelModel(makeAnswerer: { _ in answerer }, context: context)
		panel?.draft = "What changed?"
		panel?.ask()
		await waitUntil { panel?.exchanges.first?.answer == "Net" }
		try #require(panel?.exchanges.first?.answer == "Net")
		weak let dropped = panel
		panel = nil
		await waitUntil { answerer.log.terminated }
		#expect(dropped == nil, "the old budget's panel is gone")
		#expect(answerer.log.terminated, "and its model stream is cancelled")
		let yielded = answerer.log.yielded
		try await Task.sleep(for: .milliseconds(150))
		#expect(answerer.log.yielded == yielded)
	}
}

/// Streams one word, then another every 50 ms, recording what it produced and when it was cancelled.
@MainActor
final class SlowAnswerer: AssistantAnswering {
	final class Log: Sendable {
		private let state = Mutex<(yielded: Int, terminated: Bool)>((0, false))
		var yielded: Int { state.withLock { $0.yielded } }
		var terminated: Bool { state.withLock { $0.terminated } }
		func didYield() { state.withLock { $0.yielded += 1 } }
		func didTerminate() { state.withLock { $0.terminated = true } }
	}

	let log = Log()

	func answer(_ question: String) -> AsyncThrowingStream<String, any Error> {
		let log = log
		return AsyncThrowingStream { continuation in
			let producer = Task {
				var text = ""
				for word in ["Net", "Profit", "for", "October", "2026", "was", "412.50."] {
					text = text.isEmpty ? word : "\(text) \(word)"
					continuation.yield(text)
					log.didYield()
					try? await Task.sleep(for: .milliseconds(50))
					if Task.isCancelled { return }
				}
				continuation.finish()
			}
			continuation.onTermination = { _ in
				producer.cancel()
				log.didTerminate()
			}
		}
	}

	func reset() {}
}
