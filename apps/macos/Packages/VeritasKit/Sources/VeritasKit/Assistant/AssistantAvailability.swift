import Foundation
import FoundationModels

/// Whether Apple's on-device model can answer, and why not when it can't.
public enum AssistantAvailability: Hashable, Sendable {
	case available
	case unavailable(Reason)

	public enum Reason: Hashable, Sendable {
		case deviceNotEligible
		case appleIntelligenceNotEnabled
		case modelNotReady
		case unknown
	}

	public var isAvailable: Bool { self == .available }

	/// What Settings says when the model can't be used.
	public var explanation: String? {
		guard case .unavailable(let reason) = self else { return nil }
		return switch reason {
		case .deviceNotEligible: "This Mac doesn't support Apple Intelligence."
		case .appleIntelligenceNotEnabled: "Apple Intelligence is off. Turn it on in System Settings."
		case .modelNotReady: "The Apple Intelligence model hasn't finished downloading."
		case .unknown: "Apple Intelligence isn't available on this Mac."
		}
	}
}

public protocol AssistantAvailabilityChecking: Sendable {
	var availability: AssistantAvailability { get }
}

/// Asks the system's on-device model.
public struct SystemModelAvailability: AssistantAvailabilityChecking {
	public init() {}

	public var availability: AssistantAvailability {
		switch SystemLanguageModel.default.availability {
		case .available: .available
		case .unavailable(.deviceNotEligible): .unavailable(.deviceNotEligible)
		case .unavailable(.appleIntelligenceNotEnabled): .unavailable(.appleIntelligenceNotEnabled)
		case .unavailable(.modelNotReady): .unavailable(.modelNotReady)
		case .unavailable: .unavailable(.unknown)
		}
	}
}

/// A fixed answer, for tests and screenshots.
public struct FixedAvailability: AssistantAvailabilityChecking {
	public let availability: AssistantAvailability

	public init(_ availability: AssistantAvailability) {
		self.availability = availability
	}
}
