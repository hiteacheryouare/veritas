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
		panel.prompt = "Export"
		guard await panel.beginOnKeyWindow() == .OK, let url = panel.url else { return }
		model.export(export, to: url)
	}
}
