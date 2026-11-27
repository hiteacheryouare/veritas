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

/// Closing the panel or switching budgets stops an answer mid-stream. The
/// test holds the stream's continuation, so it decides when each word
/// arrives. The stream may notice the cancellation a moment after `stop()`,
/// because its `next()` runs off the main actor, so the tests wait for that
/// moment rather than assume it, and check a word sent in between never
/// lands. Nothing here waits on a clock.
@MainActor
struct AssistantPanelCancelTests {
	private func context() -> AssistantContext? {
		AssistantContext(queries: RecordingQueries(), format: MoneyFormat(), today: "2026-11-18")
	}

	/// Lets the panel's answering task run, on the main actor, until `condition` holds.
	private func letThePanelRun(until condition: () -> Bool) async {
		for _ in 0..<10_000 where !condition() {
			await Task.yield()
		}
	}

	private func refused(_ result: AsyncThrowingStream<String, any Error>.Continuation.YieldResult) -> Bool {
		if case .terminated = result { true } else { false }
	}

	@Test func closingThePanelStopsTheAnswerAndNoMoreTextLands() async throws {
		let answerer = GatedAnswerer()
		let panel = AssistantPanelModel(makeAnswerer: { _ in answerer }, context: context)
		panel.draft = "What changed?"
		panel.ask()
		await letThePanelRun { answerer.continuation != nil }
		let stream = try #require(answerer.continuation)
		stream.yield("Net")
		await letThePanelRun { panel.exchanges.first?.answer == "Net" }
		try #require(panel.exchanges.first?.answer == "Net")

		panel.stop()
		#expect(panel.exchanges.first?.state == .failed(AssistantPanelModel.stopped))
		stream.yield("Net Profit")
		await panel.settle()
		await answerer.waitUntilTerminated()
		#expect(refused(stream.yield("Net Profit for October")), "the model's stream was cancelled, so it refuses any later word")
		#expect(panel.exchanges.first?.answer == "Net", "a word sent before the stream noticed the stop never lands")
		#expect(panel.exchanges.first?.state == .failed(AssistantPanelModel.stopped))
		#expect(!panel.isAnswering)
	}

	/// The model hands the answer loop a word after the panel has stopped,
	/// the case `stop()` promises nothing more lands for. Every word shows at
	/// once, so a word that got past the stop would be on screen here.
	@Test func aWordTheModelHandsOverAfterTheStopNeverLands() async throws {
		let answerer = HandsOverAfterStop()
		let panel = AssistantPanelModel(makeAnswerer: { _ in answerer }, context: context)
		panel.frameInterval = .zero
		answerer.panel = panel
		panel.draft = "What changed?"
		panel.ask()
		await panel.settle()
		#expect(answerer.handedOver == ["Net", "Net Profit"], "the loop was handed a word after the stop")
		#expect(panel.exchanges.first?.answer == "Net")
		#expect(panel.exchanges.first?.state == .failed(AssistantPanelModel.stopped))
	}

	@Test func switchingBudgetsDropsThePanelAndStopsTheAnswer() async throws {
		let answerer = GatedAnswerer()
		var panel: AssistantPanelModel? = AssistantPanelModel(makeAnswerer: { _ in answerer }, context: context)
		panel?.draft = "What changed?"
		panel?.ask()
		await letThePanelRun { answerer.continuation != nil }
		let stream = try #require(answerer.continuation)
		stream.yield("Net")
		await letThePanelRun { panel?.exchanges.first?.answer == "Net" }
		try #require(panel?.exchanges.first?.answer == "Net")

		weak let dropped = panel
		panel = nil
		#expect(dropped == nil, "the old budget's panel is gone")
		await answerer.waitUntilTerminated()
		#expect(refused(stream.yield("Net Profit")), "and dropping it cancelled its model stream")
	}
}

/// A streaming answer redraws the panel at most once a frame, not once per
/// token, and the last words always land. The frame here is an hour long,
/// so nothing depends on how fast the machine is.
@MainActor
struct AssistantPanelCoalescingTests {
	private func letThePanelRun(until condition: () -> Bool) async {
		for _ in 0..<10_000 where !condition() {
			await Task.yield()
		}
	}

	private func panel(_ answerer: GatedAnswerer) -> AssistantPanelModel {
		let panel = AssistantPanelModel(makeAnswerer: { _ in answerer }) {
			AssistantContext(queries: RecordingQueries(), format: MoneyFormat(), today: "2026-11-18")
		}
		panel.frameInterval = .seconds(3_600)
		return panel
	}

