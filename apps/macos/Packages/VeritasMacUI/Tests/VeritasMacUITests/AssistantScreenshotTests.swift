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
		"What did I spend on groceries last month?": "Spending on Groceries, October 2026: 412.00. That's from 9 transactions on the Expenses line of the Net Profit report.",
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
		let drawn = try DrawnText.texts(in: VStack {
			AssistantTitle()
			ForEach(panel.exchanges) { exchange in
				QuestionBubble(text: exchange.question)
				AnswerBubble(exchange: exchange)
			}
		}, size: CGSize(width: 340, height: 520))
		#expect(drawn.filter { $0 == "Mo Nee" }.count == 3, "the title and one sender label per reply, none on questions: \(drawn)")

		let failing = AssistantPanelModel(makeAnswerer: { _ in HeldAnswerer(failure: "The model is busy. Ask again in a moment.") }) {
			workspace.readOnlyQueries.map { AssistantContext(queries: $0, format: workspace.format, today: "2026-11-17") }
		}
		failing.draft = "What did I spend last month?"
		failing.ask()
		await failing.settle()
		#expect(failing.exchanges.first?.state == .failed("The model is busy. Ask again in a moment."))
		try render(AssistantPanel(model: failing).frame(width: 340, height: 520), named: "panel-error")

		let held = HeldAnswerer(partial: "Spending, October 2026: 2,489.35. This is the Expenses line")
		let streaming = AssistantPanelModel(makeAnswerer: { _ in held }) {
			workspace.readOnlyQueries.map { AssistantContext(queries: $0, format: workspace.format, today: "2026-11-17") }
		}
		streaming.draft = "What did I spend last month?"
		streaming.ask()
		for _ in 0..<10_000 where streaming.exchanges.first?.answer.isEmpty ?? true {
			await Task.yield()
		}
		#expect(streaming.isAnswering)
		try render(AssistantPanel(model: streaming).frame(width: 340, height: 520), named: "panel-streaming")

		let waiting = AssistantPanelModel(makeAnswerer: { _ in HeldAnswerer() }) {
			workspace.readOnlyQueries.map { AssistantContext(queries: $0, format: workspace.format, today: "2026-11-17") }
		}
		waiting.draft = "What did I spend last month?"
		waiting.ask()
		for _ in 0..<10_000 where waiting.exchanges.isEmpty {
			await Task.yield()
		}
		#expect(waiting.isAnswering && waiting.exchanges.first?.answer.isEmpty == true)
		try render(AssistantPanel(model: waiting).frame(width: 340, height: 520), named: "panel-waiting")
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

/// Sends one part of an answer and then holds the stream open, or fails.
@MainActor
final class HeldAnswerer: AssistantAnswering {
	let partial: String?
	let failure: String?
	private var held: AsyncThrowingStream<String, any Error>.Continuation?

	init(partial: String? = nil, failure: String? = nil) {
		self.partial = partial
		self.failure = failure
	}

	func answer(_ question: String) -> AsyncThrowingStream<String, any Error> {
		let (stream, continuation) = AsyncThrowingStream.makeStream(of: String.self, throwing: (any Error).self)
		if let partial { continuation.yield(partial) }
		if let failure { continuation.finish(throwing: AssistantFailure(message: failure)) }
		held = continuation
		return stream
	}

	func reset() {}
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
