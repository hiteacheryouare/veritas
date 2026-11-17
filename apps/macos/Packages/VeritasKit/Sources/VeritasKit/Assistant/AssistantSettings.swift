import Foundation
import Observation

/// The app-wide opt-in for questions answered on this Mac (R-AI-1). It is
/// off until the user turns it on, and reads as off whenever the model can't run.
@MainActor
@Observable
public final class AssistantSettings {
	public static let storageKey = "assistantEnabled"

	public private(set) var availability: AssistantAvailability
	private var stored: Bool

	@ObservationIgnored private let defaults: UserDefaults
	@ObservationIgnored private let checker: any AssistantAvailabilityChecking
	@ObservationIgnored public let makeAnswerer: @MainActor (AssistantContext) -> any AssistantAnswering

	public init(
		defaults: UserDefaults = .standard,
		availability checker: any AssistantAvailabilityChecking = SystemModelAvailability(),
		makeAnswerer: @escaping @MainActor (AssistantContext) -> any AssistantAnswering = { FoundationModelsAssistant(context: $0) }
	) {
		self.defaults = defaults
		self.checker = checker
		self.makeAnswerer = makeAnswerer
		availability = checker.availability
		stored = defaults.bool(forKey: Self.storageKey)
	}

	public var canTurnOn: Bool { availability.isAvailable }

	public var isOn: Bool {
		get { stored && availability.isAvailable }
		set {
			guard availability.isAvailable else { return }
			stored = newValue
			defaults.set(newValue, forKey: Self.storageKey)
		}
	}

	/// The panel's toolbar button shows only while this is true.
	public var offersPanel: Bool { isOn }

	/// The model can finish downloading, or Apple Intelligence be turned on, while the app runs.
	public func refresh() {
		availability = checker.availability
		stored = defaults.bool(forKey: Self.storageKey)
	}
}
