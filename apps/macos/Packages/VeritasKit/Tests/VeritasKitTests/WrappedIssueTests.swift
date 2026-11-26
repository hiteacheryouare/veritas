import Foundation
import RegexBuilder
import Testing
import VeritasCore
@testable import VeritasKit

/// Wrapped's editorial issue from a core report (R-WRAP-1,
/// design/wrapped-editorial.md): the spreads and their order, every page's
/// words, FC-8's copy rule (every figure is a report field), and R-WRAP-2's
/// redaction (with amounts hidden no page carries a money amount).
struct WrappedIssueTests {
	static let format = MoneyFormat()
	static let none = ReportLabels(hidden: false, archived: false, deleted: false, closedGoal: false, needsReview: false)

	/// FC-8's W-14 categories, W-8 and W-24's net worth, W-16's refunds-beat-spending out,
	/// W-28's typical month, W-33's no-spend dates and W-35's goals.
	static func report(last: String = "2027-12-31", prior: WrappedPriorYear? = nil) -> WrappedReport {
		let ends: [(UInt32, Int64, Int64)] = [(1, 735_000, 21), (2, 702_000, -25), (3, 760_000, 56), (4, 1_118_300, 553)]
		let monthEnds = ends.map { month, value, share in
			WrappedMonthEnd(month: Month(year: 2027, month: month), date: month == 4 ? last : "2027-0\(month)-28", netWorth: value, share: share)
		}
		return WrappedReport(
			year: 2027, first: "2027-01-01", last: last, hasData: true,
			totalIn: 100_000, totalOut: 115_000, net: -15_000, debtPaydown: 12_000, savings: 0,
			topCategories: [
				WrappedCategory(categoryId: "groceries", name: "Groceries", total: 43_000, labels: none, share: 374),
				WrappedCategory(categoryId: "dining", name: "Dining", total: 30_000, labels: none, share: 261),
				WrappedCategory(categoryId: "fuel", name: "Fuel", total: 17_000, labels: none, share: 148),
				WrappedCategory(categoryId: "fun", name: "Fun", total: 10_000, labels: none, share: 87),
				WrappedCategory(categoryId: "books", name: "Old Books", total: 6_000, labels: ReportLabels(hidden: true, archived: false, deleted: false, closedGoal: false, needsReview: false), share: 52)
			],
			otherCategories: 9_000,
			topGroups: [WrappedGroup(groupId: "food", name: "Food", total: 73_000, share: 635), WrappedGroup(groupId: "life", name: "Life", total: 42_000, share: 365)],
			otherGroups: 0,
			topPayees: [WrappedPayee(payeeId: "market", name: "Market", total: 40_000, count: 12, share: 800), WrappedPayee(payeeId: "cafe", name: "Cafe", total: 10_000, count: 1, share: 200)],
			otherPayees: 0,
			biggestMonth: WrappedMonth(month: Month(year: 2027, month: 3), total: 30_000),
			biggestVsTypical: 5_600,
			noSpend: WrappedNoSpend(first: "2027-01-05", last: "2027-12-31", days: 5, evaluatedDays: 16, longest: WrappedStreak(start: "2027-01-07", days: 2), current: nil,
				dates: ["2027-01-05", "2027-01-07", "2027-01-08", "2027-01-11", "2027-01-14"]),
			completedGoalIds: ["laptop", "trip"],
			netWorthStart: 720_000, netWorthEnd: 1_118_300, netWorthChange: 398_300,
			priorYear: prior ?? WrappedPriorYear(totalIn: .basisPoints(value: 750), totalOut: .newThisYear, net: .basisPoints(value: 15_000), from: nil),
			shares: WrappedShares(outVsIn: 1_150, net: -150, debtPaydown: 120, savings: 0, biggestMonth: 261, netWorthChange: 553, otherCategories: 78, otherGroups: nil, otherPayees: nil, typicalMonth: 247),
			funFacts: [
				.priorYear(measure: .totalIn, basisPoints: 750),
				.biggestMonthVsTypical(month: Month(year: 2027, month: 3), basisPoints: 5_600),
				.topCategoryShare(categoryId: "groceries", tenths: 374),
				.noSpendShare(days: 5, evaluatedDays: 16, tenths: 313)
			],
			months: [
				WrappedSpendingMonth(month: Month(year: 2027, month: 1), total: 20_000, share: 174),
				WrappedSpendingMonth(month: Month(year: 2027, month: 2), total: -5_000, share: -43),
				WrappedSpendingMonth(month: Month(year: 2027, month: 3), total: 30_000, share: 261),
				WrappedSpendingMonth(month: Month(year: 2027, month: 4), total: 0, share: 0)
			],
			budgetName: "Household",
			monthEnds: monthEnds,
			lowestMonthEnd: monthEnds[1],
			highestMonthEnd: monthEnds[3],
			typicalMonth: 28_400,
			reachedGoals: [
				WrappedReachedGoal(goalId: "laptop", name: "New laptop", deleted: false, month: Month(year: 2027, month: 2)),
				WrappedReachedGoal(goalId: "trip", name: "Trip", deleted: true, month: Month(year: 2027, month: 7))
			]
		)
	}

