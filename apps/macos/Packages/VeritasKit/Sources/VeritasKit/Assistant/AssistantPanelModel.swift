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

	public private(set) var exchanges: [AssistantExchange] = []
	public var draft = ""

	@ObservationIgnored private let context: @MainActor () -> AssistantContext?
	@ObservationIgnored private let makeAnswerer: @MainActor (AssistantContext) -> any AssistantAnswering
	@ObservationIgnored private var answerer: (any AssistantAnswering)?
	@ObservationIgnored private var task: Task<Void, Never>?

	/// `context` is read when the first question is asked, so it uses the budget's format at that point.
	public init(makeAnswerer: @escaping @MainActor (AssistantContext) -> any AssistantAnswering, context: @escaping @MainActor () -> AssistantContext?) {
		self.makeAnswerer = makeAnswerer
		self.context = context
	}

	public var isAnswering: Bool { exchanges.last?.state == .answering }

	private var question: String { draft.trimmingCharacters(in: .whitespacesAndNewlines) }

	public var canAsk: Bool { !isAnswering && !question.isEmpty }

	public func ask() {
		guard canAsk else { return }
		guard let answerer = answerer ?? context().map(makeAnswerer) else { return }
		self.answerer = answerer
		let exchange = AssistantExchange(question: question)
		draft = ""
		exchanges.append(exchange)
		task = Task { [weak self] in
			do {
				for try await partial in answerer.answer(exchange.question) {
					self?.update(exchange.id) { $0.answer = partial }
				}
				self?.update(exchange.id) { $0.state = $0.answer.isEmpty ? .failed(Self.noAnswer) : .answered }
			} catch {
				self?.update(exchange.id) { $0.state = .failed(AssistantFailure(error).message) }
			}
		}
	}

	/// Waits for the answer in progress, for tests and screenshots.
	public func settle() async {
		await task?.value
	}

	public func clear() {
		task?.cancel()
		task = nil
		exchanges = []
		answerer?.reset()
	}

	private func update(_ id: UUID, _ change: (inout AssistantExchange) -> Void) {
		guard let index = exchanges.firstIndex(where: { $0.id == id }) else { return }
		change(&exchanges[index])
	}
}
