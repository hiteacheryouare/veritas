import Foundation
import Observation
import VeritasCore

/// The Wrapped screen (R-WRAP-1, R-WRAP-2): one year's story, card by card,
/// with or without amounts. The core computes every figure and share.
@MainActor
@Observable
public final class WrappedModel {
	public private(set) var year: Int32
	/// The core's years with data, newest first; this year alone when there are none.
	public private(set) var years: [Int32]
	public private(set) var report: WrappedReport?
	public private(set) var goalNames: [String: String] = [:]
	public private(set) var index = 0
	public var hidesAmounts = false

	@ObservationIgnored let workspace: WorkspaceModel
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
		let goals = await workspace.goals()
		goalNames = Dictionary(goals.map { ($0.id, $0.name) }, uniquingKeysWith: { first, _ in first })
		await loadReport()
	}

	private func loadReport() async {
		let year = year, today = today
		guard let report = await workspace.report({ try await $0.wrapped(year: year, today: today) }), year == self.year else { return }
		self.report = report
		index = min(index, max(cards.count - 1, 0))
	}

	public func choose(_ year: Int32) async {
		guard year != self.year else { return }
		self.year = year
		report = nil
		index = 0
		await loadReport()
	}

	public var story: WrappedStory? {
		report.map { WrappedStory(report: $0, format: format, goalNames: goalNames) }
	}

	public var cards: [WrappedCard] {
		cards(redacted: hidesAmounts)
	}

	public func cards(redacted: Bool) -> [WrappedCard] {
		story?.cards(redacted: redacted) ?? []
	}

	public var card: WrappedCard? {
		let cards = cards
		return cards.indices.contains(index) ? cards[index] : nil
	}

	public var canGoBack: Bool { index > 0 }
	public var canGoForward: Bool { index < cards.count - 1 }

	public func previous() {
		if canGoBack { index -= 1 }
	}

	public func next() {
		if canGoForward { index += 1 }
	}

	public func show(_ index: Int) {
		if cards.indices.contains(index) { self.index = index }
	}

	/// "Wrapped 2026", "Wrapped 2026 so far".
	public var title: String {
		story?.isYearToDate ?? (year == thisYear) ? "Wrapped \(year) so far" : "Wrapped \(year)"
	}

	/// "Wrapped 2026.png", "Wrapped 2026 without amounts.png".
	public func imageFileName(redacted: Bool) -> String {
		redacted ? "Wrapped \(year) without amounts.png" : "Wrapped \(year).png"
	}

	public func yearTitle(_ year: Int32) -> String {
		year == thisYear ? "\(year) so far" : String(year)
	}
}
