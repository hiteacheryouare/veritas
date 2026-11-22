import SwiftUI
import VeritasDesign
import VeritasKit

/// The conversation about this budget, in the window's inspector: questions
/// on the right, answers on the left, the field pinned under them.
struct AssistantPanel: View {
	@Bindable var model: AssistantPanelModel
	@FocusState private var isFieldFocused: Bool

	static let bottom = "assistant.bottom"

	var body: some View {
		VStack(spacing: 0) {
			header
			Divider()
			conversation
			Divider()
			composer
		}
		.background(.surfaceWindow)
		.onAppear { isFieldFocused = true }
		.onDisappear(perform: model.stop)
		.onChange(of: model.exchanges.last?.state) { _, state in
			guard state == .answered, let answer = model.exchanges.last?.answer else { return }
			AccessibilityNotification.Announcement(answer).post()
		}
		.containerIdentifier("assistant.panel")
	}

	private var header: some View {
		HStack {
			Spacer()
			Button {
				model.clear()
				isFieldFocused = true
			} label: {
				Label("New Conversation", systemImage: "square.and.pencil")
					.labelStyle(.iconOnly)
			}
			.buttonStyle(.borderless)
			.disabled(model.exchanges.isEmpty)
			.help("New Conversation")
			.accessibilityIdentifier("assistant.new")
		}
		.padding(.horizontal, Spacing.s3)
		.frame(height: Size.control)
		.background(.bar)
	}

	private var conversation: some View {
		ScrollViewReader { proxy in
			ScrollView {
				LazyVStack(spacing: Spacing.s3) {
					if model.exchanges.isEmpty {
						Text("Ask about spending, income, budgets, balances, goals or taxes in this budget.")
							.font(.text(.body))
							.foregroundStyle(.textSecondary)
							.multilineTextAlignment(.center)
							.frame(maxWidth: .infinity)
							.padding(.top, Spacing.s6)
					}
					ForEach(model.exchanges) { exchange in
						QuestionBubble(text: exchange.question)
						AnswerBubble(exchange: exchange)
					}
					Color.clear.frame(height: 1).id(Self.bottom)
				}
				.padding(Spacing.s3)
			}
			.defaultScrollAnchor(.bottom)
			.onChange(of: model.exchanges.last?.answer) {
				proxy.scrollTo(Self.bottom, anchor: .bottom)
			}
			.onChange(of: model.exchanges.count) {
				proxy.scrollTo(Self.bottom, anchor: .bottom)
			}
		}
	}

	private var composer: some View {
		HStack(alignment: .bottom, spacing: Spacing.s2) {
			TextField("Ask about this budget", text: $model.draft, axis: .vertical)
				.textFieldStyle(.plain)
				.lineLimit(1...5)
				.focused($isFieldFocused)
				.onSubmit(model.ask)
				.padding(.horizontal, Spacing.s3)
				.padding(.vertical, Spacing.s2 - 2)
				.background(.surfaceField, in: RoundedRectangle(cornerRadius: Radius.section, style: .continuous))
				.overlay(RoundedRectangle(cornerRadius: Radius.section, style: .continuous).strokeBorder(Color.separator))
				.accessibilityIdentifier("assistant.question")
			if model.isAnswering {
				ComposerButton(title: "Stop Answer", systemImage: "stop.fill", isEnabled: true, action: model.stop)
					.accessibilityIdentifier("assistant.stop")
			} else {
				ComposerButton(title: "Send", systemImage: "arrow.up", isEnabled: model.canAsk, action: model.ask)
					.accessibilityIdentifier("assistant.send")
			}
		}
		.font(.text(.body))
		.padding(Spacing.s3)
		.background(.bar)
	}
}

/// A round button beside the field, the height of one line of it.
private struct ComposerButton: View {
	let title: String
	let systemImage: String
	let isEnabled: Bool
	let action: () -> Void

	var body: some View {
		Button(action: action) {
			Image(systemName: systemImage)
				.font(.system(size: 11, weight: .bold))
				.foregroundStyle(isEnabled ? Color.textOnAccent : Color.textTertiary)
				.frame(width: 24, height: 24)
				.background(isEnabled ? Color.accentFill : Color.surfaceHover, in: Circle())
		}
		.buttonStyle(.plain)
		.disabled(!isEnabled)
		.padding(.bottom, 3)
		.help(title)
		.accessibilityLabel(title)
	}
}

private struct QuestionBubble: View {
	let text: String

	var body: some View {
		HStack {
			Spacer(minLength: Spacing.s6)
			Text(text)
				.font(.text(.body))
				.foregroundStyle(.textPrimary)
				.textSelection(.enabled)
				.padding(.horizontal, Spacing.s3)
				.padding(.vertical, Spacing.s2)
				.background(.accentSoft, in: RoundedRectangle(cornerRadius: Radius.section, style: .continuous))
		}
		.accessibilityElement(children: .combine)
		.accessibilityLabel("You: \(text)")
	}
}

private struct AnswerBubble: View {
	let exchange: AssistantExchange

	private var failure: String? {
		if case .failed(let message) = exchange.state { message } else { nil }
	}

	var body: some View {
		HStack {
			VStack(alignment: .leading, spacing: Spacing.s2) {
				if exchange.answer.isEmpty, exchange.state == .answering {
					ProgressView()
						.controlSize(.small)
						.accessibilityHidden(true)
				}
				if !exchange.answer.isEmpty {
					Text(Self.formatted(exchange.answer))
						.foregroundStyle(.textPrimary)
				}
				if let failure {
					Text(failure).foregroundStyle(.textSecondary)
				}
			}
			.font(.text(.body))
			.textSelection(.enabled)
			.padding(.horizontal, Spacing.s3)
			.padding(.vertical, Spacing.s2)
			.background(.surfaceRaised, in: RoundedRectangle(cornerRadius: Radius.section, style: .continuous))
			.overlay(RoundedRectangle(cornerRadius: Radius.section, style: .continuous).strokeBorder(Color.separator))
			Spacer(minLength: Spacing.s6)
		}
		.accessibilityElement(children: .ignore)
		.accessibilityLabel(accessibilityText)
		.accessibilityValue(exchange.state == .answering ? "Answering" : "")
	}

	private var accessibilityText: String {
		[exchange.answer.isEmpty ? nil : "Answer: \(exchange.answer)", failure].compactMap(\.self).joined(separator: " ")
	}

	/// The model sometimes writes **bold**; show it as bold rather than as asterisks.
	static func formatted(_ answer: String) -> AttributedString {
		(try? AttributedString(markdown: answer, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(answer)
	}
}
