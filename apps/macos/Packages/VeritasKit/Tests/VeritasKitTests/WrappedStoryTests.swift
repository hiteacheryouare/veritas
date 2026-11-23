import Foundation
import RegexBuilder
import Testing
import VeritasCore
@testable import VeritasKit

/// The Wrapped cards' words from a core report (R-WRAP-1), and R-WRAP-2's
/// redaction: with amounts hidden no card carries a money amount, only the
/// core's shares, counts, names and dates.
struct WrappedStoryTests {
	static let format = MoneyFormat()
	static let none = ReportLabels(hidden: false, archived: false, deleted: false, closedGoal: false, needsReview: false)

	/// FC-8's W-14 categories, W-8's net worth and W-16's refunds-beat-spending out.
	static func report(last: String = "2027-12-31", prior: WrappedPriorYear? = nil) -> WrappedReport {
		WrappedReport(
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
			noSpend: WrappedNoSpend(first: "2027-01-05", last: "2027-12-31", days: 5, evaluatedDays: 16, longest: WrappedStreak(start: "2027-01-07", days: 2), current: nil),
			completedGoalIds: ["laptop", "unknown"],
			netWorthStart: 720_000, netWorthEnd: 1_118_300, netWorthChange: 398_300,
			priorYear: prior ?? WrappedPriorYear(totalIn: .basisPoints(value: 750), totalOut: .newThisYear, net: .basisPoints(value: 15_000), from: nil),
			shares: WrappedShares(outVsIn: 1_150, net: -150, debtPaydown: 120, savings: 0, biggestMonth: 261, netWorthChange: 553, otherCategories: 78, otherGroups: nil, otherPayees: nil),
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
			]
		)
	}

	static func story(_ report: WrappedReport = report()) -> WrappedStory {
		WrappedStory(report: report, format: format, goalNames: ["laptop": "New laptop"])
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

	@Test func redactedCardsCarryNoMoneyAmount() throws {
		let report = Self.report()
		let cards = Self.story(report).cards(redacted: true)
		let text = cards.flatMap(\.text)
		#expect(!text.isEmpty)
		for line in text {
			#expect(Self.moneyFigures(in: line).isEmpty, "\(line)")
		}
		let amounts = [report.totalIn, report.totalOut, report.net, report.debtPaydown, report.netWorthStart, report.netWorthEnd, report.netWorthChange, report.otherCategories]
			+ report.topCategories.map(\.total) + report.topGroups.map(\.total) + report.topPayees.map(\.total) + [report.biggestMonth?.total ?? 0]
		for amount in amounts where amount != 0 {
			for line in text {
				#expect(!line.contains(Self.format.amount(amount, on: .register)), "\(amount) in \(line)")
			}
		}
	}

	@Test func redactedCardsShowTheCoresShares() {
		let cards = Dictionary(uniqueKeysWithValues: Self.story().cards(redacted: true).map { ($0.kind, $0) })
		#expect(cards[.categories]?.figure == "37.4%")
		#expect(cards[.categories]?.rows.map(\.value) == ["37.4%", "26.1%", "14.8%", "8.7%", "5.2%", "7.8%"])
		#expect(cards[.categories]?.rows.last?.label == "Everything else")
		#expect(cards[.categories]?.rows[4].label == "Old Books (hidden)")
		#expect(cards[.categories]?.detail == "of categorized spending")
		#expect(cards[.totals]?.rows.map(\.value) == ["+7.5% vs 2026", "115.0% of what came in", "15.0% of what came in"])
		#expect(cards[.totals]?.rows.map(\.label) == ["Came in", "Went out", "Came up short"], "a deficit says so in words")
		#expect(cards[.groups]?.rows.map(\.value) == ["63.5%", "36.5%"], "no Everything else when the core has none")
		#expect(cards[.payees]?.detail == "of what went to payees", "each row carries its own payee's count")
		#expect(cards[.biggestMonth]?.figure == "26.1%")
		#expect(cards[.biggestMonth]?.detail == "of the year's spending, 56% more than your typical month")
		#expect(cards[.debtAndSavings]?.rows.map(\.value) == ["12.0% of what came in"])
		#expect(cards[.netWorth]?.figure == "+55.3%")
		#expect(cards[.netWorth]?.rows.isEmpty == true)
	}

	@Test func amountsShowTheCoresFigures() {
		let cards = Dictionary(uniqueKeysWithValues: Self.story().cards(redacted: false).map { ($0.kind, $0) })
		#expect(cards[.totals]?.title == "Your 2027")
		#expect(cards[.totals]?.rows.map(\.value) == ["$1,000.00", "$1,150.00", "$150.00"])
		#expect(cards[.totals]?.rows.last?.label == "Came up short")
		#expect(cards[.totals]?.rows.map(\.note) == ["+7.5% vs 2026", "New this year", "+150% vs 2026"])
		#expect(cards[.categories]?.title == "Groceries topped your spending.")
		#expect(cards[.categories]?.figure == "$430.00")
		#expect(cards[.categories]?.detail == "37.4% of categorized spending")
		#expect(cards[.payees]?.title == "You paid Market the most.")
		#expect(cards[.payees]?.rows.map(\.note) == ["12 payments", "1 payment"])
		#expect(cards[.biggestMonth]?.title == "March was your biggest month for spending.")
		#expect(cards[.biggestMonth]?.detail == "56% more than your typical month")
		#expect(cards[.netWorth]?.title == "Net worth went up.")
		#expect(cards[.netWorth]?.figure == "+$3,983.00")
		#expect(cards[.netWorth]?.rows.map(\.label) == ["Start of 2027", "End of 2027"])
	}

	@Test func theBiggestMonthDrawsTheCoresMonthsByShare() {
		// v18, FC-8: one bar per month the core sends, by its share; a refund
		// month stays negative, and only the biggest month carries its amount.
		let shown = Self.story().cards(redacted: false).first { $0.kind == .biggestMonth }
		let bars = shown?.bars ?? []
		#expect(bars.map(\.share) == [174, -43, 261, 0])
		#expect(bars.map(\.slot) == [0, 1, 2, 3])
		#expect(bars.map(\.letter) == ["J", "F", "M", "A"])
		#expect(bars.map(\.isBiggest) == [false, false, true, false])
		#expect(bars.map(\.value) == [nil, nil, "$300", nil])
		#expect(bars.map(\.spoken) == ["January, $200.00", "February, refund month, \u{2212}$50.00", "March, $300.00", "April, $0.00"])

		let hidden = Self.story().cards(redacted: true).first { $0.kind == .biggestMonth }
		#expect(hidden?.bars.map(\.share) == [174, -43, 261, 0], "the same bars with amounts hidden")
		#expect(hidden?.bars.allSatisfy { $0.value == nil } == true, "bars only, no figures")
		#expect(hidden?.bars.first?.spoken == "January, 17.4% of total out")
		#expect(hidden?.bars[1].spoken == "February, refund month", "a negative share reads as a refund month")
	}

	@Test func withNoTotalOutEveryBarReadsNA() {
		// FC-8 W-21: out of 0 or less has no share, and every bar says so.
		var noOut = Self.report()
		noOut.months = noOut.months.map { WrappedSpendingMonth(month: $0.month, total: $0.total, share: nil) }
		for redacted in [true, false] {
			let bars = Self.story(noOut).cards(redacted: redacted).first { $0.kind == .biggestMonth }?.bars ?? []
			#expect(bars.count == 4, "a bar per month, not an empty chart")
			#expect(bars.allSatisfy { $0.share == nil && $0.value == "n/a" })
		}
		let hidden = Self.story(noOut).cards(redacted: true).first { $0.kind == .biggestMonth }?.bars ?? []
		#expect(hidden.map(\.spoken) == ["January, n/a", "February, n/a", "March, n/a", "April, n/a"])
	}

	@Test func aYearStartingInMayLeavesJanuaryToAprilEmpty() {
		// FC-8 leaves out months before the budget's first row, so a bar sits
		// in its own month's slot, never its index in the series.
		var report = Self.report()
		report.months = (5...12).map { month in
			WrappedSpendingMonth(month: Month(year: 2027, month: UInt32(month)), total: 10_000, share: 125)
		}
		report.biggestMonth = WrappedMonth(month: Month(year: 2027, month: 5), total: 10_000)
		let bars = Self.story(report).cards(redacted: false).first { $0.kind == .biggestMonth }?.bars ?? []
		#expect(bars.map(\.slot) == Array(4...11))
		#expect(bars.first?.letter == "M")
		#expect(bars.first?.isBiggest == true)
		#expect(bars.first?.spoken == "May, $100.00")
	}

	@Test func countsAndDatesReadTheSameEitherWay() {
		for redacted in [false, true] {
			let cards = Dictionary(uniqueKeysWithValues: Self.story().cards(redacted: redacted).map { ($0.kind, $0) })
			#expect(cards[.noSpend]?.figure == "5")
			#expect(cards[.noSpend]?.detail == "31.3% of the 16 days from January 5 to December 31")
			#expect(cards[.noSpend]?.rows.map(\.value) == ["2 days"])
			#expect(cards[.goals]?.title == "You reached 2 goals.")
			#expect(cards[.goals]?.rows.map(\.label) == ["New laptop"], "a goal the list doesn't know is counted but not named")
		}
	}

	/// FC-8: bills and loan lines don't break a no-spend day, so no card says nothing was spent.
	@Test func noCardSaysNothingWasSpent() {
		for redacted in [false, true] {
			for line in Self.story().cards(redacted: redacted).flatMap(\.text) {
				#expect(!line.localizedCaseInsensitiveContains("spent nothing"), "\(line)")
				#expect(!line.localizedCaseInsensitiveContains("nothing spent"), "\(line)")
			}
		}
	}

	@Test func theYearToDateSaysSoAndComparesWithTheSameDays() {
		let story = Self.story(Self.report(last: "2027-03-10"))
		#expect(story.isYearToDate)
		let totals = story.cards(redacted: false)[0]
		#expect(totals.title == "Your 2027 so far")
		#expect(totals.detail == "January 1 to March 10")
		#expect(totals.rows[0].note == "+7.5% vs this point in 2026")
		let month = story.cards(redacted: true).first { $0.kind == .biggestMonth }
		#expect(month?.detail == "of spending so far, 56% more than your typical month")
	}

	@Test func aLateStartCarriesThePriorYearLabel() {
		let prior = WrappedPriorYear(totalIn: .basisPoints(value: -1_250), totalOut: .newThisYear, net: .newThisYear, from: "2026-08-03")
		let totals = Self.story(Self.report(prior: prior)).cards(redacted: true)[0]
		#expect(totals.rows[0].value == "\u{2212}12.5% vs prior year from August 3, 2026")
	}

	@Test func anEmptyYearIsOneCard() {
		var report = Self.report()
		report.hasData = false
		let cards = Self.story(report).cards(redacted: false)
		#expect(cards.map(\.kind) == [.noData])
		#expect(cards[0].title == "No data for 2027.")
	}

	@Test func shareNotApplicableReadsNA() {
		var report = Self.report()
		report.shares.netWorthChange = nil
		report.shares.outVsIn = nil
		report.shares.net = nil
		let cards = Dictionary(uniqueKeysWithValues: Self.story(report).cards(redacted: true).map { ($0.kind, $0) })
		#expect(cards[.netWorth]?.figure == "n/a")
		#expect(cards[.totals]?.rows.dropFirst().map(\.value) == ["n/a", "n/a"])
	}

	@Test func percentagesAreTheCoresTenthsAndBasisPoints() {
		let format = Self.format
		#expect(WrappedWords.percent(430, format: format) == "43.0%")
		#expect(WrappedWords.percent(-3, format: format) == "\u{2212}0.3%")
		#expect(WrappedWords.percent(1_003, format: format) == "100.3%")
		#expect(WrappedWords.signedPercent(553, format: format) == "+55.3%")
		#expect(WrappedWords.signedPercent(0, format: format) == "0.0%")
		#expect(WrappedWords.basisPoints(750, format: format) == "7.5%")
		#expect(WrappedWords.basisPoints(15_000, format: format) == "150%")
		#expect(WrappedWords.basisPoints(755, format: format) == "7.55%")
		#expect(WrappedWords.basisPoints(5, format: format) == "0.05%")
		#expect(WrappedWords.basisPoints(111_184, format: format) == "1,111.84%")
		#expect(Self.moneyFigures(in: "+1,111.84% vs 2026").isEmpty)
		#expect(!Self.moneyFigures(in: "Left over 1,111.84").isEmpty)
		#expect(WrappedWords.signedBasisPoints(-320, format: format) == "\u{2212}3.2%")
		var comma = format
		comma.decimalSeparator = ","
		#expect(WrappedWords.percent(430, format: comma) == "43,0%")
		var lakh = format
		lakh.groupsInTwosAfterThousands = true
		#expect(WrappedWords.basisPoints(12_345_600, format: lakh) == "1,23,456%")
	}
}
