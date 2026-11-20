import Foundation
import VeritasCore

/// The Tax summary PDF's words (R-REP-8, docs/finance/tax-buckets.md): the
/// statements' entity name, the bucket totals with TAX-4's note, the rows
/// that need a decision, each bucket's rows, and TAX-8's known limits. Every
/// figure is the core's, with a true minus sign (ST-10.4).
public struct TaxDocument: Hashable, Sendable {
	public static let title = "Tax summary"
	/// TAX-5's PDF footer.
	public static let footer = "Organized from your own categories and tags. Not tax advice."
	public static let limitsTitle = "Known limits"
	public static let conflictsTitle = "Needs a decision"
	/// Under Needs a decision: these rows show the amount as recorded, where
	/// a bucket's rows show what was counted (TAX-4).
	public static let conflictsNote = "Not counted. Amounts as recorded."
	/// TAX-8, in the order the spec lists them.
	public static let limits = [
		"Totals are only what was recorded. A paycheck that isn't split into gross pay and withholding counts as the deposit.",
		"Interest earned inside an off-budget account counts only if it's recorded in an on-budget income category.",
		"A refund counts where it's recorded. To net a refund against a tagged purchase, give the refund the same tag.",
		"A merged category's rows count under the bucket of the category it was merged into, in every year.",
		"A bucket is a label you chose. Whether an amount affects a tax return is for your tax software or accountant."
	]

	public struct Bucket: Hashable, Sendable {
		public let id: String
		public let name: String
		public let count: String
		public let total: String
		/// TAX-4's note, when the core flags the total negative.
		public let note: String?
		public let rows: [Row]
	}

	public struct Row: Hashable, Sendable {
		public let id: String
		/// "Mar 14".
		public let date: String
		public let payee: String
		/// With ST-10.3's brackets.
		public let category: String
		public let account: String
		public let amount: String
		/// The row's notes, or for a conflict the buckets its tags name.
		public let note: String?
	}

	public let entity: String
	public let year: Int32
	/// "Tax year 2026, in USD".
	public let basis: String
	public let buckets: [Bucket]
	public let conflicts: [Row]

	public init(summary: TaxSummary, entity: String, currencyCode: String?, format: MoneyFormat) {
		self.entity = entity
		year = summary.year
		basis = currencyCode.map { "Tax year \(summary.year), in \($0)" } ?? "Tax year \(summary.year)"
		buckets = summary.buckets.map { total in
			Bucket(
				id: TaxBuckets.id(total.bucket),
				name: TaxBuckets.name(total.bucket),
				count: Int(total.count).formatted(),
				total: format.amount(total.total, on: .register),
				note: TaxBuckets.note(for: total),
				rows: summary.detail.filter { $0.bucket == total.bucket }.map { Self.row($0, amount: $0.counted ?? $0.amount, format: format) }
			)
		}
		conflicts = summary.detail.filter { $0.bucket == nil }.map { Self.row($0, amount: $0.amount, format: format) }
	}

	/// "Medical, continued", at the top of a page a table runs onto.
	public static func continued(_ title: String) -> String {
		"\(title), continued"
	}

	public var isEmpty: Bool { buckets.isEmpty && conflicts.isEmpty }

	/// "No transactions in a tax bucket in 2026", as the screen says it.
	public var emptyNote: String { "No transactions in a tax bucket in \(year)" }

	/// "Household Tax summary 2026.pdf".
	public var fileName: String {
		"\(entity) \(Self.title) \(year).pdf".replacingOccurrences(of: "/", with: "-")
	}

	static func row(_ row: TaxDetailRow, amount: Int64, format: MoneyFormat) -> Row {
		let note: String? = row.bucket == nil && !row.conflicting.isEmpty
			? "Tags name \(TaxBuckets.list(row.conflicting))"
			: (row.notes.isEmpty ? nil : row.notes)
		return Row(
			id: row.transactionId,
			date: CivilDate.date(row.date)?.formatted(.dateTime.month(.abbreviated).day()) ?? row.date,
			payee: row.payee,
			category: TaxBuckets.category(row),
			account: row.accountName,
			amount: format.amount(amount, on: .register),
			note: note
		)
	}
}

public extension TaxBuckets {
	/// The row's category as ST-10.3 labels it: "Old Gym (hidden)",
	/// "(deleted) Old Gym", or "Uncategorized (needs review)" with none.
	static func category(_ row: TaxDetailRow) -> String {
		guard let name = row.category else { return ReportWords.uncategorized }
		guard let labels = row.categoryLabels else { return name }
		return ReportWords.category(name, labels: labels)
	}

	/// "Medical and Education", "Medical, Education and HSA contributions".
	static func list(_ buckets: [TaxBucket]) -> String {
		let names = buckets.map(name)
		guard names.count > 1 else { return names.first ?? "" }
		return names.dropLast().joined(separator: ", ") + " and " + (names.last ?? "")
	}
}
