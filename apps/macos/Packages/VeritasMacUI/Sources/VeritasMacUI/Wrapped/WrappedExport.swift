import AppKit
import SwiftUI
import UniformTypeIdentifiers
import VeritasDesign
import VeritasKit

/// Every card of the story in one image, three to a row (R-WRAP-2).
struct WrappedStoryImage: View {
	static let columns = 3
	let cards: [WrappedCard]

	var body: some View {
		let rows = stride(from: 0, to: cards.count, by: Self.columns).map { Array(cards[$0..<min($0 + Self.columns, cards.count)]) }
		Grid(alignment: .topLeading, horizontalSpacing: Spacing.s5, verticalSpacing: Spacing.s5) {
			ForEach(rows.indices, id: \.self) { index in
				GridRow {
					ForEach(rows[index]) { card in
						WrappedCardView(card: card, colors: .image)
					}
				}
			}
		}
		.padding(Spacing.s6)
		.background(WrappedColors.image.page)
		.environment(\.colorScheme, .light)
	}
}

/// Saves the story as a PNG where the user picks, with or without amounts.
@MainActor
enum WrappedExport {
	static let scale: CGFloat = 2

	static func png(_ cards: [WrappedCard]) -> Data? {
		let renderer = ImageRenderer(content: WrappedStoryImage(cards: cards))
		renderer.scale = scale
		guard let image = renderer.cgImage else { return nil }
		return NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
	}

	static func run(_ model: WrappedModel, redacted: Bool, workspace: WorkspaceModel) async {
		let cards = model.cards(redacted: redacted)
		guard !cards.isEmpty else { return }
		let panel = NSSavePanel()
		panel.allowedContentTypes = [.png]
		panel.canCreateDirectories = true
		panel.nameFieldStringValue = model.imageFileName(redacted: redacted)
		panel.isExtensionHidden = false
		panel.prompt = "Export"
		guard await panel.beginOnKeyWindow() == .OK, let url = panel.url else { return }
		do {
			guard let png = png(cards) else { throw CocoaError(.fileWriteUnknown) }
			try png.write(to: url, options: .atomic)
		} catch {
			workspace.alert = ErrorAlert(title: "Couldn't save the image", message: error.localizedDescription)
		}
	}
}
