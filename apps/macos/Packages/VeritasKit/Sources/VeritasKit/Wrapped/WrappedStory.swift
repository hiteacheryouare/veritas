import Foundation
import VeritasCore

/// One card of the Wrapped story (R-WRAP-1, design/mockups/wrapped.html): a
/// sentence, an optional big figure, a line under it, and rows. A card holds
/// only finished text, so the screen and the exported image draw the same words.
public struct WrappedCard: Identifiable, Hashable, Sendable {
	public enum Kind: String, Hashable, Sendable {
		case totals
		case categories
		case groups
		case payees
		case biggestMonth
		case debtAndSavings
		case noSpend
		case goals
		case netWorth
		case noData
	}

	public struct Row: Identifiable, Hashable, Sendable {
		public let id: String
		public let label: String
		public let value: String
		public let note: String?

		public init(id: String, label: String, value: String, note: String? = nil) {
			self.id = id
			self.label = label
			self.value = value
			self.note = note
		}
	}

	public let kind: Kind
	public let title: String
	public let figure: String?
	public let detail: String?
	public let rows: [Row]

	public init(kind: Kind, title: String, figure: String? = nil, detail: String? = nil, rows: [Row] = []) {
		self.kind = kind
		self.title = title
		self.figure = figure
		self.detail = detail
		self.rows = rows
	}

	public var id: String { kind.rawValue }

	/// Every piece of text on the card, in reading order.
	public var text: [String] {
		[title, figure, detail].compactMap { $0 } + rows.flatMap { [$0.label, $0.value, $0.note].compactMap { $0 } }
	}
}

/// The words for each card from the core's `WrappedReport` (FC-8). With
/// `redacted`, R-WRAP-2's mode: no money amount appears anywhere, only the
/// core's shares, its counts, names and dates.
public struct WrappedStory: Sendable {
	public let report: WrappedReport
	public let format: MoneyFormat
	/// Goal names by id, for the goals the core lists as reached.
	public let goalNames: [String: String]

	public init(report: WrappedReport, format: MoneyFormat, goalNames: [String: String] = [:]) {
		self.report = report
		self.format = format
		self.goalNames = goalNames
	}

	/// The year to date rather than a whole year.
	public var isYearToDate: Bool {
		report.last != "\(report.year)-12-31"
	}

	public func cards(redacted: Bool) -> [WrappedCard] {
		guard report.hasData else {
			return [WrappedCard(kind: .noData, title: "No data for \(report.year).")]
		}
		return [
			totals(redacted),
			categories(redacted),
			groups(redacted),
			payees(redacted),
			biggestMonth(redacted),
			debtAndSavings(redacted),
			noSpend(),
			goals(),
			netWorth(redacted)
		].compactMap { $0 }
	}

	// MARK: Cards

	private func totals(_ redacted: Bool) -> WrappedCard {
		let prior = report.priorYear
		let rows: [WrappedCard.Row] = redacted
			? [
				WrappedCard.Row(id: "in", label: "Came in", value: change(prior.totalIn)),
				WrappedCard.Row(id: "out", label: "Went out", value: ofIncome(report.shares.outVsIn), note: change(prior.totalOut)),
				WrappedCard.Row(id: "net", label: netLabel, value: ofIncome(report.shares.net.map(abs)), note: change(prior.net))
			]
			: [
				WrappedCard.Row(id: "in", label: "Came in", value: money(report.totalIn), note: change(prior.totalIn)),
				WrappedCard.Row(id: "out", label: "Went out", value: money(report.totalOut), note: change(prior.totalOut)),
				WrappedCard.Row(id: "net", label: netLabel, value: format.currencyMagnitude(report.net, on: .register), note: change(prior.net))
			]
		return WrappedCard(
			kind: .totals,
			title: isYearToDate ? "Your \(report.year) so far" : "Your \(report.year)",
			detail: isYearToDate ? "\(WrappedWords.day(report.first)) to \(WrappedWords.day(report.last))" : nil,
			rows: rows
		)
	}

	/// A deficit year says so in words, and its figure drops the sign.
	private var netLabel: String {
		report.net < 0 ? "Came up short" : "Left over"
	}

	private func categories(_ redacted: Bool) -> WrappedCard? {
		guard let top = report.topCategories.first else { return nil }
		let name = ReportWords.category(top.name, labels: top.labels)
		var rows = report.topCategories.map { category in
			WrappedCard.Row(
				id: category.categoryId,
				label: ReportWords.category(category.name, labels: category.labels),
				value: redacted ? WrappedWords.percent(category.share, format: format) : money(category.total)
			)
		}
		if let other = everythingElse(report.otherCategories, share: report.shares.otherCategories, redacted) { rows.append(other) }
		let detail: String? = redacted ? "of categorized spending" : topCategoryShare.map { "\(WrappedWords.percent($0, format: format)) of categorized spending" }
		return WrappedCard(
			kind: .categories,
			title: "\(name) topped your spending.",
			figure: redacted ? WrappedWords.percent(top.share, format: format) : money(top.total),
			detail: detail,
			rows: rows
		)
	}

