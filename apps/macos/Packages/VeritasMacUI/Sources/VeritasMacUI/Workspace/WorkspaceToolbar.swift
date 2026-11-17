import SwiftUI
import VeritasDesign
import VeritasKit

struct WorkspaceToolbar: ToolbarContent {
	@Bindable var model: WorkspaceModel
	var offersAssistant = false

	var body: some ToolbarContent {
		if model.destination == .screen(.budget) {
			ToolbarItem(placement: .primaryAction) {
				Toggle(isOn: Binding(get: { model.isDense }, set: { _ in model.toggleDense() })) {
					Label("Dense", systemImage: "line.3.horizontal")
				}
				.toggleStyle(.button)
				.labelStyle(.titleAndIcon)
				.help("Dense (⌘D)")
				.accessibilityIdentifier("toolbar.dense")
			}
		}
		ToolbarItem(placement: .primaryAction) {
			GoToAnythingButton { model.isPaletteShown = true }
		}
		if offersAssistant {
			ToolbarItem(placement: .primaryAction) {
				Toggle(isOn: $model.isAssistantShown) {
					Label("Ask", systemImage: "text.bubble")
				}
				.toggleStyle(.button)
				.help("Ask about this budget")
				.accessibilityIdentifier("toolbar.assistant")
			}
		}
	}
}
