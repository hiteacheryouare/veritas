import SwiftUI
import VeritasKit

/// The assistant panel for one open budget. Its conversation starts over with each budget.
struct AssistantInspector: View {
	@State private var panel: AssistantPanelModel

	init(workspace: WorkspaceModel, settings: AssistantSettings) {
		_panel = State(initialValue: AssistantPanelModel(makeAnswerer: settings.makeAnswerer) { [weak workspace] in
			guard let workspace, let queries = workspace.readOnlyQueries else { return nil }
			return AssistantContext(queries: queries, format: workspace.format, today: WorkspaceModel.today())
		})
	}

	var body: some View {
		AssistantPanel(model: panel)
	}
}
