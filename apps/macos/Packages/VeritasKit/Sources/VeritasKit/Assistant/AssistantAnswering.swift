import Foundation

/// Answers one question at a time, streaming the answer so far. A
/// conversation keeps its earlier questions until `reset`.
@MainActor
public protocol AssistantAnswering: AnyObject {
	func answer(_ question: String) -> AsyncThrowingStream<String, any Error>
	func reset()
}

/// A failed answer, in words the panel shows as they are.
public struct AssistantFailure: Error, Hashable, LocalizedError {
	public let message: String

	public init(message: String) {
		self.message = message
	}

	public init(_ error: any Error) {
		message = (error as? AssistantFailure)?.message ?? error.localizedDescription
	}

	public var errorDescription: String? { message }
}
