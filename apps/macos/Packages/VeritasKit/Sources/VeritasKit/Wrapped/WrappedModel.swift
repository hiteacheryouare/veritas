import Foundation
import Observation
import VeritasCore

/// The Wrapped screen (R-WRAP-1, R-WRAP-2): one year's issue, spread by
/// spread, with or without amounts. The core computes every figure and share.
@MainActor
@Observable
public final class WrappedModel {
	public private(set) var year: Int32
	/// The core's years with data, newest first; this year alone when there are none.
	public private(set) var years: [Int32]
	public private(set) var report: WrappedReport?
	/// The spread on screen.
	public private(set) var index = 0
	public var hidesAmounts = false

	@ObservationIgnored let workspace: WorkspaceModel
	/// Both modes of the issue for the report and format they were written
	/// from, so a frame of a page turn or a mode switch reads them, not rebuilds them.
	@ObservationIgnored private var written: (report: WrappedReport, format: MoneyFormat, shown: WrappedIssue, hidden: WrappedIssue)?
	@ObservationIgnored public let today: String
	@ObservationIgnored public let thisYear: Int32

	public init(workspace: WorkspaceModel, today: String? = nil) {
		self.workspace = workspace
		let today = today ?? WorkspaceModel.today()
		self.today = today
		let year = Month(date: today)?.year ?? 2026
		thisYear = year
		self.year = year
		years = [year]
	}

	public var format: MoneyFormat { workspace.format }

	public func load() async {
		let today = today
		if let listed = await workspace.report({ try await $0.wrappedYears(today: today) }) {
			years = listed.isEmpty ? [thisYear] : listed
			if !years.contains(year) {
				year = years[0]
				report = nil
				index = 0
			}
		}
		await loadReport()
	}

	private func loadReport() async {
		let year = year, today = today
		guard let report = await workspace.report({ try await $0.wrapped(year: year, today: today) }), year == self.year else { return }
		self.report = report
		index = min(index, max(spreads.count - 1, 0))
	}

	public func choose(_ year: Int32) async {
		guard year != self.year else { return }
		self.year = year
		report = nil
		index = 0
		await loadReport()
	}

	public func issue(redacted: Bool) -> WrappedIssue? {
		guard let report else { return nil }
		let format = format
		if let written, written.report == report, written.format == format {
			return redacted ? written.hidden : written.shown
		}
		let shown = WrappedIssue(report: report, format: format, redacted: false)
		let hidden = WrappedIssue(report: report, format: format, redacted: true)
		written = (report, format, shown, hidden)
		return redacted ? hidden : shown
	}

	public var issue: WrappedIssue? { issue(redacted: hidesAmounts) }

	public var spreads: [WrappedSpread] { issue?.spreads ?? [] }

	public var spread: WrappedSpread? {
		let spreads = spreads
		return spreads.indices.contains(index) ? spreads[index] : nil
	}

	public var canGoBack: Bool { index > 0 }
	public var canGoForward: Bool { index < spreads.count - 1 }

	public func previous() {
		if canGoBack { index -= 1 }
	}

	public func next() {
		if canGoForward { index += 1 }
	}

	public func show(_ index: Int) {
		if spreads.indices.contains(index) { self.index = index }
	}

	public func showFirst() { show(0) }

	public func showLast() { show(spreads.count - 1) }

	/// The contents page's spread, or the cover's when there is none.
	public func showContents() {
		show(spreads.firstIndex { $0.pages.contains { $0.kind == .contents } } ?? 0)
	}

	/// "Wrapped 2026", "Wrapped 2026 so far".
	public var title: String {
		issue?.isYearToDate ?? (year == thisYear) ? "Wrapped \(year) so far" : "Wrapped \(year)"
	}

	/// "Wrapped 2026 spread 3.png", "Wrapped 2026 spread 3 without amounts.png".
	public func imageFileName(spread: Int, redacted: Bool) -> String {
		"Wrapped \(year) spread \(spread + 1)\(redacted ? " without amounts" : "").png"
	}

	/// The folder an all-spreads export writes into.
	public func folderName(redacted: Bool) -> String {
		"Wrapped \(year)\(redacted ? " without amounts" : "")"
	}

	/// "Wrapped 2026.pdf", "Wrapped 2026 without amounts.pdf".
	public func pdfFileName(redacted: Bool) -> String {
		"Wrapped \(year)\(redacted ? " without amounts" : "").pdf"
	}

	public func yearTitle(_ year: Int32) -> String {
		year == thisYear ? "\(year) so far" : String(year)
	}
}