	private func groups(_ redacted: Bool) -> WrappedCard? {
		guard let top = report.topGroups.first else { return nil }
		var rows = report.topGroups.map { group in
			WrappedCard.Row(id: group.groupId, label: group.name, value: redacted ? WrappedWords.percent(group.share, format: format) : money(group.total))
		}
		if let other = everythingElse(report.otherGroups, share: report.shares.otherGroups, redacted) { rows.append(other) }
		return WrappedCard(
			kind: .groups,
			title: "\(top.name) was your biggest group.",
			figure: redacted ? WrappedWords.percent(top.share, format: format) : money(top.total),
			detail: redacted ? "of categorized spending" : nil,
			rows: rows
		)
	}

	private func payees(_ redacted: Bool) -> WrappedCard? {
		guard let top = report.topPayees.first else { return nil }
		var rows = report.topPayees.map { payee in
			WrappedCard.Row(
				id: payee.payeeId,
				label: payee.name,
				value: redacted ? WrappedWords.percent(payee.share, format: format) : money(payee.total),
				note: WrappedWords.payments(payee.count)
			)
		}
		if let other = everythingElse(report.otherPayees, share: report.shares.otherPayees, redacted) { rows.append(other) }
		let payments = WrappedWords.payments(top.count)
		return WrappedCard(
			kind: .payees,
			title: "You paid \(top.name) the most.",
			figure: redacted ? WrappedWords.percent(top.share, format: format) : money(top.total),
			detail: redacted ? "of what went to payees" : "across \(payments)",
			rows: rows
		)
	}

	private func biggestMonth(_ redacted: Bool) -> WrappedCard? {
		guard let month = report.biggestMonth else { return nil }
		let typical = report.biggestVsTypical.map { "\(WrappedWords.basisPoints($0, format: format)) more than your typical month" }
		let detail: String? = redacted
			? ([isYearToDate ? "of spending so far" : "of the year's spending", typical].compactMap { $0 }.joined(separator: ", "))
			: typical
		return WrappedCard(
			kind: .biggestMonth,
			title: "\(WrappedWords.monthName(month.month)) was your biggest month for spending.",
			figure: redacted ? share(report.shares.biggestMonth) : money(month.total),
			detail: detail
		)
	}

	private func debtAndSavings(_ redacted: Bool) -> WrappedCard? {
		guard report.debtPaydown != 0 || report.savings != 0 else { return nil }
		var rows: [WrappedCard.Row] = []
		if report.debtPaydown != 0 {
			rows.append(WrappedCard.Row(id: "debt", label: "Debt paydown", value: redacted ? ofIncome(report.shares.debtPaydown) : money(report.debtPaydown)))
		}
		if report.savings != 0 {
			rows.append(WrappedCard.Row(id: "savings", label: "Savings", value: redacted ? ofIncome(report.shares.savings) : money(report.savings)))
		}
		return WrappedCard(kind: .debtAndSavings, title: "Paid down and saved.", rows: rows)
	}

	private func noSpend() -> WrappedCard? {
		let noSpend = report.noSpend
		guard noSpend.evaluatedDays > 0, let first = noSpend.first, let last = noSpend.last else { return nil }
		let range = "\(WrappedWords.day(first)) to \(WrappedWords.day(last))"
		let detail = noSpendShare.map { "\(WrappedWords.percent($0, format: format)) of the \(WrappedWords.count(noSpend.evaluatedDays)) days from \(range)" }
			?? "Out of \(WrappedWords.count(noSpend.evaluatedDays)) days from \(range)"
		var rows: [WrappedCard.Row] = []
		if let longest = noSpend.longest {
			rows.append(WrappedCard.Row(id: "longest", label: "Longest streak", value: WrappedWords.days(longest.days), note: "from \(WrappedWords.day(longest.start))"))
		}
		if let current = noSpend.current {
			rows.append(WrappedCard.Row(id: "current", label: "Current streak", value: WrappedWords.days(current)))
		}
		return WrappedCard(kind: .noSpend, title: "No-spend days", figure: WrappedWords.count(noSpend.days), detail: detail, rows: rows)
	}

	private func goals() -> WrappedCard? {
		let ids = report.completedGoalIds
		guard !ids.isEmpty else { return nil }
		return WrappedCard(
			kind: .goals,
			title: ids.count == 1 ? "You reached 1 goal." : "You reached \(WrappedWords.count(ids.count)) goals.",
			rows: ids.compactMap { id in goalNames[id].map { WrappedCard.Row(id: id, label: $0, value: "") } }
		)
	}

	private func netWorth(_ redacted: Bool) -> WrappedCard {
		let change = report.netWorthChange
		let title = change > 0 ? "Net worth went up." : change < 0 ? "Net worth went down." : "Net worth held steady."
		if redacted {
			let figure = report.shares.netWorthChange.map { WrappedWords.signedPercent($0, format: format) } ?? WrappedWords.notApplicable
			return WrappedCard(kind: .netWorth, title: title, figure: figure, detail: "since the start of \(report.year)")
		}
		return WrappedCard(
			kind: .netWorth,
			title: title,
			figure: (change > 0 ? "+" : "") + money(change),
			rows: [
				WrappedCard.Row(id: "start", label: "Start of \(report.year)", value: money(report.netWorthStart)),
				WrappedCard.Row(id: "end", label: isYearToDate ? "Today" : "End of \(report.year)", value: money(report.netWorthEnd))
			]
		)
	}

