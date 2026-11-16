import SwiftUI
import VeritasDesign
import VeritasKit

struct BillsScreen: View {
	let workspace: WorkspaceModel
	@State private var model: BillsModel

	init(workspace: WorkspaceModel, model: BillsModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? BillsModel(workspace: workspace))
	}

	var body: some View {
		Text("Bills")
			.task(id: workspace.changeCount) { await model.load() }
			.containerIdentifier("screen.bills")
	}
}
