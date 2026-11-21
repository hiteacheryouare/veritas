import SwiftUI
import VeritasDesign
import VeritasKit

/// The questions asked about this budget and their answers, in the window's inspector.
struct AssistantPanel: View {
	@Bindable var model: AssistantPanelModel
	@FocusState private var isFieldFocused: Bool

	var body: some View {
		VStack(spacing: 0) {
			ScrollViewReader { proxy in
				ScrollView {
					LazyVStack(alignment: .leading, spacing: Spacing.s5) {
						ForEach(model.exchanges) { exchange in
							AssistantExchangeView(exchange: exchange).id(exchange.id)
						}
					}
					.padding(Spacing.s4)
					.frame(maxWidth: .infinity, alignment: .leading)
				}
				.onChange(of: model.exchanges.last?.answer) {
					guard let last = model.exchanges.last else { return }
					proxy.scrollTo(last.id, anchor: .bottom)
				}
			}
			Divider()
			HStack(spacing: Spacing.s2) {
				TextField("Ask about this budget", text: $model.draft)
					.textFieldStyle(.plain)
					.focused($isFieldFocused)
					.onSubmit(model.ask)
					.disabled(model.isAnswering)
					.accessibilityIdentifier("assistant.question")
				if !model.exchanges.isEmpty {
					Button("Clear", action: model.clear)
						.buttonStyle(.borderless)
						.accessibilityIdentifier("assistant.clear")
				}
			}
			.font(.text(.body))
			.padding(Spacing.s3)
		}
		.background(.surfaceWindow)
		.onAppear { isFieldFocused = true }
		.onDisappear(perform: model.stop)
		.containerIdentifier("assistant.panel")
	}
}

private struct AssistantExchangeView: View {
	let exchange: AssistantExchange

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			Text(exchange.question)
				.font(.text(.body, weight: .semibold))
				.foregroundStyle(.textPrimary)
			switch exchange.state {
			case .answering where exchange.answer.isEmpty:
				ProgressView().controlSize(.small)
			case .failed(let message):
				if !exchange.answer.isEmpty {
					Text(exchange.answer).foregroundStyle(.textPrimary)
				}
				Text(message).foregroundStyle(.textSecondary)
			default:
				Text(exchange.answer).foregroundStyle(.textPrimary)
			}
		}
		.font(.text(.body))
		.textSelection(.enabled)
		.frame(maxWidth: .infinity, alignment: .leading)
	}
}
