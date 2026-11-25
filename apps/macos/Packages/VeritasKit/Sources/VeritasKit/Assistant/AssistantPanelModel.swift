import Foundation
import Observation

public struct AssistantExchange: Identifiable, Hashable, Sendable {
	public enum State: Hashable, Sendable {
		case answering
		case answered
		case failed(String)
	}

	public let id: UUID
	public let question: String
	public var answer: String
	public var state: State

	public init(id: UUID = UUID(), question: String, answer: String = "", state: State = .answering) {
		self.id = id
		self.question = question
		self.answer = answer
		self.state = state
	}
}

/// The questions asked in one window's panel and their answers, newest last.
@MainActor
@Observable
public final class AssistantPanelModel {
	public static let noAnswer = "The model gave no answer."
	public static let stopped = "Stopped."

	public private(set) var exchanges: [AssistantExchange] = []
	public var draft = ""

	@ObservationIgnored private let context: @MainActor () -> AssistantContext?
	@ObservationIgnored private let makeAnswerer: @MainActor (AssistantContext) -> any AssistantAnswering
	@ObservationIgnored private var answerer: (any AssistantAnswering)?
	/// A streaming answer is shown at most this often, so the panel redraws
	/// about 30 times a second rather than once per token.
	@ObservationIgnored public var frameInterval: Duration = .milliseconds(33)
	@ObservationIgnored private var task: Task<Void, Never>?
	@ObservationIgnored private var flush: Task<Void, Never>?
	@ObservationIgnored private var pending: (id: UUID, text: String)?
	@ObservationIgnored private var shownAt: ContinuousClock.Instant?

	/// `context` is read when the first question is asked, so it uses the budget's format at that point.
	public init(makeAnswerer: @escaping @MainActor (AssistantContext) -> any AssistantAnswering, context: @escaping @MainActor () -> AssistantContext?) {
		self.makeAnswerer = makeAnswerer
		self.context = context
	}

	/// A panel dropped with its budget stops the model too.
	deinit {
		task?.cancel()
		flush?.cancel()
	}

	public var isAnswering: Bool { exchanges.last?.state == .answering }

	private var question: String { draft.trimmingCharacters(in: .whitespacesAndNewlines) }

	public var canAsk: Bool { !isAnswering && !question.isEmpty }

	/// Asks `question` at once, as the empty panel's example buttons do.
	public func ask(_ question: String) {
		draft = question
		ask()
	}

	public func ask() {
		guard canAsk else { return }
		guard let answerer = answerer ?? context().map(makeAnswerer) else { return }
		self.answerer = answerer
		let exchange = AssistantExchange(question: question)
		draft = ""
		exchanges.append(exchange)
		shownAt = nil
		task = Task { [weak self] in
			do {
				for try await partial in answerer.answer(exchange.question) {
					guard !Task.isCancelled else { return }
					self?.receive(partial, for: exchange.id)
				}
				guard !Task.isCancelled else { return }
				self?.showPending()
				self?.update(exchange.id) { $0.state = $0.answer.isEmpty ? .failed(Self.noAnswer) : .answered }
			} catch {
				guard !Task.isCancelled else { return }
				self?.showPending()
				self?.update(exchange.id) { $0.state = .failed(AssistantFailure(error).message) }
			}
		}
	}

	/// Shows the answer so far at once if a frame has passed since the last
	/// update, and otherwise holds it for the next frame.
	private func receive(_ text: String, for id: UUID) {
		let now = ContinuousClock.now
		if let shownAt, now - shownAt < frameInterval {
			pending = (id, text)
			guard flush == nil else { return }
			let due = shownAt + frameInterval
			flush = Task { [weak self] in
				try? await Task.sleep(until: due, clock: .continuous)
				guard !Task.isCancelled else { return }
				self?.showPending()
			}
		} else {
			pending = (id, text)
			showPending()
		}
	}

	private func showPending() {
		flush?.cancel()
		flush = nil
		guard let pending else { return }
		self.pending = nil
		shownAt = .now
		update(pending.id) { $0.answer = pending.text }
	}

	/// Waits for the answer in progress, for tests and screenshots.
	public func settle() async {
		await task?.value
	}

	/// Stops the answer in progress, as when the panel closes. Text already shown stays.
	public func stop() {
		task?.cancel()
		task = nil
		dropPending()
		guard let last = exchanges.last, last.state == .answering else { return }
		update(last.id) { $0.state = .failed(Self.stopped) }
	}

	public func clear() {
		task?.cancel()
		task = nil
		dropPending()
		exchanges = []
		answerer?.reset()
	}

	private func dropPending() {
		flush?.cancel()
		flush = nil
		pending = nil
	}

	private func update(_ id: UUID, _ change: (inout AssistantExchange) -> Void) {
		guard let index = exchanges.firstIndex(where: { $0.id == id }) else { return }
		change(&exchanges[index])
	}
}
