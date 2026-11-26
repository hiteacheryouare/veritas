import Foundation
import VeritasCore

/// Wrapped as a year-in-review issue (design/wrapped-editorial.md): spreads of
/// facing pages, each page holding only finished text and the core's own
/// figures. Every number, date and name is a field of the core's
/// `WrappedReport`, formatted here (FC-8's copy rule). With `redacted`
/// (R-WRAP-2) no amount appears anywhere, only the core's shares, counts,
/// names and dates.
public struct WrappedIssue: Sendable {
	public let report: WrappedReport
	public let redacted: Bool
	public let spreads: [WrappedSpread]

	public init(report: WrappedReport, format: MoneyFormat, redacted: Bool) {
		self.report = report
		self.redacted = redacted
		spreads = WrappedIssueWriter(report: report, format: format, redacted: redacted).spreads()
	}

	public var pages: [WrappedPage] { spreads.flatMap(\.pages) }

	/// Every piece of text in the issue, in reading order.
	public var text: [String] { pages.flatMap(\.text) }

	/// The year to date rather than a whole year.
	public var isYearToDate: Bool { report.last != "\(report.year)-12-31" }

	public func spread(containing kind: WrappedPage.Kind) -> WrappedSpread? {
		spreads.first { $0.pages.contains { $0.kind == kind } }
	}
}

/// Two facing pages, or one page laid across both.
public struct WrappedSpread: Identifiable, Hashable, Sendable {
	public let index: Int
	public let pages: [WrappedPage]

	public var id: String { pages.map(\.kind.rawValue).joined(separator: "+") }
	public var isFull: Bool { pages.count == 1 }
	/// What the contents and the turn's spoken label call this spread.
	public var title: String { pages.first?.title ?? "" }
}

public struct WrappedPage: Identifiable, Hashable, Sendable {
	public enum Kind: String, Hashable, Sendable {
		case cover, contents, ledger, categories, groups, payees, paidDown, shape, quietDays, goals, netWorth, notes, backCover
	}

	/// A ranked or summary line: a label, its figure and an optional note.
	/// `share` is the core's tenths for the row's bar, the same in both modes.
	public struct Row: Identifiable, Hashable, Sendable {
		public let id: String
		public let label: String
		public let value: String
		public let note: String?
		public let share: Int64?
	}

	/// One part of a single proportion bar, by the core's tenths.
	public struct Segment: Hashable, Sendable {
		public let label: String
		public let share: Int64
	}

	/// One month of the monthly spending chart, drawn by the core's share of
	/// total out. A month without a share reads "n/a" in its slot.
	public struct Column: Identifiable, Hashable, Sendable {
		public let id: String
		/// 0 for January to 11 for December. Months after the period aren't in
		/// the series, so their slots stay empty.
		public let slot: Int
		public let name: String
		public let letter: String
		public let share: Int64?
		public let isBiggest: Bool
		public let isRefund: Bool
		public let spoken: String
	}

	/// The typical-month line across the chart, at the core's share of total out.
	public struct Reference: Hashable, Sendable {
		public let share: Int64
		public let label: String
	}

	/// One month end on the net worth line: an amount, or with amounts hidden
	/// the core's change over opening net worth in tenths.
	public struct Point: Identifiable, Hashable, Sendable {
		public let id: String
		public let slot: Int
		public let name: String
		public let value: Int64
		public let spoken: String
	}

	/// A label on a chart mark, at most two per chart.
	public struct Annotation: Hashable, Sendable {
		public enum Target: Hashable, Sendable {
			case column(slot: Int)
			case point(index: Int)
		}

		public let target: Target
		public let text: String
	}

	/// The quiet-days calendar: a date is no-spend (listed), spent (in the
	/// evaluated range, not listed) or not evaluated (outside it), FC-8 N3.
	public struct Calendar: Hashable, Sendable {
		public let year: Int32
		public let first: String?
		public let last: String?
		public let dates: Set<String>
		/// One line per month that has no-spend dates, for VoiceOver.
		public let months: [String]
	}

	public struct Entry: Identifiable, Hashable, Sendable {
		public let title: String
		public let page: Int
		public let spread: Int
		public var id: String { title }
	}

