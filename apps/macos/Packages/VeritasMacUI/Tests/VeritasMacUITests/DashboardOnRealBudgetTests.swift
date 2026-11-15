import AppKit
import Vision
import SQLite3
import SwiftUI
import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// The dashboard as drawn, on fixtures/synthetic/all-features through the
/// real core, read back from the accessibility tree.
@MainActor
struct DashboardOnRealBudgetTests {
	static let mainPage = "48f79e8a-0a29-4f3c-a7a3-9c04110766ad"

	@Test func aPercentageCardWithAZeroDivisorShowsNoDataRatherThanInfinity() async throws {
		// Fred on #60: a percentage card with a zero divisor shows a plain
		// "no data", never "NaN%" or "∞%". The Architect's call: the figure is blank.
		// The card is stored as Actual stores it: a divisor filter that matches nothing.
		let content = #"{\"type\":\"percentage\",\"divisorConditions\":[{\"field\":\"notes\",\"op\":\"contains\",\"value\":\"zzz-no-such-note\"}],\"divisorConditionsOp\":\"and\",\"divisorAllTimeDateRange\":false}"#
		let meta = #"{"name":"Share of nothing","content":""# + content + #"","timeFrame":{"start":"2026-01-01","end":"2026-11-30","mode":"yearToDate"}}"#
		try await withBudget(inserting: "UPDATE dashboard SET tombstone = 1; INSERT INTO dashboard (id, type, width, height, x, y, meta, tombstone, dashboard_page_id) VALUES ('pct-zero', 'summary-card', 3, 2, 0, 0, '\(meta)', 0, '\(Self.mainPage)')") { workspace in
			let model = DashboardModel(workspace: workspace)
			await model.load()
			let card = try #require(model.sheet?.cards.first { $0.id == "pct-zero" })
			#expect(card.content == .summary(.undefined), "the core's non-finite result arrives as an undefined figure")

			#expect(model.sheet?.cards.map(\.id) == ["pct-zero"], "the card is alone on its page")
			let texts = try await Self.texts(in: DashboardScreen(workspace: workspace, model: model), size: CGSize(width: 700, height: 360))
			#expect(texts.contains("Share of nothing"), "the card is drawn: \(texts)")
			let figure = texts.filter { $0.contains("%") || $0.contains("∞") || $0.contains("NaN") }
			#expect(figure.isEmpty, "\(figure)")
		}
	}

	/// Opens all-features with `sql` applied to its file first.
	private func withBudget(inserting sql: String, _ body: (WorkspaceModel) async throws -> Void) async throws {
		let budgets = URL.temporaryDirectory.appending(path: "veritas-real-\(UUID().uuidString)", directoryHint: .isDirectory)
		defer { try? FileManager.default.removeItem(at: budgets) }
		let fixture = URL(filePath: #filePath)
			.deletingLastPathComponent()
			.appending(path: "../../../../../../fixtures/synthetic/all-features.zip")
			.standardizedFileURL
		let workspace = WorkspaceModel(library: CoreBudgetLibrary(budgetsDirectory: budgets), format: MoneyFormat())
		await workspace.importActualZip(fixture)
		let directory = try #require(workspace.overview?.directory)
		await workspace.closeBudget()
		var db: OpaquePointer?
		defer { sqlite3_close(db) }
		try #require(sqlite3_open(directory.appending(path: "db.sqlite").path(percentEncoded: false), &db) == SQLITE_OK)
		sqlite3_busy_timeout(db, 5_000)
		try #require(sqlite3_exec(db, sql, nil, nil, nil) == SQLITE_OK, "\(String(cString: sqlite3_errmsg(db)))")
		await workspace.reopen(directory: directory)
		try #require(workspace.phase == .open)
		do {
			try await body(workspace)
		} catch {
			await workspace.closeBudget()
			throw error
		}
		await workspace.closeBudget()
	}

	/// The text Vision reads off the view as drawn, in light mode.
	static func texts(in view: some View, size: CGSize) async throws -> [String] {
		let host = NSHostingView(rootView: view.frame(width: size.width, height: size.height).background(.white))
		host.frame.size = size
		let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
		window.appearance = NSAppearance(named: .aqua)
		window.contentView = host
		for _ in 0..<10 {
			host.layoutSubtreeIfNeeded()
			try await Task.sleep(for: .milliseconds(20))
		}
		let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
		host.cacheDisplay(in: host.bounds, to: bitmap)
		let image = try #require(bitmap.cgImage)
		let request = VNRecognizeTextRequest()
		request.recognitionLevel = .accurate
		request.usesLanguageCorrection = false
		try VNImageRequestHandler(cgImage: image).perform([request])
		return (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }
	}
}
