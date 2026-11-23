import AppKit
import PDFKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// Wrapped on the stub budget, light and dark, and its exported image, into
/// target/screenshots/wrapped. The redacted image is drawn into a PDF too, so
/// the text it renders can be read back and checked for money amounts.
@MainActor
struct WrappedScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/wrapped")
		.standardizedFileURL
	static let today = "2026-11-09"

	private func model() async -> (WorkspaceModel, WrappedModel) {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		let model = WrappedModel(workspace: workspace, today: Self.today)
		await model.load()
		return (workspace, model)
	}

	/// The text an image of `cards` draws, read back from the same view drawn as a PDF.
	private func renderedText(_ cards: [WrappedCard]) throws -> String {
		let renderer = ImageRenderer(content: WrappedStoryImage(cards: cards))
		let data = NSMutableData()
		renderer.render { size, draw in
			var box = CGRect(origin: .zero, size: size)
			guard let consumer = CGDataConsumer(data: data as CFMutableData), let context = CGContext(consumer: consumer, mediaBox: &box, nil) else { return }
			context.beginPDFPage(nil)
			draw(context)
			context.endPDFPage()
			context.closePDF()
		}
		let pdf = try #require(PDFDocument(data: data as Data))
		return try #require(pdf.string).split(whereSeparator: \.isWhitespace).joined(separator: " ")
	}

	@Test func theStoryHasEveryCardForTheStubYear() async throws {
		let (_, model) = await model()
		#expect(model.years == [2026, 2025], "the core's years with data, 2025 included")
		#expect(model.cards.map(\.kind) == [.totals, .categories, .groups, .payees, .biggestMonth, .debtAndSavings, .noSpend, .goals, .netWorth])
		#expect(model.cards.first { $0.kind == .goals }?.rows.map(\.label) == ["Winter tires", "Vet bill"])
		#expect(model.title == "Wrapped 2026 so far")
	}

	@Test func theRedactedImageDrawsNoMoneyAmount() async throws {
		let (_, model) = await model()
		let hidden = try renderedText(model.cards(redacted: true))
		#expect(hidden.contains("36.1%"), "the core's share for Rent")
		#expect(hidden.contains("72.9% of what came in"))
		#expect(hidden.contains("Rent topped your spending."))
		#expect(!hidden.contains("$"), "no currency symbol")
		#expect(hidden.firstMatch(of: /\d\.\d\d(?![\d%])/) == nil, "no two-place figure")
		for amount: Int64 in [8_421_000, 6_140_000, 2_281_000, 1_980_000, 812_000, 3_983_000, 520_000, 960_000] {
			#expect(!hidden.contains(model.format.amount(amount, on: .register)), "\(amount)")
		}

		let shown = try renderedText(model.cards(redacted: false))
		#expect(shown.contains("$19,800.00"))
		#expect(shown.contains("$84,210.00"))
	}

	@Test func theImageIsEveryCardThreeToARow() async throws {
		let (_, model) = await model()
		let png = try #require(WrappedExport.png(model.cards(redacted: true)))
		let image = try #require(NSBitmapImageRep(data: png))
		let rows = 3, columns = 3
		let width = (WrappedCardView.size.width * CGFloat(columns) + Spacing.s5 * CGFloat(columns - 1) + Spacing.s6 * 2) * WrappedExport.scale
		let height = (WrappedCardView.size.height * CGFloat(rows) + Spacing.s5 * CGFloat(rows - 1) + Spacing.s6 * 2) * WrappedExport.scale
		#expect(image.pixelsWide == Int(width))
		#expect(image.pixelsHigh == Int(height))
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		try png.write(to: Self.output.appending(path: "wrapped-image-redacted.png"))
		try #require(WrappedExport.png(model.cards(redacted: false))).write(to: Self.output.appending(path: "wrapped-image.png"))
	}

	@Test func theScreenOneCardAtATime() async throws {
		let (workspace, model) = await model()
		try render(WrappedScreen(workspace: workspace, model: model), named: "wrapped-totals", size: CGSize(width: 900, height: 760))
		model.show(4)
		try render(WrappedScreen(workspace: workspace, model: model), named: "wrapped-biggest-month", size: CGSize(width: 900, height: 760))
		model.hidesAmounts = true
		try render(WrappedScreen(workspace: workspace, model: model), named: "wrapped-biggest-month-hidden", size: CGSize(width: 900, height: 760))
	}

	@Test func anEmptyYearIsOneCard() async throws {
		let (workspace, model) = await model()
		await model.choose(2024)
		#expect(model.cards.map(\.title) == ["No data for 2024."])
		try render(WrappedScreen(workspace: workspace, model: model), named: "wrapped-empty", size: CGSize(width: 900, height: 760))
	}

	func render(_ view: some View, named name: String, size: CGSize, settle: TimeInterval = 0.4) throws {
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		for (appearance, suffix) in [(NSAppearance.Name.aqua, "light"), (.darkAqua, "dark")] {
			let host = NSHostingView(rootView: view.tint(Color.accent).background(.surfaceWindow))
			host.frame.size = size
			let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
			window.appearance = NSAppearance(named: appearance)
			window.contentView = host
			host.layoutSubtreeIfNeeded()
			RunLoop.main.run(until: .now.addingTimeInterval(settle))
			host.layoutSubtreeIfNeeded()
			let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
			host.cacheDisplay(in: host.bounds, to: bitmap)
			let png = try #require(bitmap.representation(using: .png, properties: [:]))
			try png.write(to: Self.output.appending(path: "\(name)-\(suffix).png"))
		}
	}
}
