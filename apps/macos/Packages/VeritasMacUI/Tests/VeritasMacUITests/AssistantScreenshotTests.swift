import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// Settings › AI and the Ask panel, light and dark, into target/screenshots/ai.
/// The panel's answers come from a stub, never the real model.
@MainActor
struct AssistantScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/ai")
		.standardizedFileURL

	static let answers = [
		"What did I spend on groceries last month?": "Groceries spent 412.00 in October 2026, from the October budget. You budgeted 450.00 and 38.00 is still available.",
		"How is my net worth?": "Net worth was 48,210.55 on October 31, 2026, from the balance sheet: total assets 61,904.12 and total liabilities 13,693.57."
	]

	private func settings(_ availability: AssistantAvailability, on: Bool = false) -> (AssistantSettings, UserDefaults, String) {
		let suite = "veritas-assistant-shots-\(UUID().uuidString)"
		let defaults = UserDefaults(suiteName: suite)!
		let settings = AssistantSettings(defaults: defaults, availability: FixedAvailability(availability)) { _ in StubAnswerer(answers: Self.answers) }
		settings.isOn = on
		return (settings, defaults, suite)
	}

	@Test func settingsRow() throws {
		for (name, availability, on) in [
			("settings-off", AssistantAvailability.available, false),
			("settings-on", .available, true),
			("settings-unavailable", .unavailable(.appleIntelligenceNotEnabled), true),
			("settings-not-downloaded", .unavailable(.modelNotReady), true)
		] {
			let (settings, defaults, suite) = self.settings(availability, on: on)
			defer { defaults.removePersistentDomain(forName: suite) }
			try render(AssistantSettingsView(settings: settings).frame(width: 760), named: name)
		}
	}

	@Test func settingsSayWhyTheModelCantBeUsed() async throws {
		let (settings, defaults, suite) = self.settings(.unavailable(.deviceNotEligible), on: true)
		defer { defaults.removePersistentDomain(forName: suite) }
		let texts = try DrawnText.texts(in: AssistantSettingsView(settings: settings), size: CGSize(width: 760, height: 160))
		#expect(texts.contains { $0.contains("This Mac doesn't support Apple Intelligence") }, "\(texts)")
		#expect(!settings.offersPanel)
	}

	@Test func panel() async throws {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		let (settings, defaults, suite) = self.settings(.available, on: true)
		defer { defaults.removePersistentDomain(forName: suite) }
		func makePanel() -> AssistantPanelModel {
			AssistantPanelModel(makeAnswerer: settings.makeAnswerer) {
				workspace.readOnlyQueries.map { AssistantContext(queries: $0, format: workspace.format, today: "2026-11-17") }
			}
		}
		try render(AssistantPanel(model: makePanel()).frame(width: 340, height: 520), named: "panel-empty")
		// Each rendered panel stops its model's answer when its offscreen window goes, so the asked panel is never rendered before it answers.
		let panel = makePanel()
		for question in ["What did I spend on groceries last month?", "How is my net worth?"] {
			panel.draft = question
			panel.ask()
			await panel.settle()
		}
		#expect(panel.exchanges.map(\.state) == [.answered, .answered])
		try render(AssistantPanel(model: panel).frame(width: 340, height: 520), named: "panel")
	}

	func render(_ view: some View, named name: String, settle: TimeInterval = 0.4) throws {
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		for (appearance, suffix) in [(NSAppearance.Name.aqua, "light"), (.darkAqua, "dark")] {
			let host = NSHostingView(rootView: view.tint(Color.accent).background(.surfaceWindow))
			host.frame.size = host.fittingSize
			let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
			window.appearance = NSAppearance(named: appearance)
			window.contentView = host
			host.layoutSubtreeIfNeeded()
			RunLoop.main.run(until: .now.addingTimeInterval(settle))
			host.frame.size = host.fittingSize
			host.layoutSubtreeIfNeeded()
			let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
			host.cacheDisplay(in: host.bounds, to: bitmap)
			let png = try #require(bitmap.representation(using: .png, properties: [:]))
			try png.write(to: Self.output.appending(path: "\(name)-\(suffix).png"))
		}
	}
}

/// Answers from a fixed list, a word at a time, as the real model streams.
@MainActor
final class StubAnswerer: AssistantAnswering {
	let answers: [String: String]

	init(answers: [String: String]) {
		self.answers = answers
	}

	func answer(_ question: String) -> AsyncThrowingStream<String, any Error> {
		let answer = answers[question] ?? ""
		return AsyncThrowingStream { continuation in
			let words = answer.split(separator: " ")
			for count in words.indices {
				continuation.yield(words[...count].joined(separator: " "))
			}
			continuation.finish()
		}
	}

	func reset() {}
}