	static func issue(_ report: WrappedReport = report(), redacted: Bool = false) -> WrappedIssue {
		WrappedIssue(report: report, format: format, redacted: redacted)
	}

	static func page(_ kind: WrappedPage.Kind, _ report: WrappedReport = report(), redacted: Bool = false) -> WrappedPage? {
		issue(report, redacted: redacted).pages.first { $0.kind == kind }
	}

	/// A money amount as any format writes one: the currency symbol, or digits
	/// with two decimal places that aren't a percentage.
	static func moneyFigures(in text: String, format: MoneyFormat = format) -> [String] {
		var found: [String] = []
		if text.contains(format.currencySymbol) { found.append(text) }
		let separator = Regex<Substring>(verbatim: format.decimalSeparator)
		let amount = Regex { One(.digit); separator; One(.digit); One(.digit); NegativeLookahead { CharacterClass(.digit, .anyOf("%")) } }
		if text.firstMatch(of: amount) != nil { found.append(text) }
		return found
	}

	// MARK: The issue

	@Test func theSpreadsRunInTheDesignsOrder() {
		let spreads = Self.issue().spreads.map { $0.pages.map(\.kind) }
		#expect(spreads == [
			[.cover, .contents], [.ledger], [.categories, .groups], [.payees, .paidDown],
			[.shape], [.quietDays, .goals], [.netWorth], [.notes, .backCover]
		])
		let numbers = Self.issue().spreads.map { $0.pages.map(\.number) }
		#expect(numbers == [[1, 2], [3], [5, 6], [7, 8], [9], [11, 12], [13], [15, 16]], "a full spread takes two page numbers")
	}

	@Test func aMissingPageGivesItsPartnerTheWholeSpread() {
		var report = Self.report()
		report.debtPaydown = 0
		report.reachedGoals = []
		report.topGroups = []
		let spreads = Self.issue(report).spreads.map { $0.pages.map(\.kind) }
		#expect(spreads.contains([.categories]))
		#expect(spreads.contains([.payees]))
		#expect(spreads.contains([.quietDays]))
		#expect(!spreads.joined().contains(.paidDown))
		#expect(!spreads.joined().contains(.goals))

		report.topPayees = []
		#expect(!Self.issue(report).spreads.contains { $0.pages.contains { $0.kind == .payees } }, "neither page: no spread")
	}

	@Test func theContentsListEveryPageThatExistsWithItsNumber() {
		let contents = Self.page(.contents)?.contents ?? []
		#expect(contents.map(\.title) == ["The ledger", "Where it went", "Who you paid", "Paid down and saved", "The shape of the year", "Quiet days", "Goals reached", "Net worth", "Notes"])
		#expect(contents.map(\.page) == [3, 5, 7, 8, 9, 11, 12, 13, 15])
		#expect(contents.map(\.spread) == [1, 2, 3, 3, 4, 5, 5, 6, 7])
		#expect(Self.page(.ledger)?.footnote == "Debt paydown and savings aren't in \"went out\". See page 8.")
	}

	@Test func anEmptyYearIsTheCoverAlone() {
		var report = Self.report()
		report.hasData = false
		let issue = Self.issue(report)
		#expect(issue.spreads.count == 1)
		#expect(issue.pages.map(\.kind) == [.cover])
		#expect(issue.pages.first?.dek == "No data for 2027.")
	}

	// MARK: Words

	@Test func amountsShowTheCoresFigures() {
		let ledger = Self.page(.ledger)
		#expect(ledger?.headline == "$1,000.00 came in.\n$1,150.00 went out.")
		#expect(ledger?.dek == "That came up $150.00 short.")
		#expect(ledger?.rows.map(\.label) == ["Came in", "Went out", "Came up short"], "a deficit says so in words")
		#expect(ledger?.rows.map(\.note) == ["+7.5% vs 2026", "New this year", "+150% vs 2026"])
		#expect(ledger?.segments.isEmpty == true, "out is more than in, so there is no bar to split")
		#expect(Self.page(.cover)?.dek == "Household\nJanuary 1 to December 31")
		#expect(Self.page(.categories)?.headline == "Groceries took the largest share.")
		#expect(Self.page(.categories)?.dek == "$430.00, 37.4% of categorized spending.")
		#expect(Self.page(.categories)?.rows.map(\.label) == ["Groceries", "Dining", "Fuel", "Fun", "Old Books (hidden)", "Everything else"])
		#expect(Self.page(.payees)?.dek == "$400.00 across 12 payments.")
		#expect(Self.page(.payees)?.rows.map(\.note) == ["12 payments", "1 payment"])
		#expect(Self.page(.paidDown)?.dek == "$120.00 to debt. It isn't counted in what went out.")
		#expect(Self.page(.shape)?.headline == "March was the biggest month for spending.")
		#expect(Self.page(.shape)?.dek == "$300.00, 56% more than your typical month.")
		#expect(Self.page(.shape)?.reference?.label == "typical month $284.00")
		#expect(Self.page(.goals)?.rows.map(\.label) == ["New laptop", "(deleted) Trip"])
		#expect(Self.page(.goals)?.rows.map(\.value) == ["February", "July"])
		#expect(Self.page(.netWorth)?.headline == "Net worth went up.")
		#expect(Self.page(.netWorth)?.hero == "+$3,983.00")
		#expect(Self.page(.netWorth)?.rows.map(\.label) == ["Start of 2027", "End of 2027"])
	}

	@Test func aSurplusYearSaysWhatWasLeftAndComparesWithLastYear() {
		var report = Self.report()
		report.net = 30_000
		report.shares.outVsIn = 700
		report.shares.net = 300
		#expect(Self.page(.ledger, report)?.dek == "That left $300.00, 150% more than in 2026.")
		#expect(Self.page(.ledger, report)?.segments.map(\.label) == ["went out 70.0%", "left over 30.0%"])
		report.priorYear.net = .basisPoints(value: -320)
		#expect(Self.page(.ledger, report)?.dek == "That left $300.00, 3.2% less than in 2026.")
	}

	@Test func theShapeOfTheYearDrawsTheCoresMonthsByShare() {
		// v18, FC-8: a column per month the core sends, by its share; a refund
		// month stays negative, and only the biggest month is annotated with a figure.
		let shown = Self.page(.shape)
		#expect(shown?.columns.map(\.share) == [174, -43, 261, 0])
		#expect(shown?.columns.map(\.slot) == [0, 1, 2, 3])
		#expect(shown?.columns.map(\.isBiggest) == [false, false, true, false])
		#expect(shown?.columns.map(\.isRefund) == [false, true, false, false])
		#expect(shown?.annotations.map(\.text) == ["March  $300.00", "Refund month"])
		#expect(shown?.columns.map(\.spoken) == ["January, $200.00", "February, refund month, \u{2212}$50.00", "March, $300.00", "April, $0.00"])

		let hidden = Self.page(.shape, redacted: true)
		#expect(hidden?.columns.map(\.share) == [174, -43, 261, 0], "the same columns with amounts hidden")
		#expect(hidden?.annotations.map(\.text) == ["March  26.1%", "Refund month"])
		#expect(hidden?.columns.first?.spoken == "January, 17.4% of total out")
		#expect(hidden?.reference?.label == "typical month")
	}

	@Test func theTypicalMonthLineNeedsAPositiveTypicalMonth() {
		var report = Self.report()
		report.typicalMonth = -5_000
		#expect(Self.page(.shape, report)?.reference == nil, "W-32")
		report.typicalMonth = 28_400
		report.shares.typicalMonth = nil
		#expect(Self.page(.shape, report)?.reference == nil)
	}

	@Test func netWorthDrawsTheMonthEndsAndMarksTheLowest() {
		let shown = Self.page(.netWorth)
		#expect(shown?.points.map(\.value) == [735_000, 702_000, 760_000, 1_118_300])
		#expect(shown?.annotations.map(\.text) == ["$11,183.00  December 31", "Lowest month end, February  $7,020.00"])
		let hidden = Self.page(.netWorth, redacted: true)
		#expect(hidden?.points.map(\.value) == [21, -25, 56, 553], "N1's tenths")
		#expect(hidden?.hero == "+55.3%")
		#expect(hidden?.annotations.map(\.text) == ["+55.3%  December 31", "Lowest month end, February  \u{2212}2.5%"])
	}

	@Test func aRedactedNetWorthWithNoOpeningShareReadsNAAndDrawsNoLine() {
		// N1: opening net worth of 0 or less gives no share, so the page shows "n/a" and no line.
		var report = Self.report()
		report.shares.netWorthChange = nil
		report.monthEnds = report.monthEnds.map { WrappedMonthEnd(month: $0.month, date: $0.date, netWorth: $0.netWorth, share: nil) }
		let hidden = Self.page(.netWorth, report, redacted: true)
		#expect(hidden?.hero == "n/a")
		#expect(hidden?.dek == nil)
		#expect(hidden?.points.isEmpty == true)
		#expect(hidden?.annotations.isEmpty == true)
		#expect(Self.page(.netWorth, report)?.points.count == 4, "with amounts shown the line is drawn from the amounts")
	}

	@Test func quietDaysUseTheCoresDates() {
		for redacted in [false, true] {
			let page = Self.page(.quietDays, redacted: redacted)
			#expect(page?.headline == "5 no-spend days.")
			#expect(page?.dek == "31.3% of the 16 days from January 5 to December 31. The longest run was 2 days, from January 7.")
			#expect(page?.calendar?.dates.count == 5)
			#expect(page?.calendar?.months == ["January: no-spend days 5, 7, 8, 11, 14"])
		}
	}

	/// FC-8: bills and loan lines don't break a no-spend day, so no page says nothing was spent.
	@Test func noPageSaysNothingWasSpent() {
		for redacted in [false, true] {
			for line in Self.issue(redacted: redacted).text {
				#expect(!line.localizedCaseInsensitiveContains("spent nothing"), "\(line)")
				#expect(!line.localizedCaseInsensitiveContains("nothing spent"), "\(line)")
			}
		}
	}

	@Test func theYearToDateSaysSoAndComparesWithTheSameDays() {
		let issue = Self.issue(Self.report(last: "2027-04-10"))
		#expect(issue.isYearToDate)
		#expect(issue.pages.first?.dek == "Household\nJanuary 1 to April 10")
		#expect(issue.pages.first { $0.kind == .ledger }?.rows.first?.note == "+7.5% vs this point in 2026")
		#expect(issue.pages.first { $0.kind == .netWorth }?.rows.last?.label == "Today")
		let hidden = Self.issue(Self.report(last: "2027-04-10"), redacted: true)
		#expect(hidden.pages.first { $0.kind == .shape }?.dek == "26.1% of spending so far, 56% more than your typical month.")
	}

	@Test func aPartialPriorYearIsNeverCompared() {
		// FC-8 Prior-year display: the ledger says why once, and no change percentage appears.
		let prior = WrappedPriorYear(totalIn: .basisPoints(value: 53_073), totalOut: .basisPoints(value: 92_153), net: .newThisYear, from: "2025-11-01")
		for redacted in [false, true] {
			let ledger = Self.page(.ledger, Self.report(prior: prior), redacted: redacted)
			let words = (ledger?.text ?? []).joined(separator: " ")
			#expect(!words.contains("vs"), "\(words)")
			#expect(!words.contains("New this year"), "\(words)")
			#expect(!words.contains("530") && !words.contains("921"), "\(words)")
			#expect(ledger?.dek?.contains("No full prior year to compare. Your budget starts November 1, 2025.") == true, "\(ledger?.dek ?? "")")
			#expect(ledger?.rows.allSatisfy { $0.note == nil } == true)
		}
	}

	// MARK: Hide amounts (R-WRAP-2)

	@Test func redactedPagesCarryNoMoneyAmount() {
		let report = Self.report()
		let text = Self.issue(report, redacted: true).text
		#expect(!text.isEmpty)
		for line in text {
			#expect(Self.moneyFigures(in: line).isEmpty, "\(line)")
		}
		let amounts = [report.totalIn, report.totalOut, report.net, report.debtPaydown, report.netWorthStart, report.netWorthEnd, report.netWorthChange, report.otherCategories, report.typicalMonth ?? 0]
			+ report.topCategories.map(\.total) + report.topGroups.map(\.total) + report.topPayees.map(\.total) + [report.biggestMonth?.total ?? 0]
			+ report.monthEnds.map(\.netWorth) + report.months.map(\.total)
		for amount in amounts where amount != 0 {
			for line in text {
				#expect(!line.contains(Self.format.amount(amount, on: .register)), "\(amount) in \(line)")
			}
		}
	}

	@Test func redactedPagesShowTheCoresShares() {
		let ledger = Self.page(.ledger, redacted: true)
		#expect(ledger?.headline == "115.0% of what came in went out.")
		#expect(ledger?.dek == "That came up 15.0% short.")
		#expect(ledger?.rows.map(\.value) == ["+7.5% vs 2026", "115.0% of what came in", "15.0% of what came in"])
		let categories = Self.page(.categories, redacted: true)
		#expect(categories?.dek == "37.4% of categorized spending.")
		#expect(categories?.rows.map(\.value) == ["37.4%", "26.1%", "14.8%", "8.7%", "5.2%", "7.8%"])
		#expect(Self.page(.groups, redacted: true)?.rows.map(\.value) == ["63.5%", "36.5%"], "no Everything else when the core has none")
		#expect(Self.page(.payees, redacted: true)?.dek == "80.0% of what went to payees, across 12 payments.")
		#expect(Self.page(.paidDown, redacted: true)?.rows.map(\.value) == ["12.0% of what came in"])
		let shownRows = Self.page(.categories)?.rows.map(\.share)
		#expect(categories?.rows.map(\.share) == shownRows, "the bars are the same either way")
	}

	@Test func aShareTheCoreLeavesOutReadsNA() {
		var report = Self.report()
		report.shares.outVsIn = nil
		report.shares.net = nil
		let ledger = Self.page(.ledger, report, redacted: true)
		#expect(ledger?.rows.dropFirst().map(\.value) == ["n/a", "n/a"])
		#expect(ledger?.headline == "The ledger", "FC-8 copy rule 3: no sentence about an absent figure")
		#expect(ledger?.dek == nil)
	}

	// MARK: FC-8's copy rule

	/// Every way the issue may write a report field, and the copy's own
	/// constants (copy rule 1). What's left of a line once these are taken out
	/// must hold no digit.
	static func allowedFigures(_ report: WrappedReport, _ issue: WrappedIssue) -> [String] {
		let format = Self.format
		var amounts = [report.totalIn, report.totalOut, report.net, report.debtPaydown, report.savings, report.otherCategories, report.otherGroups, report.otherPayees,
			report.netWorthStart, report.netWorthEnd, report.netWorthChange, report.biggestMonth?.total, report.typicalMonth].compactMap(\.self)
		amounts += report.topCategories.map(\.total) + report.topGroups.map(\.total) + report.topPayees.map(\.total)
		amounts += report.months.map(\.total) + report.monthEnds.map(\.netWorth)
		var shares = [report.shares.outVsIn, report.shares.net, report.shares.debtPaydown, report.shares.savings, report.shares.biggestMonth, report.shares.netWorthChange,
			report.shares.otherCategories, report.shares.otherGroups, report.shares.otherPayees, report.shares.typicalMonth].compactMap(\.self)
		shares += report.topCategories.map(\.share) + report.topGroups.map(\.share) + report.topPayees.map(\.share)
		shares += report.months.compactMap(\.share) + report.monthEnds.compactMap(\.share)
		for fact in report.funFacts {
			switch fact {
			case .topCategoryShare(_, let tenths), .noSpendShare(_, _, let tenths): shares.append(tenths)
			default: break
			}
		}
		var basisPoints = [report.biggestVsTypical].compactMap(\.self)
		for change in [report.priorYear.totalIn, report.priorYear.totalOut, report.priorYear.net] {
			if case .basisPoints(let value) = change { basisPoints.append(value) }
		}
		var dates = [report.first, report.last, report.noSpend.first, report.noSpend.last, report.noSpend.longest?.start, report.priorYear.from].compactMap(\.self)
		dates += report.monthEnds.map(\.date)
		var figures = amounts.flatMap { [format.currency($0, on: .register), format.currencyMagnitude($0, on: .register), format.amount($0, on: .register), "+" + format.currency($0, on: .register)] }
		figures += shares.flatMap { [WrappedWords.percent($0, format: format), WrappedWords.percent(abs($0), format: format), WrappedWords.signedPercent($0, format: format)] }
		figures += basisPoints.flatMap { [WrappedWords.basisPoints($0, format: format), WrappedWords.basisPoints(abs($0), format: format), WrappedWords.signedBasisPoints($0, format: format)] }
		figures += dates.flatMap { [WrappedWords.day($0), WrappedWords.day($0, withYear: true)] }
		figures += report.noSpend.dates.compactMap { Int($0.suffix(2)) }.map(String.init)
		figures += [report.noSpend.days, report.noSpend.evaluatedDays].map { WrappedWords.count($0) } + [report.noSpend.longest?.days].compactMap(\.self).map(WrappedWords.days)
		figures += report.topPayees.map { WrappedWords.payments($0.count) } + [WrappedWords.count(report.reachedGoals.count)]
		figures += [String(report.year), String(report.year - 1)]
		figures += issue.pages.map { String($0.number) } + issue.pages.map { String($0.number + 1) }
		return figures.sorted { $0.count > $1.count }
	}

	@Test func everyFigureOnEveryPageIsAReportField() {
		// FC-8 copy rule 1, populated and redacted.
		for report in [Self.report(), Self.report(last: "2027-04-10")] {
			for redacted in [false, true] {
				let issue = Self.issue(report, redacted: redacted)
				let allowed = Self.allowedFigures(report, issue)
				for line in issue.text {
					var rest = line
					for figure in allowed where !figure.isEmpty {
						rest = rest.replacingOccurrences(of: figure, with: "")
					}
					#expect(rest.firstMatch(of: /\d/) == nil, "\(line) leaves \(rest)")
				}
			}
		}
	}

	@Test func everyNameIsAReportFieldOrTheCopysOwn() {
		// FC-8 copy rule 1 for names: each row is named by the report.
		let report = Self.report()
		var names = Set(report.topCategories.map { ReportWords.category($0.name, labels: $0.labels) } + report.topGroups.map(\.name) + report.topPayees.map(\.name))
		names.formUnion(report.reachedGoals.map { $0.deleted ? "(deleted) \($0.name)" : $0.name })
		let fixed: Set = ["Everything else", "Came in", "Went out", "Left over", "Came up short", "Debt paydown", "Savings", "Start of 2027", "End of 2027", "Today"]
		for redacted in [false, true] {
			for row in Self.issue(report, redacted: redacted).pages.flatMap(\.rows) {
				#expect(names.contains(row.label) || fixed.contains(row.label), "\(row.label)")
			}
		}
		#expect(Self.page(.cover)?.dek?.hasPrefix(report.budgetName) == true)
	}

	@Test func anAbsentFigureWritesNoSentence() {
		// FC-8 copy rule 3: None, nil and `from` each leave their sentence out.
		var report = Self.report()
		report.biggestVsTypical = nil
		report.typicalMonth = nil
		report.funFacts = []
		report.noSpend.longest = nil
		report.lowestMonthEnd = nil
		for redacted in [false, true] {
			let text = Self.issue(report, redacted: redacted).text.joined(separator: " ")
			#expect(!text.contains("typical"), "\(text)")
			#expect(!text.contains("longest run"), "\(text)")
			#expect(!text.contains("Lowest month end"), "\(text)")
			#expect(!text.contains("of categorized spending.") || redacted, "no top category share without the fact: \(text)")
		}
		#expect(Self.page(.categories, report)?.dek == "$430.00.")
		#expect(Self.page(.quietDays, report)?.dek == "Out of 16 days from January 5 to December 31.")
	}

	@Test func percentagesAreTheCoresTenthsAndBasisPoints() {
		let format = Self.format
		#expect(WrappedWords.percent(430, format: format) == "43.0%")
		#expect(WrappedWords.percent(-3, format: format) == "\u{2212}0.3%")
		#expect(WrappedWords.signedPercent(553, format: format) == "+55.3%")
		#expect(WrappedWords.basisPoints(750, format: format) == "7.5%")
		#expect(WrappedWords.basisPoints(15_000, format: format) == "150%")
		#expect(WrappedWords.basisPoints(111_184, format: format) == "1,111.84%")
		#expect(Self.moneyFigures(in: "+1,111.84% vs 2026").isEmpty)
		#expect(!Self.moneyFigures(in: "Left over 1,111.84").isEmpty)
		var comma = format
		comma.decimalSeparator = ","
		#expect(WrappedWords.percent(430, format: comma) == "43,0%")
	}
}
