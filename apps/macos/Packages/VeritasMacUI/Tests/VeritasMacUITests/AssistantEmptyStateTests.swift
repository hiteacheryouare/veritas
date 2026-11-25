import SwiftUI
import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// The empty panel says what Mo Nee answers and offers AI-11.4's examples,
/// each of which asks itself when clicked.
@MainActor
struct AssistantEmptyStateTests {
	@Test func theEmptyPanelShowsTheScopeAndTheRedirectsExamples() throws {
		let model = AssistantPanelModel(makeAnswerer: { _ in StubAnswerer(answers: [:]) }) { nil }
		let drawn = try DrawnText.texts(in: AssistantEmptyState(model: model), size: CGSize(width: 340, height: 420))
		#expect(drawn.joined(separator: " ").contains("answers from your own numbers"), "\(drawn)")
		for example in AssistantPrompt.examples {
			#expect(drawn.joined(separator: " ").contains(example), "\(example) in \(drawn)")
			#expect(AssistantPrompt.redirect.contains("\"\(example)\""), "the buttons and the redirect stay in step")
		}
	}

	@Test func clickingAnExampleAsksIt() async {
		let model = AssistantPanelModel(makeAnswerer: { _ in StubAnswerer(answers: ["How much did I spend last month?": "Spending, October 2026: $2,489.35"]) }) {
			AssistantContext(queries: StubReadOnlyQueries(), format: MoneyFormat(), today: "2026-11-17")
		}
		model.ask(AssistantPrompt.examples[0])
		await model.settle()
		#expect(model.exchanges.map(\.question) == [AssistantPrompt.examples[0]], "sent at once, and the empty state gives way to the conversation")
		#expect(model.draft.isEmpty)
		model.clear()
		#expect(model.exchanges.isEmpty, "New Conversation brings the empty state back")
	}
}
