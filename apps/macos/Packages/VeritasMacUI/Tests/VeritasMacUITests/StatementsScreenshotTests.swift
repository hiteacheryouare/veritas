import AppKit
import PDFKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// The four statements on the stub budget, light and dark, and their PDF,
/// into target/screenshots/insights.
@MainActor
struct StatementsScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/insights")
		.standardizedFileURL
	static let today = "2026-11-09"

	private func model(_ kind: StatementKind) async -> (WorkspaceModel, StatementsModel) {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		let model = StatementsModel(workspace: workspace, today: Self.today)
		await model.show(kind)
		if !kind.isDated {
			await model.choose(.quarter)
			await model.previous()
		}
		return (workspace, model)
	}

	@Test(arguments: StatementKind.allCases)
	func statement(_ kind: StatementKind) async throws {
		let (workspace, model) = await model(kind)
		let document = try #require(model.document)
		#expect(document.entity == "Stub")
		#expect(document.basis == (kind.isDated ? "As of November 9, 2026, in USD" : "July to September 2026, in USD"))
		try render(StatementsScreen(workspace: workspace, model: model), named: "statement-\(kind.rawValue)", size: CGSize(width: 900, height: kind == .incomeStatement ? 1120 : 820))
	}

	@Test func theBalanceSheetLaysOutAsTheMockupDoes() async throws {
		let (_, model) = await model(.balanceSheet)
		let lines = try #require(model.document).lines
		#expect(lines.map(\.title) == [
			"Assets", "Cash", "Checking", "Savings", "Old savings (closed)", "Total cash", "Investments", "Brokerage", "Total assets",
			"Liabilities", "Credit", "Visa", "Loans", "Car loan", "Total liabilities", "Net worth"
		])
	}

	@Test func ownersEquityHidesItsZeroLines() async throws {
		let (_, model) = await model(.ownersEquity)
		let titles = try #require(model.document).lines.map(\.title)
		#expect(titles == [
			"Opening equity", "Net income", "Off-budget assets acquired (expensed above)", "Reconciliation adjustments",
			"Off-budget revaluations", "Closing equity"
		])
	}

	@Test func thePDFHasTheScreensLabelsAndNumbers() async throws {
		for kind in StatementKind.allCases {
			let (_, model) = await model(kind)
			let document = try #require(model.document)
			let pdf = try #require(PDFDocument(data: StatementPDF.data(document, format: model.format)))
			#expect(pdf.pageCount == StatementPDF.paginate(document).count, "\(kind)")
			#expect(pdf.pageCount == (kind == .incomeStatement ? 2 : 1), "\(kind)")
			let text = try #require(pdf.string)
			for expected in ["Stub", "Unaudited. Obviously.", document.title, document.basis] {
				#expect(text.contains(expected), "\(kind): \(expected)")
			}
			for line in document.lines {
				#expect(text.contains(line.title), "\(kind): \(line.title)")
				if let amount = line.amount {
					#expect(text.contains(model.format.statement(amount, on: .register)), "\(kind): \(line.title)")
				}
			}
		}
	}

	@Test func aLongStatementRunsOntoMorePagesWithItsHeaderOnEach() async throws {
		let (_, model) = await model(.balanceSheet)
		let short = try #require(model.document)
		let accounts = (1...90).map { StatementLine(id: "account.\($0)", title: "Account \($0)", amount: Int64($0) * 10_000, style: .line) }
		let long = StatementDocument(
			kind: .balanceSheet, entity: short.entity, basis: short.basis,
			lines: [StatementLine(id: "assets", title: "Assets", style: .section)] + accounts + Array(short.lines.drop { $0.style != .total })
		)
		let pages = StatementPDF.paginate(long)
		#expect(pages.count > 1)
		#expect(pages.flatMap { $0 } == long.lines, "every line, once, in order")
		#expect(pages.dropLast().allSatisfy { page in page.last.map { $0.style != .section && $0.style != .group } ?? false }, "no heading ends a page")
		let pdf = try #require(PDFDocument(data: StatementPDF.data(long, format: model.format)))
		#expect(pdf.pageCount == pages.count)
		for index in 0..<pdf.pageCount {
			let text = try #require(pdf.page(at: index)?.string)
			#expect(text.contains("Balance sheet") && text.contains(short.basis), "page \(index + 1) has the header")
			#expect(text.contains("Page \(index + 1) of \(pdf.pageCount)"))
		}
		#expect(pdf.page(at: pdf.pageCount - 1)?.string?.contains("Net worth") == true)
	}

	@Test func aPDFPageAsAnImage() async throws {
		let (_, model) = await model(.balanceSheet)
		let document = try #require(model.document)
		let pdf = try #require(PDFDocument(data: StatementPDF.data(document, format: model.format)))
		let page = try #require(pdf.page(at: 0))
		let bounds = page.bounds(for: .mediaBox)
		let scale: CGFloat = 2
		let bitmap = try #require(NSBitmapImageRep(
			bitmapDataPlanes: nil, pixelsWide: Int(bounds.width * scale), pixelsHigh: Int(bounds.height * scale),
			bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
		))
		let context = try #require(NSGraphicsContext(bitmapImageRep: bitmap)).cgContext
		context.setFillColor(.white)
		context.fill(CGRect(x: 0, y: 0, width: bounds.width * scale, height: bounds.height * scale))
		context.scaleBy(x: scale, y: scale)
		page.draw(with: .mediaBox, to: context)
		let png = try #require(bitmap.representation(using: .png, properties: [:]))
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		try png.write(to: Self.output.appending(path: "statement-pdf-page-1.png"))
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
