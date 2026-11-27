import AppKit
import PDFKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// Wrapped's editorial spreads on the stub budget, into
/// target/screenshots/wrapped: every spread on light and dark paper, with and
/// without amounts, the screen at a wide and a narrow window, and the exports.
/// The redacted issue is drawn into a PDF so the text it renders can be read
/// back and checked for money amounts.
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

	private func renderedText(_ spreads: [WrappedSpread]) throws -> String {
		let data = try #require(WrappedExport.pdf(spreads, paper: .light))
		let pdf = try #require(PDFDocument(data: data))
		#expect(pdf.pageCount == spreads.count, "a page per spread")
		return try #require(pdf.string).split(whereSeparator: \.isWhitespace).joined(separator: " ")
	}

	@Test func theIssueHasEverySpreadForTheStubYear() async throws {
		let (_, model) = await model()
		#expect(model.years == [2026, 2025], "the core's years with data, 2025 included")
		#expect(model.spreads.map { $0.pages.map(\.kind) } == [
			[.cover, .contents], [.ledger], [.categories, .groups], [.payees, .paidDown],
			[.shape], [.quietDays, .goals], [.netWorth], [.notes, .backCover]
		])
		let goals = model.issue?.pages.first { $0.kind == .goals }
		#expect(goals?.rows.map(\.label) == ["Vet bill", "Winter tires"], "N4, by month")
		#expect(model.title == "Wrapped 2026 so far")
	}

	@Test func everySpreadOnLightAndDarkPaperWithAndWithoutAmounts() async throws {
		let (_, model) = await model()
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		for redacted in [false, true] {
			let issue = try #require(model.issue(redacted: redacted))
			for spread in issue.spreads {
				for paper in [ColorScheme.light, .dark] {
					let png = try #require(WrappedExport.png(spread, paper: paper))
					let image = try #require(NSBitmapImageRep(data: png))
					#expect(image.pixelsWide == 2560 && image.pixelsHigh == 1600, "\(spread.id)")
					let name = "spread-\(spread.index + 1)-\(spread.id)\(redacted ? "-redacted" : "")-\(paper == .dark ? "dark" : "light").png"
					try png.write(to: Self.output.appending(path: name))
				}
			}
		}
	}

	@Test func theRedactedIssueDrawsNoMoneyAmount() async throws {
		let (_, model) = await model()
		let redactedIssue = try #require(model.issue(redacted: true))
		let shownIssue = try #require(model.issue(redacted: false))
		let hidden = try renderedText(redactedIssue.spreads)
		#expect(hidden.contains("36.1%"), "the core's share for Rent")
		#expect(hidden.contains("72.9% of what came in went out."))
		#expect(hidden.contains("Rent took the") && hidden.contains("largest share."), "a headline over two lines, which the PDF text interleaves with the facing page")
		#expect(!hidden.contains("$"), "no currency symbol")
		#expect(hidden.firstMatch(of: /\d\.\d\d(?![\d%])/) == nil, "no two-place figure")
		for amount: Int64 in [8_421_000, 6_140_000, 2_281_000, 1_980_000, 812_000, 3_983_000, 520_000, 960_000, 575_000, 11_183_000, 7_150_000] {
			#expect(!hidden.contains(model.format.amount(amount, on: .register)), "\(amount)")
		}
		let shown = try renderedText(shownIssue.spreads)
		#expect(shown.contains("$19,800.00"))
		#expect(shown.contains("$84,210.00"))
		// An export draws one mode only: the hidden twin that keeps a figure's
		// room on screen never reaches the file (the PNG draws the same view).
		#expect(!shown.contains("72.9% of what came in went out."), "no redacted twin in the shown export")
		#expect(!shown.contains("Since the start of 2026."))
		#expect(!hidden.contains("From "), "no shown twin, the net worth dek, in the redacted export")
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		try #require(WrappedExport.pdf(shownIssue.spreads, paper: .light)).write(to: Self.output.appending(path: "issue.pdf"))
	}

	@Test func theScreenWideAndNarrow() async throws {
		let (workspace, model) = await model()
		try render(WrappedScreen(workspace: workspace, model: model), named: "screen-cover", size: CGSize(width: 1400, height: 900))
		model.show(4)
		try render(WrappedScreen(workspace: workspace, model: model), named: "screen-shape", size: CGSize(width: 1400, height: 900))
		model.hidesAmounts = true
		try render(WrappedScreen(workspace: workspace, model: model), named: "screen-shape-hidden", size: CGSize(width: 1400, height: 900))
		model.hidesAmounts = false
		model.show(2)
		try render(WrappedScreen(workspace: workspace, model: model), named: "screen-narrow", size: CGSize(width: 800, height: 900))
	}

	@Test func anEmptyYearIsTheCoverAlone() async throws {
		let (workspace, model) = await model()
		await model.choose(2024)
		#expect(model.issue?.pages.map(\.kind) == [.cover])
		#expect(model.issue?.pages.first?.dek == "No data for 2024.")
		try render(WrappedScreen(workspace: workspace, model: model), named: "screen-empty", size: CGSize(width: 1400, height: 900))
	}

	/// A second lets a spread's first-view entrance (at most 700 ms) finish before the capture.
	func render(_ view: some View, named name: String, size: CGSize, settle: TimeInterval = 1) throws {
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
