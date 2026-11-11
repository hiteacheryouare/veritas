import SwiftUI
import UniformTypeIdentifiers
import VeritasDesign
import VeritasKit

/// One window. It keeps its budget and its place across relaunches.
struct WorkspaceWindow: View {
	static let id = "workspace"

	@State private var model = WorkspaceModel(library: LaunchEnvironment.library())
	@SceneStorage("destination") private var storedDestination = Destination.start
	@SceneStorage("budgetDirectory") private var storedBudgetDirectory = ""

	var body: some View {
		@Bindable var model = model
		Group {
			if model.phase == .open {
				WorkspaceView(model: model)
			} else {
				EmptyStateView(isWorking: model.phase == .working, importActualZip: chooseActualZip)
			}
		}
		.frame(minWidth: 900, minHeight: 560)
		.background(.surfaceWindow)
		.tint(Color.accent)
		.focusedSceneValue(\.workspace, model)
		.fileImporter(isPresented: $model.isImportPanelShown, allowedContentTypes: [.zip]) { result in
			guard case .success(let zip) = result else { return }
			Task { await importActualZip(zip) }
		}
		.sheet(item: $model.sheet) { sheet in
			PlaceholderSheet(sheet: sheet)
		}
		.errorAlert(model: model)
		.onChange(of: model.destination) { _, destination in
			storedDestination = destination
		}
		.onChange(of: model.overview?.directory) { _, directory in
			storedBudgetDirectory = directory?.path(percentEncoded: false) ?? ""
		}
		.task {
			model.destination = storedDestination
			if let directory = LaunchEnvironment.budgetToOpenOnLaunch ?? restoredBudgetDirectory {
				await model.reopen(directory: directory)
				if model.phase != .open {
					storedBudgetDirectory = ""
				}
			}
		}
	}

	private var restoredBudgetDirectory: URL? {
		storedBudgetDirectory.isEmpty ? nil : URL(filePath: storedBudgetDirectory, directoryHint: .isDirectory)
	}

	private func chooseActualZip() {
		if let zip = LaunchEnvironment.importWithoutPanel {
			Task { await importActualZip(zip) }
		} else {
			model.isImportPanelShown = true
		}
	}

	private func importActualZip(_ zip: URL) async {
		await model.importActualZip(zip)
	}
}
