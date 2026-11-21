import Foundation
import FoundationModels
import Synchronization
import Testing
import VeritasCore
@testable import VeritasKit

@MainActor
struct AssistantSettingsTests {
	private func withDefaults(_ body: (UserDefaults) throws -> Void) rethrows {
		let suite = "veritas-assistant-\(UUID().uuidString)"
		let defaults = UserDefaults(suiteName: suite)!
		defer { defaults.removePersistentDomain(forName: suite) }
		try body(defaults)
	}

	@Test func itIsOffUntilTurnedOn() {
		withDefaults { defaults in
			let settings = AssistantSettings(defaults: defaults, availability: FixedAvailability(.available))
			#expect(!settings.isOn)
			#expect(!settings.offersPanel)
			#expect(settings.canTurnOn)
			#expect(settings.availability == nil, "off, so the model isn't asked")
			settings.isOn = true
			#expect(settings.offersPanel)
			#expect(defaults.bool(forKey: AssistantSettings.storageKey))
			#expect(AssistantSettings(defaults: defaults, availability: FixedAvailability(.available)).isOn, "app-wide, so a new window reads it")
		}
	}

	@Test(arguments: [
		(AssistantAvailability.Reason.deviceNotEligible, "This Mac doesn't support Apple Intelligence."),
		(.appleIntelligenceNotEnabled, "Apple Intelligence is off. Turn it on in System Settings."),
		(.modelNotReady, "The Apple Intelligence model hasn't finished downloading."),
		(.unknown, "Apple Intelligence isn't available on this Mac.")
	])
	func withoutTheModelItReadsOffCantTurnOnAndSaysWhy(reason: AssistantAvailability.Reason, explanation: String) {
		withDefaults { defaults in
			defaults.set(true, forKey: AssistantSettings.storageKey)
			let settings = AssistantSettings(defaults: defaults, availability: FixedAvailability(.unavailable(reason)))
			#expect(!settings.isOn, "a choice made earlier doesn't count while the model can't run")
			#expect(!settings.canTurnOn)
			settings.isOn = true
			#expect(!settings.isOn)
			#expect(!settings.offersPanel)
			#expect(settings.availability?.explanation == explanation)
		}
	}

	@Test func tryingToTurnItOnAsksTheModelAndSaysWhyItCant() {
		withDefaults { defaults in
			let settings = AssistantSettings(defaults: defaults, availability: FixedAvailability(.unavailable(.modelNotReady)))
			#expect(settings.canTurnOn, "not asked yet")
			settings.isOn = true
			#expect(!settings.isOn)
			#expect(!settings.canTurnOn)
			#expect(!defaults.bool(forKey: AssistantSettings.storageKey))
			#expect(settings.availability?.explanation == "The Apple Intelligence model hasn't finished downloading.")
		}
	}

	@Test func withTheSettingOffTheModelIsNeverAsked() {
		// QA §10 D5: with AI off, nothing AI-related loads.
		withDefaults { defaults in
			let checker = CountingAvailability()
			var answerers = 0
			let settings = AssistantSettings(defaults: defaults, availability: checker) { _ in
				answerers += 1
				return ScriptedAnswerer(parts: [])
			}
			#expect(!settings.offersPanel)
			#expect(settings.canTurnOn)
			settings.refresh()
			#expect(!settings.isOn)
			#expect(checker.asks == 0, "a launch with AI off, and opening Settings, never asks the model")
			#expect(answerers == 0, "no model session is made")
			settings.isOn = true
			#expect(checker.asks == 1, "turning it on is what asks")
		}
	}

	@Test func aModelThatArrivesLaterIsPickedUpOnRefresh() {
		withDefaults { defaults in
			let checker = SwitchableAvailability(.unavailable(.modelNotReady))
			let settings = AssistantSettings(defaults: defaults, availability: checker)
			settings.isOn = true
			#expect(!settings.canTurnOn)
			checker.set(.available)
			settings.refresh()
			#expect(settings.canTurnOn)
			#expect(!settings.isOn, "still off until the user turns it on")
		}
	}

	@Test func failuresReadAsPlainSentences() {
		#expect(AssistantFailure(AssistantFailure(message: "Kept.")).message == "Kept.")
		let tool = LanguageModelSession.ToolCallError(tool: AccountsTool(context: AssistantContext(queries: RecordingQueries(), format: MoneyFormat(), today: "2026-11-17")), underlyingError: FfiError.Io(message: "The disk is full."))
		#expect(AssistantFailure(tool).message == "Couldn't read the budget. The disk is full.")
	}
}

final class CountingAvailability: AssistantAvailabilityChecking {
	private let count = Mutex(0)

	var asks: Int { count.withLock { $0 } }

	var availability: AssistantAvailability {
		count.withLock { $0 += 1 }
		return .available
	}
}

final class SwitchableAvailability: AssistantAvailabilityChecking {
	private let current: Mutex<AssistantAvailability>

	init(_ availability: AssistantAvailability) {
		current = Mutex(availability)
	}

	var availability: AssistantAvailability { current.withLock { $0 } }

	func set(_ availability: AssistantAvailability) {
		current.withLock { $0 = availability }
	}
}

/// Runs only where Apple Intelligence is on and its model is downloaded.
@MainActor
@Suite(.enabled(if: SystemLanguageModel.default.isAvailable, "needs the on-device model"))
struct AssistantModelTests {
	@Test func theModelAnswersFromTheToolsAndChangesNothing() async throws {
		try await RealBudget.with { budget in
			let before = try budget.transactionRows()
			let context = AssistantContext(queries: try #require(budget.workspace.readOnlyQueries), format: budget.workspace.format, today: "2026-11-17")
			let assistant = FoundationModelsAssistant(context: context)
			var answer = ""
			for try await partial in assistant.answer("What was my Net Profit in October 2026?") {
				answer = partial
			}
			#expect(!answer.isEmpty)
			#expect(try budget.transactionRows() == before)
		}
	}
}
