import SwiftUI
import VeritasDesign
import VeritasKit

/// The conversation about this budget, in the window's inspector: questions
/// on the right, answers on the left, the field pinned under them.
struct AssistantPanel: View {
	@Bindable var model: AssistantPanelModel
	@FocusState private var isFieldFocused: Bool
	@State private var position = ScrollPosition(edge: .bottom)

	/// How close to the end still counts as reading the newest message.
	static let followSlack: CGFloat = 24
	/// Room always kept for a scroller that takes space ("Always show scroll
	/// bars"), so the messages don't narrow and reflow when one appears. The
	/// system's preferred style can change while the panel is open, so it
	/// isn't consulted.
	static let scrollerRoom = NSScroller.scrollerWidth(for: .regular, scrollerStyle: .legacy)

	/// The messages' width, read from the panel's own width during layout,
	/// so it is right on the first frame and never waits for a state update.
	static func contentWidth(panel: CGFloat) -> CGFloat {
		max(0, panel - 2 * Spacing.s3 - scrollerRoom)
	}

	var body: some View {
		GeometryReader { geometry in
			VStack(spacing: 0) {
				header
				Divider()
				if model.exchanges.isEmpty {
					AssistantEmptyState(model: model)
				} else {
					conversation(width: Self.contentWidth(panel: geometry.size.width))
				}
				Divider()
				composer
			}
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

	/// The newest answer stays in view as it grows, but only for a reader
	/// already at the bottom, and without animating each update.
	private func conversation(width: CGFloat) -> some View {
		ScrollView {
			VStack(spacing: Spacing.s3) {
				ForEach(model.exchanges) { exchange in
					QuestionBubble(text: exchange.question)
					AnswerBubble(exchange: exchange, width: width - Spacing.s6)
				}
			}
			.frame(width: width)
			.padding(Spacing.s3)
			.frame(maxWidth: .infinity, alignment: .leading)
		}
		.scrollPosition($position)
		.defaultScrollAnchor(.bottom)
		.onScrollGeometryChange(for: ScrollFill.self) { geometry in
			ScrollFill(height: geometry.contentSize.height, below: geometry.contentSize.height - geometry.visibleRect.maxY)
		} action: { old, new in
			guard new.height > old.height, old.below <= Self.followSlack else { return }
			position.scrollTo(edge: .bottom)
		}
		.onChange(of: model.exchanges.count) {
			position.scrollTo(edge: .bottom)
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

/// What Mo Nee answers, and AI-11.4's example questions, each of which asks
/// itself when clicked. It shows only while the conversation is empty.
struct AssistantEmptyState: View {
	let model: AssistantPanelModel

	var body: some View {
		VStack(spacing: Spacing.s4) {
			Spacer(minLength: 0)
			Text(AssistantPrompt.scope)
				.font(.text(.heading))
				.foregroundStyle(.textPrimary)
				.multilineTextAlignment(.center)
				.accessibilityLabel(AssistantPrompt.scope.replacingOccurrences(of: AssistantName.shown, with: AssistantName.spoken))
			VStack(spacing: Spacing.s2) {
				ForEach(AssistantPrompt.examples, id: \.self) { example in
					Button {
						model.ask(example)
					} label: {
						Text(example)
							.font(.text(.body))
							.foregroundStyle(.textPrimary)
							.multilineTextAlignment(.center)
							.fixedSize(horizontal: false, vertical: true)
							.padding(.horizontal, Spacing.s3)
							.padding(.vertical, Spacing.s2)
							.frame(maxWidth: .infinity)
							.background(.surfaceRaised, in: RoundedRectangle(cornerRadius: Radius.section, style: .continuous))
							.overlay(RoundedRectangle(cornerRadius: Radius.section, style: .continuous).strokeBorder(Color.separator))
							.contentShape(RoundedRectangle(cornerRadius: Radius.section, style: .continuous))
					}
					.buttonStyle(.plain)
					.accessibilityLabel("Ask: \(example)")
						.accessibilityIdentifier("assistant.example")
				}
			}
			Spacer(minLength: 0)
		}
		.padding(Spacing.s5)
		.frame(maxWidth: .infinity, maxHeight: .infinity)
	}
}

/// The conversation's height, and how much of it is below the visible part.
struct ScrollFill: Equatable {
	var height: CGFloat
	var below: CGFloat
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

extension EnvironmentValues {
	/// Told each reply bubble's frame as it lays out, for the streaming test.
	@Entry var assistantReplyFrame: (@MainActor (UUID, CGRect) -> Void)?
}

struct AnswerBubble: View {
	let exchange: AssistantExchange
	/// Fixed while the answer streams, so the bubble never resizes as words arrive.
	var width: CGFloat?
	@Environment(\.assistantReplyFrame) private var reportFrame

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
						ZStack(alignment: .leading) {
							Text(" ").hidden()
							TypingDots()
						}
					}
					if !exchange.answer.isEmpty {
						Text(Self.formatted(exchange.answer, streaming: exchange.state == .answering))
							.foregroundStyle(.textPrimary)
					}
					if let failure {
						Text(failure).foregroundStyle(.textSecondary)
					}
				}
				.font(.text(.body))
				.textSelection(.enabled)
				.frame(maxWidth: width.map { $0 - 2 * Spacing.s3 } ?? .infinity, alignment: .leading)
				.frame(minWidth: width.map { $0 - 2 * Spacing.s3 }, alignment: .leading)
				.padding(.horizontal, Spacing.s3)
				.padding(.vertical, Spacing.s2)
				.background(.surfaceRaised, in: RoundedRectangle(cornerRadius: Radius.section, style: .continuous))
				.overlay(RoundedRectangle(cornerRadius: Radius.section, style: .continuous).strokeBorder(Color.separator))
				.onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frame in
					reportFrame?(exchange.id, frame)
				}
				Spacer(minLength: Spacing.s6)
			}
		}
		.accessibilityElement(children: .ignore)
		.accessibilityLabel(AssistantSpeech.reply(answer: exchange.answer, failure: failure))
		.accessibilityValue(exchange.state == .answering ? AssistantSpeech.answering : "")
	}