	public let kind: Kind
	public var number = 0
	public var title = ""
	public var heading: String?
	public var headline: String?
	public var dek: String?
	public var hero: String?
	public var rows: [Row] = []
	public var segments: [Segment] = []
	public var columns: [Column] = []
	public var reference: Reference?
	public var points: [Point] = []
	/// What the points measure, for the audio graph's axis.
	public var pointsTitle = ""
	public var annotations: [Annotation] = []
	public var calendar: Calendar?
	public var contents: [Entry] = []
	public var notes: [String] = []
	public var footnote: String?
	/// A chart's summary for VoiceOver.
	public var spoken: String?

	public init(kind: Kind) {
		self.kind = kind
	}

	public var id: String { kind.rawValue }

	public var text: [String] {
		[heading, headline, dek, hero].compactMap(\.self)
			+ rows.flatMap { [$0.label, $0.value, $0.note].compactMap(\.self) }
			+ segments.map(\.label)
			+ columns.map(\.spoken)
			+ [reference?.label].compactMap(\.self)
			+ points.map(\.spoken)
			+ annotations.map(\.text)
			+ (calendar?.months ?? [])
			+ contents.map(\.title)
			+ notes
			+ [footnote, spoken].compactMap(\.self)
	}
}

/// The words and the page order for one report.
private struct WrappedIssueWriter {
	let report: WrappedReport
	let format: MoneyFormat
	let redacted: Bool

	var isYearToDate: Bool { report.last != "\(report.year)-12-31" }
	var previousYear: Int32 { report.year - 1 }

	func spreads() -> [WrappedSpread] {
		guard report.hasData else {
			var cover = cover()
			cover.dek = "No data for \(report.year)."
			cover.number = 1
			return [WrappedSpread(index: 0, pages: [cover])]
		}
		var layout: [[WrappedPage]] = [[cover(), WrappedPage(kind: .contents)], [ledger()]]
		layout += pair(categories(), groups())
		layout += pair(payees(), paidDown())
		layout += [shape()].compactMap(\.self).map { [$0] }
		layout += pair(quietDays(), goals())
		layout.append([netWorth()])
		layout.append([notes(), backCover()])

		var number = 1
		var numbered: [[WrappedPage]] = []
		for pages in layout {
			numbered.append(pages.map { page in
				var page = page
				page.number = number
				number += 1
				return page
			})
			if pages.count == 1 { number += 1 }
		}
		let entries = numbered.enumerated().flatMap { index, pages in
			pages.filter { ![.cover, .contents, .backCover, .groups].contains($0.kind) }
				.map { WrappedPage.Entry(title: $0.title, page: $0.number, spread: index) }
		}
		let paidDownPage = numbered.joined().first { $0.kind == .paidDown }?.number
		return numbered.enumerated().map { index, pages in
			WrappedSpread(index: index, pages: pages.map { page in
				var page = page
				if page.kind == .contents { page.contents = entries }
				if page.kind == .ledger {
					page.footnote = "Debt paydown and savings aren't in \"went out\"." + (paidDownPage.map { " See page \($0)." } ?? "")
				}
				return page
			})
		}
	}

	/// Two pages share a spread; one alone takes the whole spread; none, no spread.
	private func pair(_ left: WrappedPage?, _ right: WrappedPage?) -> [[WrappedPage]] {
		let pages = [left, right].compactMap(\.self)
		return pages.isEmpty ? [] : [pages]
	}

	// MARK: Pages

	private func cover() -> WrappedPage {
		var page = WrappedPage(kind: .cover)
		page.title = String(report.year)
		page.headline = String(report.year)
		page.dek = [report.budgetName.isEmpty ? nil : report.budgetName, span].compactMap(\.self).joined(separator: "\n")
		return page
	}

	private var span: String {
		"\(WrappedWords.day(report.first)) to \(WrappedWords.day(report.last))"
	}

