import AppKit
import UniformTypeIdentifiers
import VeritasKit

/// Saves a statement as a PDF where the user picks. The sandbox grants the
/// chosen file through the panel (user-selected read-write).
@MainActor
enum StatementExport {
	static func run(_ document: StatementDocument, period: String, format: MoneyFormat, workspace: WorkspaceModel) async {
		await save(named: document.fileName(period: period), workspace: workspace) {
			StatementPDF.data(document, format: format)
		}
	}

	/// The four statements for the chosen months, read when the user picks the file.
	static func runSet(_ model: StatementsModel, workspace: WorkspaceModel) async {
		guard let name = model.setFileName else { return }
		await save(named: name, workspace: workspace) {
			guard let documents = await model.statementSet() else { return nil }
			return StatementPDF.data(documents, title: String(name.dropLast(4)), format: model.format)
		}
	}

	static func save(named name: String, workspace: WorkspaceModel, data: () async -> Data?) async {
		let panel = NSSavePanel()
		panel.allowedContentTypes = [.pdf]
		panel.canCreateDirectories = true
		panel.nameFieldStringValue = name
		panel.prompt = "Export"
		guard await panel.beginOnKeyWindow() == .OK, let url = panel.url, let pdf = await data() else { return }
		do {
			try pdf.write(to: url, options: .atomic)
		} catch {
			workspace.alert = ErrorAlert(title: "Couldn't save the PDF", message: error.localizedDescription)
		}
	}
}
