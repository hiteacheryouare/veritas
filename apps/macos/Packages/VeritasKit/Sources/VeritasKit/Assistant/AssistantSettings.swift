import Foundation
import Observation

/// The app-wide opt-in for questions answered on this Mac (R-AI-1). It is
/// off until the user turns it on, and reads as off whenever the model can't run.
@MainActor
@Observable
public final class AssistantSettings {
	public static let storageKey = "assistantEnabled"

	/// Nil until the model is first asked about. With the setting off it is
	/// never asked, so the AI plug-in, and FoundationModels with it, never loads (§10 D5).
	public private(set) var availability: AssistantAvailability?
	private var stored: Bool

	@ObservationIgnored private let defaults: UserDefaults
	@ObservationIgnored private let checker: any AssistantAvailabilityChecking
	@ObservationIgnored public let makeAnswerer: @MainActor (AssistantContext) -> any AssistantAnswering

	public init(
		defaults: UserDefaults = .standard,
		availability checker: any AssistantAvailabilityChecking = PluginAvailability(),
		makeAnswerer: @escaping @MainActor (AssistantContext) -> any AssistantAnswering = { PluginAssistant(context: $0) }
	) {
		self.defaults = defaults
		self.checker = checker
		self.makeAnswerer = makeAnswerer
		stored = defaults.bool(forKey: Self.storageKey)
		availability = stored ? checker.availability : nil
	}

	/// Until the model has been asked, turning it on is offered; the attempt asks.
	public var canTurnOn: Bool { availability?.isAvailable ?? true }

	public var isOn: Bool {
		get { stored && availability?.isAvailable == true }
		set {
			if newValue {
				let current = checker.availability
				availability = current
				guard current.isAvailable else { return }
			}
			stored = newValue
			defaults.set(newValue, forKey: Self.storageKey)
		}
	}

	/// The panel's toolbar button shows only while this is true.
	public var offersPanel: Bool { isOn }

	/// The model can finish downloading, or Apple Intelligence be turned on,
	/// while the app runs. Asks again only once the model has been asked before.
	public func refresh() {
		stored = defaults.bool(forKey: Self.storageKey)
		if stored || availability != nil { availability = checker.availability }
	}
}
