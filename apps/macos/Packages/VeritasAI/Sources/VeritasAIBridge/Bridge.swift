import Foundation

// The app and the AI plug-in each link a copy of this module, so only
// Objective-C protocols and Foundation types cross between them. The runtime
// treats two protocols with one Objective-C name as one protocol.

/// The plug-in's principal class.
@objc(VeritasAIPluginEntry)
public protocol AIPluginEntry: NSObjectProtocol {
	/// An `AIPluginAvailability` raw value.
	var availability: Int { get }
	/// A conversation with the on-device model, which reads the budget only
	/// through `reader` and the tools described with `AIPluginTool`'s keys.
	func conversation(instructions: String, tools: [[String: Any]], reader: any AIPluginReader) -> any AIPluginConversation
}

@objc(VeritasAIPluginConversation)
public protocol AIPluginConversation: NSObjectProtocol {
	/// Sends the answer so far to `partial` as it grows, then calls `finished`
	/// once: with nil when the answer is complete, or with why it failed.
	/// Cancelling the returned progress stops the answer.
	func respond(to question: String, partial: @escaping @Sendable (String) -> Void, finished: @escaping @Sendable (String?) -> Void) -> Progress
	/// Forgets the earlier questions.
	func reset()
}

/// The app's side of the tools.
@objc(VeritasAIPluginReader)
public protocol AIPluginReader: NSObjectProtocol {
	/// Runs the named tool and replies with its text, or with why it couldn't read.
	func read(tool: String, arguments: [String: Any], reply: @escaping @Sendable (String?, String?) -> Void)
}

public enum AIPluginAvailability: Int, Sendable {
	case available
	case deviceNotEligible
	case appleIntelligenceNotEnabled
	case modelNotReady
	case unknown
}

/// The keys and values that describe a tool.
public enum AIPluginTool {
	public static let name = "name"
	public static let description = "description"
	/// Each argument is a dictionary of `name`, `description` and `type`, and
	/// an integer's `minimum` and `maximum`.
	public static let arguments = "arguments"
	public static let type = "type"
	public static let minimum = "minimum"
	public static let maximum = "maximum"
	public static let text = "text"
	public static let integer = "integer"
}
