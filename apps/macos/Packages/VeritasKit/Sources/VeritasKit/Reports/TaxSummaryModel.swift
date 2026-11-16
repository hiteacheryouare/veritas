import Foundation
import Observation
import VeritasCore

/// The Tax summary screen (R-REP-8, docs/finance/tax-buckets.md): one calendar
/// year's buckets, the rows behind each, the rows whose tags conflict, and the
/// two TAX-5 CSVs exactly as the core writes them.
@MainActor
@Observable
public final class TaxSummaryModel {
	public enum Export: CaseIterable, Hashable, Sendable {
		case summary
		case detail
	}

	public private(set) var year: Int32
	public private(set) var summary: TaxSummary?
	public var expanded: Set<TaxBucket> = []

	@ObservationIgnored let workspace: WorkspaceModel
	@ObservationIgnored public let thisYear: Int32

	public init(workspace: WorkspaceModel, thisYear: Int32? = nil) {
		self.workspace = workspace
		let year = thisYear ?? Month(date: WorkspaceModel.today())?.year ?? 2026
		self.thisYear = year
		self.year = year
	}

	public var format: MoneyFormat { workspace.format }

	/// This year and the nine before it, newest first.
	public var years: [Int32] { Array((thisYear - 9)...thisYear).reversed() }
	public var canGoForward: Bool { year < thisYear }

	public func load() async {
		let year = year
		guard let summary = await workspace.report({ try await $0.taxSummary(year: year) }) else { return }
		guard year == self.year else { return }
		self.summary = summary
	}

	public func choose(_ year: Int32) async {
		guard year != self.year else { return }
		self.year = year
		expanded = []
		await load()
	}

	public func previous() async {
		await choose(year - 1)
	}

	public func next() async {
		guard canGoForward else { return }
		await choose(year + 1)
	}

	public func toggle(_ bucket: TaxBucket) {
		if expanded.contains(bucket) { expanded.remove(bucket) } else { expanded.insert(bucket) }
	}

	public var buckets: [TaxBucketTotal] { summary?.buckets ?? [] }

	/// The core's detail rows for one bucket, in the core's order.
	public func rows(in bucket: TaxBucket) -> [TaxDetailRow] {
		summary?.detail.filter { $0.bucket == bucket } ?? []
	}

	/// TAX-3.3's "Needs a decision" rows: tags that name two or more buckets.
	public var conflicts: [TaxDetailRow] {
		summary?.detail.filter { $0.bucket == nil } ?? []
	}

	/// TAX-5's file names, `veritas-tax-summary-2026.csv`.
	public func fileName(_ export: Export) -> String {
		let year = summary?.year ?? year
		switch export {
		case .summary: return "veritas-tax-summary-\(year).csv"
		case .detail: return "veritas-tax-detail-\(year).csv"
		}
	}

	public func csv(_ export: Export) -> String? {
		guard let summary else { return nil }
		switch export {
		case .summary: return summary.summaryCsv
		case .detail: return summary.detailCsv
		}
	}

	/// Writes the core's CSV to `url` unchanged. A failed write is shown as an alert.
	@discardableResult
	public func export(_ export: Export, to url: URL) -> Bool {
		guard let csv = csv(export) else { return false }
		do {
			try Self.write(csv, to: url)
			return true
		} catch {
			workspace.present(FfiError.Io(message: error.localizedDescription), during: .export)
			return false
		}
	}

	public nonisolated static func write(_ csv: String, to url: URL) throws {
		try Data(csv.utf8).write(to: url, options: .atomic)
	}
}
