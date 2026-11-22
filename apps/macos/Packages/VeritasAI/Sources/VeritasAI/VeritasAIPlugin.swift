import Foundation
import FoundationModels
import Synchronization
import VeritasAIBridge

/// The bundle's principal class. Loading the bundle is the first time
/// anything in FoundationModels loads.
@objc(VeritasAIPlugin)
public final class VeritasAIPlugin: NSObject, AIPluginEntry {
	override public init() {
		super.init()
	}

	public var availability: Int {
		Self.availability(SystemLanguageModel.default.availability).rawValue
	}

	public func conversation(instructions: String, tools: [[String: Any]], reader: any AIPluginReader) -> any AIPluginConversation {
		let reader = SendableReader(reader)
		return ModelConversation(instructions: instructions, tools: tools.compactMap { ReadingTool($0, reader: reader) })
	}

	static func availability(_ availability: SystemLanguageModel.Availability) -> AIPluginAvailability {
		switch availability {
		case .available: .available
		case .unavailable(.deviceNotEligible): .deviceNotEligible
		case .unavailable(.appleIntelligenceNotEnabled): .appleIntelligenceNotEnabled
		case .unavailable(.modelNotReady): .modelNotReady
		case .unavailable: .unknown
		}
	}
}

/// One conversation: it keeps its earlier questions until `reset`.
final class ModelConversation: NSObject, AIPluginConversation, Sendable {
	private let instructions: String
	private let tools: [ReadingTool]
	private let session = Mutex<LanguageModelSession?>(nil)

	init(instructions: String, tools: [ReadingTool]) {
		self.instructions = instructions
		self.tools = tools
	}

	func reset() {
		session.withLock { $0 = nil }
	}

	func respond(to question: String, partial: @escaping @Sendable (String) -> Void, finished: @escaping @Sendable (String?) -> Void) -> Progress {
		let task = Task {
			do {
				try await stream(question, to: partial, retrying: true)
				finished(nil)
			} catch {
				finished(ModelFailure.message(for: error))
			}
		}
		let progress = Progress(totalUnitCount: 1)
		progress.cancellationHandler = { task.cancel() }
		return progress
	}

	/// A conversation that outgrows the model's context starts over once with just this question.
	private func stream(_ question: String, to partial: @Sendable (String) -> Void, retrying: Bool) async throws {
		let current = session.withLock { session in
			let current = session ?? LanguageModelSession(model: .default, tools: tools, instructions: instructions)
			session = current
			return current
		}
		do {
			for try await snapshot in current.streamResponse(to: question) {
				try Task.checkCancellation()
				partial(snapshot.content)
			}
		} catch let error where retrying && ModelFailure.isContextOverflow(error) {
			reset()
			try await stream(question, to: partial, retrying: false)
		}
	}
}

/// The app's reader, which the tools call from any thread.
struct SendableReader: @unchecked Sendable {
	let reader: any AIPluginReader

	init(_ reader: any AIPluginReader) {
		self.reader = reader
	}

	func read(_ tool: String, _ arguments: [String: Any]) async throws -> String {
		try await withCheckedThrowingContinuation { continuation in
			reader.read(tool: tool, arguments: arguments) { text, failure in
				if let failure {
					continuation.resume(throwing: ToolFailure(message: failure))
				} else {
					continuation.resume(returning: text ?? "")
				}
			}
		}
	}
}

/// A tool described by the app: the model fills in its arguments, and the app reads the budget.
struct ReadingTool: Tool {
	struct Argument: Sendable {
		let name: String
		let description: String
		let integerRange: ClosedRange<Int>?
		let isInteger: Bool
	}

	let name: String
	let description: String
	let arguments: [Argument]
	let parameters: GenerationSchema
	let reader: SendableReader

	init?(_ tool: [String: Any], reader: SendableReader) {
		guard let name = tool[AIPluginTool.name] as? String, let description = tool[AIPluginTool.description] as? String else { return nil }
		let arguments = (tool[AIPluginTool.arguments] as? [[String: Any]] ?? []).compactMap { argument -> Argument? in
			guard let name = argument[AIPluginTool.name] as? String, let description = argument[AIPluginTool.description] as? String else { return nil }
			let isInteger = argument[AIPluginTool.type] as? String == AIPluginTool.integer
			let range = (argument[AIPluginTool.minimum] as? Int).flatMap { low in (argument[AIPluginTool.maximum] as? Int).map { low...$0 } }
			return Argument(name: name, description: description, integerRange: range, isInteger: isInteger)
		}
		let properties = arguments.map { argument in
			let schema = if argument.isInteger {
				DynamicGenerationSchema(type: Int.self, guides: argument.integerRange.map { [.range($0)] } ?? [])
			} else {
				DynamicGenerationSchema(type: String.self)
			}
			return DynamicGenerationSchema.Property(name: argument.name, description: argument.description, schema: schema)
		}
		guard let parameters = try? GenerationSchema(root: DynamicGenerationSchema(name: "\(name)Arguments", properties: properties), dependencies: []) else { return nil }
		self.name = name
		self.description = description
		self.arguments = arguments
		self.parameters = parameters
		self.reader = reader
	}

	func call(arguments content: GeneratedContent) async throws -> String {
		var values: [String: Any] = [:]
		for argument in arguments {
			values[argument.name] = argument.isInteger
				? try content.value(Int.self, forProperty: argument.name)
				: try content.value(String.self, forProperty: argument.name)
		}
		return try await reader.read(name, values)
	}
}

/// Why the app's reader couldn't answer a tool, in its own words.
struct ToolFailure: Error {
	let message: String
}

/// The model's failures, in words the panel shows as they are.
///
/// The macOS 27 SDK replaced `LanguageModelSession.GenerationError` with
/// `LanguageModelError` and `SystemLanguageModel.Error`, and Swift 6.4 is the
/// first compiler to ship with it. The swift CI job still builds on the macOS 26 SDK.
enum ModelFailure {
	static let wouldNotAnswer = "The model wouldn't answer that question."
	static let tooLong = "That question is too long for the model."
	static let unsupportedLanguage = "The model doesn't support this language."
	static let busy = "The model is busy. Ask again in a moment."
	static let timedOut = "The model took too long to answer."
	static let unavailable = "The Apple Intelligence model isn't available right now."

	static func message(for error: any Error) -> String {
		if let error = error as? LanguageModelSession.ToolCallError {
			let reason = (error.underlyingError as? ToolFailure)?.message ?? error.underlyingError.localizedDescription
			return "Couldn't read the budget. \(reason)"
		}
		return modelMessage(for: error) ?? error.localizedDescription
	}

	#if compiler(>=6.4)
	static func isContextOverflow(_ error: any Error) -> Bool {
		guard case .contextSizeExceeded? = error as? LanguageModelError else { return false }
		return true
	}

	static func modelMessage(for error: any Error) -> String? {
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

	static func modelMessage(for error: any Error) -> String? {
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
