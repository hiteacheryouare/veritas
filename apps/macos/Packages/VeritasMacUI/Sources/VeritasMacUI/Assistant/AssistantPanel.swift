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
			AssistantTitle()
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

/// "Mo Nee" on screen, "Money" to VoiceOver.
struct AssistantTitle: View {
	var body: some View {
		Text(AssistantName.shown)
			.font(.text(.body, weight: .semibold))
			.foregroundStyle(.textPrimary)
			.accessibilityLabel(AssistantName.spoken)
			.accessibilityAddTraits(.isHeader)
			.accessibilityIdentifier("assistant.title")
	}
}

struct QuestionBubble: View {
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
		.accessibilityLabel(AssistantSpeech.question(text))
	}
}

struct AnswerBubble: View {
	let exchange: AssistantExchange

	private var failure: String? {
		if case .failed(let message) = exchange.state { message } else { nil }
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s1) {
			Text(AssistantName.shown)
				.font(.text(.caption))
				.foregroundStyle(.textSecondary)
				.accessibilityLabel(AssistantName.spoken)
				.padding(.leading, Spacing.s1)
			HStack {
				VStack(alignment: .leading, spacing: Spacing.s2) {
					if exchange.answer.isEmpty, exchange.state == .answering {
						TypingDots()
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
		}
		.accessibilityElement(children: .ignore)
		.accessibilityLabel(AssistantSpeech.reply(answer: exchange.answer, failure: failure))
		.accessibilityValue(exchange.state == .answering ? AssistantSpeech.answering : "")
	}

	/// The model sometimes writes **bold**; show it as bold rather than as asterisks.
	static func formatted(_ answer: String) -> AttributedString {
		(try? AttributedString(markdown: answer, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(answer)
	}
}

/// What VoiceOver reads for the conversation: the assistant's name as it's said, "Money".
enum AssistantSpeech {
	static let answering = "Answering"

	static func question(_ text: String) -> String {
		"You: \(text)"
	}

	static func reply(answer: String, failure: String?) -> String {
		let said = [answer.isEmpty ? nil : answer, failure].compactMap(\.self).joined(separator: " ")
		return said.isEmpty ? AssistantName.spoken : "\(AssistantName.spoken): \(said)"
	}
}

/// Three dots that pulse one after another while the first words are on
/// their way. With Reduce Motion they hold still.
struct TypingDots: View {
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	static let count = 3
	static let cycle: TimeInterval = 1.2
	static let still = 0.55

	var body: some View {
		if reduceMotion {
			dots(at: nil)
		} else {
			TimelineView(.animation) { timeline in
				dots(at: timeline.date.timeIntervalSinceReferenceDate)
			}
		}
	}

	private func dots(at time: TimeInterval?) -> some View {
		HStack(spacing: 4) {
			ForEach(0..<Self.count, id: \.self) { index in
				Circle()
					.fill(Color.secondary)
					.frame(width: 7, height: 7)
					.opacity(Self.opacity(index: index, time: time))
			}
		}
		.frame(height: 18)
	}

	/// Each dot brightens a third of a cycle after the one before it. A nil
	/// time, as with Reduce Motion, holds every dot at the same opacity.
	static func opacity(index: Int, time: TimeInterval?) -> Double {
		guard let time else { return still }
		let phase = (time / cycle - Double(index) / Double(count + 1)).truncatingRemainder(dividingBy: 1)
		let wave = 0.5 - 0.5 * cos(2 * .pi * (phase < 0 ? phase + 1 : phase))
		return 0.3 + 0.7 * wave
	}
}
