import SwiftUI
import VeritasDesign
import VeritasKit

struct WorkspaceView: View {
	@Bindable var model: WorkspaceModel
	@State private var paletteChoice: PaletteItem?

	var body: some View {
		NavigationSplitView {
			SidebarView(model: model)
				.navigationSplitViewColumnWidth(min: 200, ideal: Size.sidebar, max: 320)
		} detail: {
			ScreenView(destination: model.destination, title: model.title(of: model.destination))
				.safeAreaInset(edge: .top, spacing: 0) {
					VStack(spacing: 0) {
						if let readOnly = model.readOnly {
							ReadOnlyBanner(notice: readOnly.notice) { action in
								switch action {
								case .showIntegrityReport: model.sheet = .integrityReport
								}
							}
						}
						FindBar(isShown: $model.isFindShown)
					}
				}
				.navigationTitle(model.title(of: model.destination))
				.toolbar { WorkspaceToolbar(model: model) }
		}
		.sheet(isPresented: $model.isPaletteShown, onDismiss: activatePaletteChoice) {
			CommandPaletteView(index: model.paletteIndex, format: model.format) { item in
				paletteChoice = item
				model.isPaletteShown = false
			}
		}
	}

	/// Runs after the palette has gone, so a command that opens a sheet
	/// doesn't race the palette's own sheet off the window.
	private func activatePaletteChoice() {
		guard let item = paletteChoice else { return }
		paletteChoice = nil
		model.activate(item)
	}
}
