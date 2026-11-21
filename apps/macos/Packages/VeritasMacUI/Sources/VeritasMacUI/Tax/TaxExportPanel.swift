import AppKit
import UniformTypeIdentifiers
import VeritasKit

/// Saves one of the core's tax CSVs where the user picks.
@MainActor
enum TaxExportPanel {
	static func run(_ export: TaxSummaryModel.Export, for model: TaxSummaryModel) async {
		let panel = NSSavePanel()
		panel.allowedContentTypes = [.commaSeparatedText]
		panel.canCreateDirectories = true
		panel.nameFieldStringValue = model.fileName(export)
		panel.isExtensionHidden = false
		panel.prompt = "Export"
		guard await panel.beginOnKeyWindow() == .OK, let url = panel.url else { return }
		model.export(export, to: url)
	}

	/// The summary and its rows as a PDF, with TAX-8's limits and TAX-5's footer.
	static func runPDF(for model: TaxSummaryModel, workspace: WorkspaceModel) async {
		guard let document = model.document else { return }
		let panel = NSSavePanel()
		panel.allowedContentTypes = [.pdf]
		panel.canCreateDirectories = true
		panel.nameFieldStringValue = document.fileName
		panel.isExtensionHidden = false
		panel.prompt = "Export"
		guard await panel.beginOnKeyWindow() == .OK, let url = panel.url else { return }
		do {
			try TaxPDF.data(document).write(to: url, options: .atomic)
		} catch {
			workspace.alert = ErrorAlert(title: "Couldn't save the PDF", message: error.localizedDescription)
		}
	}
}
