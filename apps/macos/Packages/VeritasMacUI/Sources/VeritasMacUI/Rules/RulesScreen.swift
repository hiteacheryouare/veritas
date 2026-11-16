import SwiftUI
import VeritasDesign
import VeritasKit

struct RulesScreen: View {
	let workspace: WorkspaceModel
	@State private var model: RulesModel

	init(workspace: WorkspaceModel, model: RulesModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? RulesModel(workspace: workspace))
	}

	var body: some View {
		Text("Rules")
			.task(id: workspace.changeCount) { await model.load() }
			.containerIdentifier("screen.rules")
	}
}