	@Test func theFirstWordsShowAtOnceAndLaterOnesWaitForTheFrame() async throws {
		let answerer = GatedAnswerer()
		let panel = panel(answerer)
		panel.draft = "What changed?"
		panel.ask()
		await letThePanelRun { answerer.continuation != nil }
		let stream = try #require(answerer.continuation)
		stream.yield("Spending")
		await letThePanelRun { panel.exchanges.first?.answer == "Spending" }
		#expect(panel.exchanges.first?.answer == "Spending")

		var text = "Spending"
		for word in ["October", "2026:", "$2,489.35."] {
			text += " \(word)"
			stream.yield(text)
		}
		for _ in 0..<1_000 { await Task.yield() }
		#expect(panel.exchanges.first?.answer == "Spending", "held for the next frame")

		stream.finish()
		await panel.settle()
		#expect(panel.exchanges.first?.answer == "Spending October 2026: $2,489.35.", "the last words land when the stream ends")
		#expect(panel.exchanges.first?.state == .answered)
	}

	@Test func stoppingDropsTheHeldWords() async throws {
		let answerer = GatedAnswerer()
		let panel = panel(answerer)
		panel.draft = "What changed?"
		panel.ask()
		await letThePanelRun { answerer.continuation != nil }
		let stream = try #require(answerer.continuation)
		stream.yield("Net")
		await letThePanelRun { panel.exchanges.first?.answer == "Net" }
		stream.yield("Net Profit")
		for _ in 0..<1_000 { await Task.yield() }
		panel.stop()
		for _ in 0..<1_000 { await Task.yield() }
		#expect(panel.exchanges.first?.answer == "Net", "text already shown stays, and nothing held lands")
		#expect(panel.exchanges.first?.state == .failed(AssistantPanelModel.stopped))
	}
}

/// Hands over "Net", then stops the panel inside the model's next step and
/// hands over "Net Profit" anyway, as a model can when the cancellation
/// arrives while it is producing a word.
/// This relies on `AsyncThrowingStream(unfolding:)` still delivering a step
/// that was already running when the cancel arrived. If it ever dropped that
/// word, the test would pass without reaching the answer loop's check.
@MainActor
final class HandsOverAfterStop: AssistantAnswering {
	final class Given: Sendable {
		private let words = Mutex<[String]>([])
		var all: [String] { words.withLock { $0 } }
		func hand(_ word: String) -> String {
			words.withLock { $0.append(word) }
			return word
		}
	}

	weak var panel: AssistantPanelModel?
	private let given = Given()

	var handedOver: [String] { given.all }

	func answer(_ question: String) -> AsyncThrowingStream<String, any Error> {
		let given = given
		let panel = panel
		return AsyncThrowingStream(unfolding: { [weak panel] in
			switch given.all.count {
			case 0:
				return given.hand("Net")
			case 1:
				await MainActor.run { panel?.stop() }
				return given.hand("Net Profit")
			default:
				return nil
			}
		})
	}

	func reset() {}
}

/// Hands the test the stream's continuation instead of producing words itself.
@MainActor
final class GatedAnswerer: AssistantAnswering {
	/// Set when the stream terminates; `wait()` returns once it has.
	final class Ended: Sendable {
		private let state = Mutex<(ended: Bool, waiters: [CheckedContinuation<Void, Never>])>((false, []))
		var value: Bool { state.withLock { $0.ended } }

		func set() {
			let waiters = state.withLock { state in
				state.ended = true
				defer { state.waiters = [] }
				return state.waiters
			}
			waiters.forEach { $0.resume() }
		}

		func wait() async {
			await withCheckedContinuation { continuation in
				let ended = state.withLock { state in
					if !state.ended { state.waiters.append(continuation) }
					return state.ended
				}
				if ended { continuation.resume() }
			}
		}
	}

	private(set) var continuation: AsyncThrowingStream<String, any Error>.Continuation?
	private let ended = Ended()

	var terminated: Bool { ended.value }

	func waitUntilTerminated() async {
		await ended.wait()
	}

	func answer(_ question: String) -> AsyncThrowingStream<String, any Error> {
		let (stream, continuation) = AsyncThrowingStream.makeStream(of: String.self, throwing: (any Error).self)
		let ended = ended
		continuation.onTermination = { _ in ended.set() }
		self.continuation = continuation
		return stream
	}

	func reset() {}
}