	private func ledger() -> WrappedPage {
		var page = WrappedPage(kind: .ledger)
		page.title = "The ledger"
		let prior = report.priorYear
		if redacted {
			// A refund-heavy year has a negative share, and "went out" can't be negative in words.
			page.headline = report.shares.outVsIn.flatMap { $0 >= 0 ? "\(percent($0)) of what came in went out." : nil } ?? "The ledger"
			page.dek = [report.shares.net.map(netShareSentence), partialPriorYear].compactMap(\.self).joined(separator: " ").nilIfEmpty
			page.rows = [
				change(prior.totalIn).map { WrappedPage.Row(id: "in", label: "Came in", value: $0, note: nil, share: nil) },
				WrappedPage.Row(id: "out", label: "Went out", value: ofIncome(report.shares.outVsIn), note: change(prior.totalOut), share: nil),
				WrappedPage.Row(id: "net", label: netLabel, value: ofIncome(report.shares.net.map(abs)), note: change(prior.net), share: nil)
			].compactMap(\.self)
		} else {
			page.headline = "\(money(report.totalIn)) came in.\n\(money(report.totalOut)) went out."
			page.dek = [netSentence, partialPriorYear].compactMap(\.self).joined(separator: " ")
			page.rows = [
				WrappedPage.Row(id: "in", label: "Came in", value: money(report.totalIn), note: change(prior.totalIn), share: nil),
				WrappedPage.Row(id: "out", label: "Went out", value: money(report.totalOut), note: change(prior.totalOut), share: nil),
				WrappedPage.Row(id: "net", label: netLabel, value: format.currencyMagnitude(report.net, on: .register), note: change(prior.net), share: nil)
			]
		}
		if let out = report.shares.outVsIn, let net = report.shares.net, out >= 0, net >= 0 {
			page.segments = [
				WrappedPage.Segment(label: "went out \(percent(out))", share: out),
				WrappedPage.Segment(label: "left over \(percent(net))", share: net)
			]
		}
		return page
	}

	private var netLabel: String {
		report.net < 0 ? "Came up short" : "Left over"
	}

	private var netSentence: String {
		guard report.net >= 0 else {
			return "That came up \(format.currencyMagnitude(report.net, on: .register)) short."
		}
		var sentence = "That left \(money(report.net))"
		if report.priorYear.from == nil, case .basisPoints(let value) = report.priorYear.net, value != 0 {
			let base = isYearToDate ? "at this point in \(previousYear)" : "in \(previousYear)"
			sentence += ", \(WrappedWords.basisPoints(abs(value), format: format)) \(value > 0 ? "more" : "less") than \(base)"
		}
		return sentence + "."
	}

	private func netShareSentence(_ tenths: Int64) -> String {
		tenths < 0 ? "That came up \(percent(abs(tenths))) short." : "\(percent(tenths)) was left over."
	}

	/// FC-8: a partial prior year is never compared. The page says so once, in place of every change.
	private var partialPriorYear: String? {
		report.priorYear.from.map { "No full prior year to compare. Your budget starts \(WrappedWords.day($0, withYear: true))." }
	}

	private func categories() -> WrappedPage? {
		guard let top = report.topCategories.first else { return nil }
		var page = WrappedPage(kind: .categories)
		page.title = "Where it went"
		page.headline = "\(ReportWords.category(top.name, labels: top.labels)) took the largest share."
		if redacted {
			page.dek = "\(percent(top.share)) of categorized spending."
		} else {
			page.dek = topCategoryShare.map { "\(money(top.total)), \(percent($0)) of categorized spending." } ?? "\(money(top.total))."
		}
		page.rows = report.topCategories.map { category in
			WrappedPage.Row(
				id: category.categoryId,
				label: ReportWords.category(category.name, labels: category.labels),
				value: redacted ? percent(category.share) : money(category.total),
				note: nil,
				share: category.share
			)
		}
		page.rows += everythingElse(report.otherCategories, share: report.shares.otherCategories)
		return page
	}

	private func groups() -> WrappedPage? {
		guard let top = report.topGroups.first, report.topGroups.count + (report.otherGroups > 0 ? 1 : 0) > 1 else { return nil }
		var page = WrappedPage(kind: .groups)
		page.title = "Groups"
		page.heading = "Groups"
		page.dek = redacted
			? "\(top.name) was the biggest group, at \(percent(top.share)) of categorized spending."
			: "\(top.name) was the biggest group, at \(money(top.total))."
		page.rows = report.topGroups.map { group in
			WrappedPage.Row(id: group.groupId, label: group.name, value: redacted ? percent(group.share) : money(group.total), note: nil, share: group.share)
		}
		page.rows += everythingElse(report.otherGroups, share: report.shares.otherGroups)
		return page
	}

