import AppKit
import UniformTypeIdentifiers
import VeritasKit

@MainActor
enum ExportPanel {
	static func run(for model: WorkspaceModel) async {
		let panel = NSSavePanel()
		panel.allowedContentTypes = [.zip]
		panel.canCreateDirectories = true
		panel.nameFieldStringValue = "\(model.overview?.name ?? "Budget").zip"
		panel.prompt = "Export"
		guard await panel.begin() == .OK, let url = panel.url else { return }
		await model.exportActualZip(to: url)
	}
}
