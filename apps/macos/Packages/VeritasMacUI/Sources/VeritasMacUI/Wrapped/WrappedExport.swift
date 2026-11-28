import AppKit
import SwiftUI
import UniformTypeIdentifiers
import VeritasDesign
import VeritasKit

/// Saves Wrapped as the user picks (R-WRAP-2, design/wrapped-editorial.md,
/// Export): this spread as a PNG, every spread as numbered PNGs in a folder,
/// or the whole issue as a PDF with real text. Each comes with or without
/// amounts, on light or dark paper, from the same views as the screen.
@MainActor
enum WrappedExport {
	enum Kind {
		case spread, allSpreads, pdf
	}

	/// A spread is 2560 by 1600 pixels.
	static let scale: CGFloat = 2

	static func view(_ spread: WrappedSpread, paper: ColorScheme) -> some View {
		WrappedSpreadView(spread: spread, palette: .of(paper))
	}

	static func png(_ spread: WrappedSpread, paper: ColorScheme) -> Data? {
		let renderer = ImageRenderer(content: view(spread, paper: paper))
		renderer.scale = scale
		guard let image = renderer.cgImage else { return nil }
		return NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
	}

	/// One page per spread, drawn as vectors so the text stays selectable.
	static func pdf(_ spreads: [WrappedSpread], paper: ColorScheme) -> Data? {
		let data = NSMutableData()
		var box = CGRect(origin: .zero, size: WrappedCanvas.spread)
		guard let consumer = CGDataConsumer(data: data as CFMutableData),
			let context = CGContext(consumer: consumer, mediaBox: &box, nil)
		else { return nil }
		for spread in spreads {
			ImageRenderer(content: view(spread, paper: paper)).render { _, draw in
				context.beginPDFPage(nil)
				draw(context)
				context.endPDFPage()
			}
		}
		context.closePDF()
		return data as Data
	}

	static func run(_ kind: Kind, model: WrappedModel, redacted: Bool, paper: ColorScheme, workspace: WorkspaceModel) async {
		guard let issue = model.issue(redacted: redacted), let current = issue.spreads.first(where: { $0.index == model.index }) ?? issue.spreads.first else { return }
		let choice = PaperChoice(paper)
		let panel: NSSavePanel
		switch kind {
		case .spread:
			panel = NSSavePanel()
			panel.allowedContentTypes = [.png]
			panel.nameFieldStringValue = model.imageFileName(spread: current.index, redacted: redacted)
		case .pdf:
			panel = NSSavePanel()
			panel.allowedContentTypes = [.pdf]
			panel.nameFieldStringValue = model.pdfFileName(redacted: redacted)
		case .allSpreads:
			let open = NSOpenPanel()
			open.canChooseFiles = false
			open.canChooseDirectories = true
			open.message = "Choose a folder for \(model.folderName(redacted: redacted))."
			panel = open
		}
		panel.canCreateDirectories = true
		panel.isExtensionHidden = false
		panel.prompt = "Export"
		panel.accessoryView = choice.view
		guard await panel.beginOnKeyWindow() == .OK, let url = panel.url else { return }
		do {
			let paper = choice.scheme
			switch kind {
			case .spread:
				try write(png(current, paper: paper), to: url)
			case .pdf:
				try write(pdf(issue.spreads, paper: paper), to: url)
			case .allSpreads:
				let folder = url.appending(path: model.folderName(redacted: redacted))
				try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
				for spread in issue.spreads {
					try write(png(spread, paper: paper), to: folder.appending(path: model.imageFileName(spread: spread.index, redacted: redacted)))
				}
			}
		} catch {
			workspace.alert = ErrorAlert(title: "Couldn't save Wrapped", message: error.localizedDescription)
		}
	}

	private static func write(_ data: Data?, to url: URL) throws {
		guard let data else { throw CocoaError(.fileWriteUnknown) }
		try data.write(to: url, options: .atomic)
	}
}

/// The save panel's Paper popup, set to the appearance on screen, so a reader
/// in dark mode can still send a light page.
@MainActor
private final class PaperChoice {
	let view: NSView
	private let popup: NSPopUpButton

	init(_ scheme: ColorScheme) {
		popup = NSPopUpButton(frame: .zero, pullsDown: false)
		popup.addItems(withTitles: ["Light", "Dark"])
		popup.selectItem(at: scheme == .dark ? 1 : 0)
		popup.setAccessibilityIdentifier("wrapped.export.paper")
		let label = NSTextField(labelWithString: "Paper:")
		let stack = NSStackView(views: [label, popup])
		stack.edgeInsets = NSEdgeInsets(top: 8, left: 16, bottom: 8, right: 16)
		view = stack
	}

	var scheme: ColorScheme {
		popup.indexOfSelectedItem == 1 ? .dark : .light
	}
}
