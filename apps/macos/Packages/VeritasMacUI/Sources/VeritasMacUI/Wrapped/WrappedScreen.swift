import SwiftUI
import VeritasDesign
import VeritasKit

/// Wrapped (R-WRAP-1, R-WRAP-2, design/mockups/wrapped.html): a year's story,
/// one card at a time, with the arrows or ← and →. Hide amounts shows the
/// core's shares in place of every amount, and the export saves every card
/// as one image.
struct WrappedScreen: View {
	let workspace: WorkspaceModel
	@State private var model: WrappedModel
	@FocusState private var hasKeyFocus: Bool

	init(workspace: WorkspaceModel, model: WrappedModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? WrappedModel(workspace: workspace))
	}

	var body: some View {
		Group {
			if let card = model.card {
				HStack(spacing: Spacing.s5) {
					arrow("chevron.left", label: "Previous card", id: "wrapped.previous", enabled: model.canGoBack) { model.previous() }
					WrappedCardView(card: card, progress: (model.index, model.cards.count))
						.shadow(color: .black.opacity(0.18), radius: 24, y: 10)
					arrow("chevron.right", label: "Next card", id: "wrapped.next", enabled: model.canGoForward) { model.next() }
				}
				.padding(Spacing.s5)
			} else {
				ProgressView()
			}
		}
		.frame(maxWidth: .infinity, maxHeight: .infinity)
		.background(.surfaceWindow)
		.focusable()
		.focusEffectDisabled()
		.focused($hasKeyFocus)
		.defaultFocus($hasKeyFocus, true)
		.onKeyPress(keys: [.leftArrow, .rightArrow]) { press in
			if press.key == .leftArrow { model.previous() } else { model.next() }
			return .handled
		}
		.toolbar {
			ToolbarItem(placement: .navigation) {
				Picker("Year", selection: Binding(get: { model.year }, set: { year in Task { await model.choose(year) } })) {
					ForEach(model.years, id: \.self) { Text(model.yearTitle($0)).tag($0) }
				}
				.pickerStyle(.menu)
				.labelsHidden()
				.fixedSize()
				.accessibilityIdentifier("wrapped.yearPicker")
			}
			ToolbarItem(placement: .primaryAction) {
				Toggle(isOn: $model.hidesAmounts) {
					Label("Hide amounts", systemImage: "eye.slash")
						.labelStyle(.titleAndIcon)
				}
				.toggleStyle(.button)
				.accessibilityIdentifier("wrapped.hideAmounts")
			}
			ToolbarItem(placement: .primaryAction) {
				Menu {
					Button("Image…") { Task { await WrappedExport.run(model, redacted: false, workspace: workspace) } }
						.accessibilityIdentifier("wrapped.export.image")
					Button("Image without amounts…") { Task { await WrappedExport.run(model, redacted: true, workspace: workspace) } }
						.accessibilityIdentifier("wrapped.export.redacted")
				} label: {
					Label("Export", systemImage: "square.and.arrow.up")
						.labelStyle(.titleAndIcon)
				}
				.disabled(model.report == nil)
				.accessibilityIdentifier("wrapped.export")
			}
		}
		.navigationTitle(model.title)
		.task(id: workspace.changeCount) { await model.load() }
		.containerIdentifier("screen.wrapped")
	}

	private func arrow(_ symbol: String, label: String, id: String, enabled: Bool, action: @escaping () -> Void) -> some View {
		Button(action: action) {
			Image(systemName: symbol)
				.font(.system(size: 15, weight: .semibold))
				.frame(width: 44, height: 44)
				.background(.surfaceControl, in: .circle)
				.overlay { Circle().strokeBorder(Color.edgeControl) }
				.contentShape(.circle)
		}
		.buttonStyle(.plain)
		.disabled(!enabled)
		.opacity(enabled ? 1 : 0.4)
		.help(label)
		.accessibilityLabel(label)
		.accessibilityIdentifier(id)
	}
}