	// MARK: Parts

	private var topCategoryShare: Int64? {
		for fact in report.funFacts {
			if case .topCategoryShare(_, let tenths) = fact { return tenths }
		}
		return nil
	}

	private var noSpendShare: Int64? {
		for fact in report.funFacts {
			if case .noSpendShare(_, _, let tenths) = fact { return tenths }
		}
		return nil
	}

	private func everythingElse(_ amount: Int64, share: Int64?, _ redacted: Bool) -> WrappedCard.Row? {
		if redacted {
			return share.map { WrappedCard.Row(id: "other", label: "Everything else", value: WrappedWords.percent($0, format: format)) }
		}
		return amount > 0 ? WrappedCard.Row(id: "other", label: "Everything else", value: money(amount)) : nil
	}

	private func money(_ amount: Int64) -> String {
		format.currency(amount, on: .register)
	}

	private func share(_ tenths: Int64?) -> String {
		tenths.map { WrappedWords.percent($0, format: format) } ?? WrappedWords.notApplicable
	}

	private func ofIncome(_ tenths: Int64?) -> String {
		tenths.map { "\(WrappedWords.percent($0, format: format)) of what came in" } ?? WrappedWords.notApplicable
	}

	/// The prior-year change FC-8 carries: a percentage, "New this year", or
	/// the "prior year from {date}" label when last year's figures start late.
	private func change(_ change: YearChange) -> String {
		switch change {
		case .newThisYear:
			return "New this year"
		case .basisPoints(let value):
			let amount = WrappedWords.signedBasisPoints(value, format: format)
			if let from = report.priorYear.from {
				return "\(amount) vs prior year from \(WrappedWords.day(from, withYear: true))"
			}
			return isYearToDate ? "\(amount) vs this point in \(report.year - 1)" : "\(amount) vs \(report.year - 1)"
		}
	}
}

/// Percentages, counts and dates as the Wrapped cards write them.
public enum WrappedWords {
	public static let notApplicable = "n/a"
	static let minus = "\u{2212}"

	/// Tenths of a percent: 430 is "43.0%", -3 is "−0.3%".
	public static func percent(_ tenths: Int64, format: MoneyFormat) -> String {
		let magnitude = tenths.magnitude
		return (tenths < 0 ? minus : "") + "\(grouped(magnitude / 10, format: format))\(format.decimalSeparator)\(magnitude % 10)%"
	}

	/// 1111 is "1,111" in the budget's own grouping; 123456 is "1,23,456"
	/// where groups go in twos after the thousands.
	static func grouped(_ value: UInt64, format: MoneyFormat) -> String {
		var digits = Array(String(value))
		var index = digits.count - 3
		while index > 0 {
			digits.insert(contentsOf: format.groupingSeparator, at: index)
			index -= format.groupsInTwosAfterThousands ? 2 : 3
		}
		return String(digits)
	}

	/// "+55.3%", "−4.0%", "0.0%".
	public static func signedPercent(_ tenths: Int64, format: MoneyFormat) -> String {
		(tenths > 0 ? "+" : "") + percent(tenths, format: format)
	}

	/// Basis points, exactly: 750 is "7.5%", 15000 is "150%", 755 is "7.55%".
	public static func basisPoints(_ value: Int64, format: MoneyFormat) -> String {
		let magnitude = value.magnitude
		var fraction = String(format: "%02d", Int(magnitude % 100))
		while fraction.hasSuffix("0") { fraction.removeLast() }
		let whole = grouped(magnitude / 100, format: format)
		let number = fraction.isEmpty ? whole : "\(whole)\(format.decimalSeparator)\(fraction)"
		return (value < 0 ? minus : "") + number + "%"
	}

	public static func signedBasisPoints(_ value: Int64, format: MoneyFormat) -> String {
		(value > 0 ? "+" : "") + basisPoints(value, format: format)
	}

	public static func count<T: BinaryInteger>(_ value: T) -> String {
		Int(value).formatted()
	}

	public static func days(_ value: UInt32) -> String {
		value == 1 ? "1 day" : "\(count(value)) days"
	}

	public static func payments(_ value: UInt64) -> String {
		value == 1 ? "1 payment" : "\(count(value)) payments"
	}

	/// "March 3", or "March 3, 2025".
	public static func day(_ text: String, withYear: Bool = false) -> String {
		guard let date = CivilDate.date(text) else { return text }
		return withYear ? date.formatted(.dateTime.month(.wide).day().year()) : date.formatted(.dateTime.month(.wide).day())
	}

	public static func monthName(_ month: Month) -> String {
		month.date()?.formatted(.dateTime.month(.wide)) ?? "\(month.month)"
	}
}