	/// The model sometimes writes **bold**; show it as bold rather than as
	/// asterisks. While it streams, an unclosed "**" waits for its partner,
	/// so the words don't flip from plain to bold and reflow.
	static func formatted(_ answer: String, streaming: Bool = false) -> AttributedString {
		var text = answer
		if streaming, text.components(separatedBy: "**").count % 2 == 0, let open = text.range(of: "**", options: .backwards) {
			text.removeSubrange(open)
		}
		return (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(text)
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

	nonisolated static let count = 3
	nonisolated static let cycle: TimeInterval = 1.2
	nonisolated static let still = 0.55

	var body: some View {
		if reduceMotion {
			dots(at: nil)
		} else {
			TimelineView(.animation(minimumInterval: 1.0 / 30)) { timeline in
				dots(at: timeline.date.timeIntervalSinceReferenceDate)
			}
		}
	}

	private func dots(at time: TimeInterval?) -> some View {
		HStack(spacing: 4) {
			ForEach(0..<Self.count, id: \.self) { index in
				Circle()
					.fill(Color.textSecondary)
					.frame(width: 7, height: 7)
					.opacity(Self.opacity(index: index, time: time))
			}
		}
	}

	/// Each dot brightens a third of a cycle after the one before it. A nil
	/// time, as with Reduce Motion, holds every dot at the same opacity.
	nonisolated static func opacity(index: Int, time: TimeInterval?) -> Double {
		guard let time else { return still }
		let phase = (time / cycle - Double(index) / Double(count + 1)).truncatingRemainder(dividingBy: 1)
		let wave = 0.5 - 0.5 * cos(2 * .pi * (phase < 0 ? phase + 1 : phase))
		return 0.3 + 0.7 * wave
	}
}
