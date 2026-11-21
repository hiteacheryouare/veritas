import Foundation
import FoundationModels

/// Answers one question at a time, streaming the answer so far. A
/// conversation keeps its earlier questions until `reset`.
@MainActor
public protocol AssistantAnswering: AnyObject {
	func answer(_ question: String) -> AsyncThrowingStream<String, any Error>
	func reset()
}

/// Apple's on-device model with the read-only tools. Nothing leaves the Mac.
@MainActor
public final class FoundationModelsAssistant: AssistantAnswering {
	private let context: AssistantContext
	private var session: LanguageModelSession?

	public init(context: AssistantContext) {
		self.context = context
	}

	public func reset() {
		session = nil
	}

	public func answer(_ question: String) -> AsyncThrowingStream<String, any Error> {
		AsyncThrowingStream { continuation in
			let task = Task { @MainActor in
				do {
					try await self.stream(question, into: continuation, retrying: true)
					continuation.finish()
				} catch {
					continuation.finish(throwing: AssistantFailure(error))
				}
			}
			continuation.onTermination = { _ in task.cancel() }
		}
	}

	/// A conversation that outgrows the model's context starts over once with just this question.
	private func stream(_ question: String, into continuation: AsyncThrowingStream<String, any Error>.Continuation, retrying: Bool) async throws {
		let session = self.session ?? makeSession()
		self.session = session
		do {
			for try await snapshot in session.streamResponse(to: question) {
				continuation.yield(snapshot.content)
			}
		} catch let error where retrying && ModelFailure.isContextOverflow(error) {
			self.session = nil
			try await stream(question, into: continuation, retrying: false)
		}
	}

	private func makeSession() -> LanguageModelSession {
		LanguageModelSession(
			model: SystemLanguageModel.default,
			tools: AssistantTools.all(context),
			instructions: AssistantPrompt.instructions(today: context.today)
		)
	}
}

/// A failed answer, in words the panel shows as they are.
public struct AssistantFailure: Error, Hashable, LocalizedError {
	public let message: String

	public init(message: String) {
		self.message = message
	}

	public init(_ error: any Error) {
		if let failure = error as? AssistantFailure {
			self = failure
			return
		}
		message = Self.message(for: error)
	}

	public var errorDescription: String? { message }

	private static func message(for error: any Error) -> String {
		if let message = ModelFailure.message(for: error) { return message }
		if let error = error as? LanguageModelSession.ToolCallError {
			return "Couldn't read the budget. \(ErrorPresentation.coreMessage(error.underlyingError))"
		}
		return error.localizedDescription
	}
}

/// The macOS 27 SDK replaced `LanguageModelSession.GenerationError` with
/// `LanguageModelError` and `SystemLanguageModel.Error`, and Swift 6.4 is the
/// first compiler to ship with it. The swift CI job still builds on the macOS 26 SDK.
private enum ModelFailure {
	static let wouldNotAnswer = "The model wouldn't answer that question."
	static let tooLong = "That question is too long for the model."
	static let unsupportedLanguage = "The model doesn't support this language."
	static let busy = "The model is busy. Ask again in a moment."
	static let timedOut = "The model took too long to answer."
	static let unavailable = "The Apple Intelligence model isn't available right now."

	#if compiler(>=6.4)
	static func isContextOverflow(_ error: any Error) -> Bool {
		guard case .contextSizeExceeded? = error as? LanguageModelError else { return false }
		return true
	}

	static func message(for error: any Error) -> String? {
		switch error {
		case let error as LanguageModelError:
			switch error {
			case .guardrailViolation, .refusal: wouldNotAnswer
			case .contextSizeExceeded: tooLong
			case .unsupportedLanguageOrLocale: unsupportedLanguage
			case .rateLimited: busy
			case .timeout: timedOut
			default: error.localizedDescription
			}
		case is SystemLanguageModel.Error: unavailable
		default: nil
		}
	}
	#else
	static func isContextOverflow(_ error: any Error) -> Bool {
		guard case .exceededContextWindowSize? = error as? LanguageModelSession.GenerationError else { return false }
		return true
	}

	static func message(for error: any Error) -> String? {
		guard let error = error as? LanguageModelSession.GenerationError else { return nil }
		return switch error {
		case .guardrailViolation, .refusal: wouldNotAnswer
		case .exceededContextWindowSize: tooLong
		case .unsupportedLanguageOrLocale: unsupportedLanguage
		case .rateLimited: busy
		case .assetsUnavailable: unavailable
		default: error.localizedDescription
		}
	}
	#endif
}