	private func payees() -> WrappedPage? {
		guard let top = report.topPayees.first else { return nil }
		var page = WrappedPage(kind: .payees)
		page.title = "Who you paid"
		page.headline = "You paid \(top.name) the most."
		let payments = WrappedWords.payments(top.count)
		page.dek = redacted
			? "\(percent(top.share)) of what went to payees, across \(payments)."
			: "\(money(top.total)) across \(payments)."
		page.rows = report.topPayees.map { payee in
			WrappedPage.Row(
				id: payee.payeeId,
				label: payee.name,
				value: redacted ? percent(payee.share) : money(payee.total),
				note: WrappedWords.payments(payee.count),
				share: payee.share
			)
		}
		page.rows += everythingElse(report.otherPayees, share: report.shares.otherPayees)
		return page
	}

	private func paidDown() -> WrappedPage? {
		guard report.debtPaydown != 0 || report.savings != 0 else { return nil }
		var page = WrappedPage(kind: .paidDown)
		page.title = "Paid down and saved"
		page.heading = "Paid down and saved"
		var parts: [String] = []
		if report.debtPaydown != 0 {
			let figure = redacted ? ofIncome(report.shares.debtPaydown) : money(report.debtPaydown)
			parts.append("\(figure) to debt")
			page.rows.append(WrappedPage.Row(id: "debt", label: "Debt paydown", value: figure, note: nil, share: nil))
		}
		if report.savings != 0 {
			let figure = redacted ? ofIncome(report.shares.savings) : money(report.savings)
			parts.append("\(figure) to savings")
			page.rows.append(WrappedPage.Row(id: "savings", label: "Savings", value: figure, note: nil, share: nil))
		}
		let neither = parts.count == 2 ? "Neither is counted in what went out." : "It isn't counted in what went out."
		page.dek = parts.joined(separator: " and ") + ". " + neither
		return page
	}

