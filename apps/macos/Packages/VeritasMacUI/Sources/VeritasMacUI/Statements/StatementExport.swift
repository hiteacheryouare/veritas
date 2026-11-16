import AppKit
import UniformTypeIdentifiers
import VeritasKit

/// Saves a statement as a PDF where the user picks. The sandbox grants the
/// chosen file through the panel (user-selected read-write).
@MainActor
enum StatementExport {
	static func run(_ document: StatementDocument, period: String, format: MoneyFormat, workspace: WorkspaceModel) async {
		let panel = NSSavePanel()
		panel.allowedContentTypes = [.pdf]
		panel.canCreateDirectories = true
		panel.nameFieldStringValue = document.fileName(period: period)
		panel.prompt = "Export"
		let response = if let window = NSApp.keyWindow {
			await panel.beginSheetModal(for: window)
		} else {
			await panel.begin()
		}
		guard response == .OK, let url = panel.url else { return }
		do {
			try StatementPDF.data(document, format: format).write(to: url, options: .atomic)
		} catch {
			workspace.alert = ErrorAlert(title: "Couldn't save the PDF", message: error.localizedDescription)
		}
	}
}
