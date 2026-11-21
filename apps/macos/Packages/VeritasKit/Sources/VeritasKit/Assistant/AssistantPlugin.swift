import Foundation
import Synchronization
import VeritasAIBridge

/// The AI plug-in, PlugIns/VeritasAI.bundle. It is the only code that links
/// FoundationModels, and it loads the first time `load()` is called, which is
/// only once the AI setting is on or the user turns it on (§10 D5).
public final class AssistantPlugin: Sendable {
	public static let shared = AssistantPlugin(url: Bundle.main.builtInPlugInsURL?.appending(path: "VeritasAI.bundle"))

	/// A bundle, or a dynamic library such as SwiftPM's `libVeritasAIPlugin.dylib`.
	public let url: URL?
	private let entry: Mutex<Loaded?>

	private struct Loaded: @unchecked Sendable {
		let entry: (any AIPluginEntry)?
	}

	public init(url: URL?) {
		self.url = url
		entry = Mutex(nil)
	}

	/// A plug-in already in this process, for tests.
	init(loaded: any AIPluginEntry) {
		url = nil
		entry = Mutex(Loaded(entry: loaded))
	}

	/// Nil when the app was built without the plug-in or it wouldn't load.
	public func load() -> (any AIPluginEntry)? {
		entry.withLock { loaded in
			if let loaded { return loaded.entry }
			let made = url.flatMap(Self.principal).flatMap { ($0 as? NSObject.Type)?.init() as? any AIPluginEntry }
			loaded = Loaded(entry: made)
			return made
		}
	}

	private static func principal(at url: URL) -> AnyClass? {
		if url.pathExtension == "dylib" {
			guard dlopen(url.path(percentEncoded: false), RTLD_NOW | RTLD_LOCAL) != nil else { return nil }
			return NSClassFromString("VeritasAIPlugin")
		}
		guard let bundle = Bundle(url: url), bundle.load() else { return nil }
		return bundle.principalClass
	}
}

/// Asks the on-device model through the plug-in, loading it the first time.
public struct PluginAvailability: AssistantAvailabilityChecking {
	private let plugin: AssistantPlugin

	public init(plugin: AssistantPlugin = .shared) {
		self.plugin = plugin
	}

	public var availability: AssistantAvailability {
		guard let entry = plugin.load() else { return .unavailable(.unknown) }
		return switch AIPluginAvailability(rawValue: entry.availability) {
		case .available: .available
		case .deviceNotEligible: .unavailable(.deviceNotEligible)
		case .appleIntelligenceNotEnabled: .unavailable(.appleIntelligenceNotEnabled)
		case .modelNotReady: .unavailable(.modelNotReady)
		case .unknown, nil: .unavailable(.unknown)
		}
	}
}

/// The on-device model through the plug-in, reading the budget only with the
/// read-only tools. Nothing leaves the Mac.
@MainActor
public final class PluginAssistant: AssistantAnswering {
	private let context: AssistantContext
	private let plugin: AssistantPlugin
	private var conversation: Conversation?

	private struct Conversation: @unchecked Sendable {
		let model: any AIPluginConversation
	}

	public init(context: AssistantContext, plugin: AssistantPlugin = .shared) {
		self.context = context
		self.plugin = plugin
	}

	public func reset() {
		conversation?.model.reset()
	}

	public func answer(_ question: String) -> AsyncThrowingStream<String, any Error> {
		let conversation = conversation ?? makeConversation()
		self.conversation = conversation
		return AsyncThrowingStream { continuation in
			guard let conversation else {
				continuation.finish(throwing: AssistantFailure(message: Self.unavailable))
				return
			}
			let progress = conversation.model.respond(to: question) { partial in
				continuation.yield(partial)
			} finished: { failure in
				continuation.finish(throwing: failure.map { AssistantFailure(message: $0) })
			}
			continuation.onTermination = { _ in progress.cancel() }
		}
	}

	static let unavailable = "The Apple Intelligence model isn't available right now."

	private func makeConversation() -> Conversation? {
		guard let entry = plugin.load() else { return nil }
		let tools = AssistantTools.all(context)
		let model = entry.conversation(
			instructions: AssistantPrompt.instructions(today: context.today),
			tools: tools.map(Self.description),
			reader: ToolReader(tools)
		)
		return Conversation(model: model)
	}

	static func description(_ tool: any AssistantTool) -> [String: Any] {
		[
			AIPluginTool.name: tool.name,
			AIPluginTool.description: tool.description,
			AIPluginTool.arguments: tool.arguments.map { argument -> [String: Any] in
				var described: [String: Any] = [AIPluginTool.name: argument.name, AIPluginTool.description: argument.description]
				switch argument.kind {
				case .text:
					described[AIPluginTool.type] = AIPluginTool.text
				case .integer(let range):
					described[AIPluginTool.type] = AIPluginTool.integer
					described[AIPluginTool.minimum] = range.lowerBound
					described[AIPluginTool.maximum] = range.upperBound
				}
				return described
			}
		]
	}
}

/// Runs a tool the model asked for, by name, with the arguments it filled in.
final class ToolReader: NSObject, AIPluginReader, Sendable {
	private let tools: [String: any AssistantTool]

	init(_ tools: [any AssistantTool]) {
		self.tools = Dictionary(tools.map { ($0.name, $0) }) { first, _ in first }
	}

	func read(tool name: String, arguments: [String: Any], reply: @escaping @Sendable (String?, String?) -> Void) {
		guard let tool = tools[name] else {
			reply(nil, "There's no \(name) tool.")
			return
		}
		let input = Self.input(arguments, for: tool)
		Task {
			do {
				reply(try await tool.run(input), nil)
			} catch {
				reply(nil, ErrorPresentation.coreMessage(error))
			}
		}
	}

	static func input(_ arguments: [String: Any], for tool: any AssistantTool) -> AssistantToolInput {
		var values: [String: AssistantToolInput.Value] = [:]
		for argument in tool.arguments {
			switch argument.kind {
			case .text: values[argument.name] = (arguments[argument.name] as? String).map { .text($0) }
			case .integer: values[argument.name] = (arguments[argument.name] as? Int).map { .integer($0) }
			}
		}
		return AssistantToolInput(values)
	}
}