	private func shape() -> WrappedPage? {
		guard let biggest = report.biggestMonth else { return nil }
		var page = WrappedPage(kind: .shape)
		page.title = "The shape of the year"
		let name = WrappedWords.monthName(biggest.month)
		page.headline = "\(name) was the biggest month for spending."
		// A biggest month that is the typical month (one month of spending) has nothing to compare.
		let typical = report.biggestVsTypical.flatMap { $0 > 0 ? "\(WrappedWords.basisPoints($0, format: format)) more than your typical month" : nil }
		if redacted {
			let share = report.shares.biggestMonth.map { "\(percent($0)) \(isYearToDate ? "of spending so far" : "of the year's spending")" }
			page.dek = [share, typical].compactMap(\.self).joined(separator: ", ").nilIfEmpty.map { $0.prefix(1).uppercased() + $0.dropFirst() + "." }
		} else {
			page.dek = [money(biggest.total), typical].compactMap(\.self).joined(separator: ", ") + "."
		}
		page.columns = report.months.map { month in
			let monthName = WrappedWords.monthName(month.month)
			let isRefund = month.share.map { $0 < 0 } ?? (month.total < 0)
			let spoken: String = switch (redacted, month.share) {
			case (true, nil): "\(monthName), \(WrappedWords.notApplicable)"
			case (true, let share?) where share < 0: "\(monthName), refund month"
			case (true, let share?): "\(monthName), \(percent(share)) of total out"
			case (false, _): isRefund ? "\(monthName), refund month, \(money(month.total))" : "\(monthName), \(money(month.total))"
			}
			return WrappedPage.Column(
				id: "\(month.month.year)-\(month.month.month)",
				slot: Int(month.month.month) - 1,
				name: monthName,
				letter: month.month.date()?.formatted(.dateTime.month(.narrow)) ?? "",
				share: month.share,
				isBiggest: month.month == biggest.month,
				isRefund: isRefund,
				spoken: spoken
			)
		}
		let biggestSlot = Int(biggest.month.month) - 1
		let biggestFigure = redacted ? report.shares.biggestMonth.map(percent) : money(biggest.total)
		page.annotations.append(WrappedPage.Annotation(target: .column(slot: biggestSlot), text: [name, biggestFigure].compactMap(\.self).joined(separator: "  ")))
		if let refund = page.columns.first(where: \.isRefund) {
			page.annotations.append(WrappedPage.Annotation(target: .column(slot: refund.slot), text: "Refund month"))
		}
		if let amount = report.typicalMonth, amount > 0, let share = report.shares.typicalMonth {
			page.reference = WrappedPage.Reference(share: share, label: redacted ? "typical month" : "typical month \(money(amount))")
		}
		page.spoken = "Spending by month. " + page.columns.map(\.spoken).joined(separator: "; ")
		return page
	}

	private func quietDays() -> WrappedPage? {
		let noSpend = report.noSpend
		guard noSpend.evaluatedDays > 0, let first = noSpend.first, let last = noSpend.last else { return nil }
		var page = WrappedPage(kind: .quietDays)
		page.title = "Quiet days"
		page.headline = noSpend.days == 1 ? "1 no-spend day." : "\(WrappedWords.count(noSpend.days)) no-spend days."
		let range = "\(WrappedWords.day(first)) to \(WrappedWords.day(last))"
		var sentences = [noSpendShare.map { "\(percent($0)) of the \(WrappedWords.count(noSpend.evaluatedDays)) days from \(range)." }
			?? "Out of \(WrappedWords.count(noSpend.evaluatedDays)) days from \(range)."]
		if let longest = noSpend.longest {
			sentences.append("The longest run was \(WrappedWords.days(longest.days)), from \(WrappedWords.day(longest.start)).")
		}
		page.dek = sentences.joined(separator: " ")
		let byMonth = Dictionary(grouping: noSpend.dates) { String($0.prefix(7)) }
		page.calendar = WrappedPage.Calendar(
			year: report.year,
			first: first,
			last: last,
			dates: Set(noSpend.dates),
			months: byMonth.keys.sorted().compactMap { key in
				guard let dates = byMonth[key], let month = Month(date: "\(key)-01") else { return nil }
				let days = dates.sorted().compactMap { Int($0.suffix(2)) }.map(String.init).joined(separator: ", ")
				return "\(WrappedWords.monthName(month)): no-spend days \(days)"
			}
		)
		page.spoken = [page.headline, page.dek].compactMap(\.self).joined(separator: " ")
		return page
	}

	private func goals() -> WrappedPage? {
		let reached = report.reachedGoals
		guard !reached.isEmpty else { return nil }
		var page = WrappedPage(kind: .goals)
		page.title = "Goals reached"
		page.headline = reached.count == 1 ? "You reached 1 goal." : "You reached \(WrappedWords.count(reached.count)) goals."
		page.rows = reached.map { goal in
			WrappedPage.Row(
				id: goal.goalId,
				label: Self.goalLabel(goal),
				value: WrappedWords.monthName(goal.month),
				note: nil,
				share: nil
			)
		}
		return page
	}

	/// ST-10.3: a deleted category keeps its name after "(deleted)", and one
	/// the core has no name for at all says only that.
	static func goalLabel(_ goal: WrappedReachedGoal) -> String {
		guard goal.deleted else { return goal.name }
		return goal.name.isEmpty ? "(deleted category)" : "(deleted) \(goal.name)"
	}

	private func netWorth() -> WrappedPage {
		var page = WrappedPage(kind: .netWorth)
		page.title = "Net worth"
		page.pointsTitle = redacted ? "Change since the start of \(report.year)" : "Net worth"
		let change = report.netWorthChange
		page.headline = change > 0 ? "Net worth went up." : change < 0 ? "Net worth went down." : "Net worth held steady."
		let ends = report.monthEnds
		if redacted {
			page.hero = report.shares.netWorthChange.map { WrappedWords.signedPercent($0, format: format) } ?? WrappedWords.notApplicable
			page.dek = report.shares.netWorthChange == nil ? nil : "Since the start of \(report.year)."
			let shares = ends.compactMap(\.share)
			if shares.count == ends.count, !ends.isEmpty {
				page.points = ends.enumerated().map { index, end in
					WrappedPage.Point(id: end.date, slot: Int(end.month.month) - 1, name: WrappedWords.monthName(end.month), value: shares[index], spoken: "\(WrappedWords.monthName(end.month)), \(WrappedWords.signedPercent(shares[index], format: format))")
				}
			}
		} else {
			page.hero = (change > 0 ? "+" : "") + money(change)
			page.rows = [
				WrappedPage.Row(id: "start", label: "Start of \(report.year)", value: money(report.netWorthStart), note: nil, share: nil),
				WrappedPage.Row(id: "end", label: isYearToDate ? "Today" : "End of \(report.year)", value: money(report.netWorthEnd), note: nil, share: nil)
			]
			page.points = ends.map { end in
				WrappedPage.Point(id: end.date, slot: Int(end.month.month) - 1, name: WrappedWords.monthName(end.month), value: end.netWorth, spoken: "\(WrappedWords.monthName(end.month)), \(money(end.netWorth))")
			}
		}
		if let last = ends.last, !page.points.isEmpty {
			let figure = redacted ? last.share.map { WrappedWords.signedPercent($0, format: format) } : money(last.netWorth)
			page.annotations.append(WrappedPage.Annotation(target: .point(index: ends.count - 1), text: [figure, WrappedWords.day(last.date)].compactMap(\.self).joined(separator: "  ")))
			if let lowest = report.lowestMonthEnd, lowest.month != last.month, let index = ends.firstIndex(where: { $0.month == lowest.month }) {
				let figure = redacted ? lowest.share.map { WrappedWords.signedPercent($0, format: format) } : money(lowest.netWorth)
				page.annotations.append(WrappedPage.Annotation(target: .point(index: index), text: ["Lowest month end, \(WrappedWords.monthName(lowest.month))", figure].compactMap(\.self).joined(separator: "  ")))
			}
			page.spoken = "Net worth at each month end. " + page.points.map(\.spoken).joined(separator: "; ")
		}
		return page
	}

	private func notes() -> WrappedPage {
		var page = WrappedPage(kind: .notes)
		page.title = "Notes"
		page.heading = "How these figures are counted"
		page.notes = [
			"These pages cover \(WrappedWords.day(report.first, withYear: true)) to \(WrappedWords.day(report.last, withYear: true)).",
			"\"Went out\" is spending after refunds. Debt paydown and savings are counted separately.",
			"Categories and groups rank by spending after refunds. Uncategorized spending isn't ranked.",
			"Payees rank by what was paid to them. Transfers aren't payees.",
			"A no-spend day is a day with no discretionary spending. Bills, loan payments, refunds, and transfers, card payments and savings moves between your own accounts don't count against it. A transfer recorded as spending does.",
			"Net worth at the start is the balance at the end of the day before January 1.",
			"Percentages compare the same dates a year earlier."
		]
		return page
	}

	private func backCover() -> WrappedPage {
		var page = WrappedPage(kind: .backCover)
		page.title = String(report.year)
		page.headline = String(report.year)
		page.dek = report.budgetName.nilIfEmpty
		return page
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

	private func everythingElse(_ amount: Int64, share: Int64?) -> [WrappedPage.Row] {
		if redacted {
			return share.map { [WrappedPage.Row(id: "other", label: "Everything else", value: percent($0), note: nil, share: $0)] } ?? []
		}
		return amount > 0 ? [WrappedPage.Row(id: "other", label: "Everything else", value: money(amount), note: nil, share: share)] : []
	}

	private func money(_ amount: Int64) -> String {
		format.currency(amount, on: .register)
	}

	private func percent(_ tenths: Int64) -> String {
		WrappedWords.percent(tenths, format: format)
	}

	private func ofIncome(_ tenths: Int64?) -> String {
		tenths.map { "\(percent($0)) of what came in" } ?? WrappedWords.notApplicable
	}

	/// The prior-year change FC-8 carries: a percentage or "New this year".
	/// Nothing when last year's figures start late: `partialPriorYear` says why.
	private func change(_ change: YearChange) -> String? {
		guard report.priorYear.from == nil else { return nil }
		switch change {
		case .newThisYear:
			return "New this year"
		case .basisPoints(let value):
			let amount = WrappedWords.signedBasisPoints(value, format: format)
			return isYearToDate ? "\(amount) vs this point in \(previousYear)" : "\(amount) vs \(previousYear)"
		}
	}
}

private extension String {
	var nilIfEmpty: String? { isEmpty ? nil : self }
}
