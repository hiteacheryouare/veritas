#if DEBUG
import Foundation
import VeritasCore
import VeritasKit

/// The stub budget's Wrapped: 2026 to date with every page, a quieter 2025
/// that is the budget's first year, and no data before that. Shares are
/// worked by hand the way FC-8 rounds them (largest remainder for the top
/// five and Everything else, half away from zero for the rest).
enum StubWrapped {
	static let years: [Int32] = [2026, 2025]

	/// FC-8's monthly series from the budget's first month through the
	/// period's last: each share of total out rounded half away from zero,
	/// and a month without rows 0.
	private static func months(_ year: Int32, _ figures: [(month: UInt32, total: Int64, share: Int64)], through last: String) -> [WrappedSpendingMonth] {
		let end = Month(date: last)?.month ?? 12
		let start = figures.first?.month ?? 1
		return (start...max(start, end)).map { month in
			let figure = figures.first { $0.month == month }
			return WrappedSpendingMonth(month: Month(year: year, month: month), total: figure?.total ?? 0, share: figure?.share ?? 0)
		}
	}

	/// N1: each month of the series at its month end, the final month at
	/// `last`, with its change over `start` rounded half away from zero, or
	/// no change at all when `start` is 0 or less.
	private static func monthEnds(_ months: [WrappedSpendingMonth], start: Int64, last: String, values: [UInt32: Int64]) -> [WrappedMonthEnd] {
		var carried = start
		return months.map { month in
			carried = values[month.month.month] ?? carried
			let end = Calendar(identifier: .gregorian).range(of: .day, in: .month, for: month.month.date() ?? .distantPast)?.count ?? 28
			let date = min(String(format: "%04d-%02d-%02d", month.month.year, month.month.month, end), last)
			let share: Int64? = start > 0 ? Int64((Double(carried - start) * 1000 / Double(start)).rounded(.toNearestOrAwayFromZero)) : nil
			return WrappedMonthEnd(month: month.month, date: date, netWorth: carried, share: share)
		}
	}

	/// N3: `run` consecutive days, then every `step`th day from `from`, never
	/// touching the run, until `count` dates in all, none after `last`.
	private static func noSpendDates(run: (start: String, days: Int), every step: Int, from: String, count: Int, last: String) -> [String] {
		let calendar = Calendar(identifier: .gregorian)
		let parse = { (text: String) -> Date in
			let parts = text.split(separator: "-").compactMap { Int($0) }
			return calendar.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2])) ?? .distantPast
		}
		let text = { (date: Date) in String(format: "%04d-%02d-%02d", calendar.component(.year, from: date), calendar.component(.month, from: date), calendar.component(.day, from: date)) }
		let runStart = parse(run.start)
		let runDays = (0..<run.days).compactMap { calendar.date(byAdding: .day, value: $0, to: runStart) }
		var dates = Set(runDays.map(text))
		var day = parse(from)
		while dates.count < count, text(day) <= last {
			let near = runDays.contains { abs(calendar.dateComponents([.day], from: $0, to: day).day ?? 0) <= 1 }
			if !near { dates.insert(text(day)) }
			day = calendar.date(byAdding: .day, value: step, to: day) ?? day
		}
		return dates.filter { $0 <= last }.sorted()
	}

	static func report(year: Int32, today: String) throws -> WrappedReport {
		guard let thisYear = Int32(today.prefix(4)), year <= thisYear else {
			throw FfiError.Invalid(message: "\(year) hasn't started.")
		}
		let last = year == thisYear ? today : "\(year)-12-31"
		switch year {
		case 2026: return y2026(last: last, isCurrent: year == thisYear)
		case 2025: return y2025
		default: return empty(year: year, last: last)
		}
	}

	private static let none = ReportLabels(hidden: false, archived: false, deleted: false, closedGoal: false, needsReview: false)

	private static func category(_ id: String, _ name: String, _ total: Int64, _ share: Int64, labels: ReportLabels = none) -> WrappedCategory {
		WrappedCategory(categoryId: id, name: name, total: total, labels: labels, share: share)
	}

	static func y2026(last: String, isCurrent: Bool) -> WrappedReport {
		let series = months(2026, [
			(1, 540_000, 88), (2, 498_000, 81), (3, 812_000, 132), (4, 560_000, 91), (5, 575_000, 94), (6, 590_000, 96),
			(7, 610_000, 99), (8, 470_000, 77), (9, 600_000, 98), (10, 585_000, 95), (11, 300_000, 49)
		], through: last)
		let ends = monthEnds(series, start: 7_200_000, last: last, values: [
			1: 7_420_000, 2: 7_310_000, 3: 7_150_000, 4: 7_600_000, 5: 7_980_000, 6: 8_350_000,
			7: 8_720_000, 8: 9_200_000, 9: 9_800_000, 10: 10_600_000, 11: 11_183_000
		])
		let dates = noSpendDates(run: ("2026-06-03", 9), every: 3, from: "2026-01-02", count: 87, last: last)
		return WrappedReport(
			year: 2026, first: "2026-01-01", last: last, hasData: true,
			totalIn: 8_421_000, totalOut: 6_140_000, net: 2_281_000, debtPaydown: 520_000, savings: 960_000,
			topCategories: [
				category("rent", "Rent", 1_980_000, 361),
				category("groceries", "Groceries", 1_120_000, 204),
				category("dining", "Dining out", 480_000, 88),
				category("travel", "Travel", 410_000, 75),
				category("electric", "Electric", 290_000, 53)
			],
			otherCategories: 1_200_000,
			topGroups: [
				WrappedGroup(groupId: "housing", name: "Housing and utilities", total: 2_270_000, share: 414),
				WrappedGroup(groupId: "everyday", name: "Everyday", total: 1_600_000, share: 292),
				WrappedGroup(groupId: "personal", name: "Personal", total: 410_000, share: 75)
			],
			otherGroups: 1_200_000,
			topPayees: [
				WrappedPayee(payeeId: "landlord", name: "Landlord", total: 1_980_000, count: 11, share: 404),
				WrappedPayee(payeeId: "fresh-market", name: "Fresh Market", total: 860_000, count: 48, share: 176),
				WrappedPayee(payeeId: "northwind-air", name: "Northwind Air", total: 410_000, count: 2, share: 84),
				WrappedPayee(payeeId: "city-power", name: "City Power", total: 290_000, count: 11, share: 59),
				WrappedPayee(payeeId: "corner-cafe", name: "Corner Cafe", total: 260_000, count: 37, share: 53)
			],
			otherPayees: 1_100_000,
			biggestMonth: WrappedMonth(month: Month(year: 2026, month: 3), total: 812_000),
			biggestVsTypical: 4_122,
			noSpend: WrappedNoSpend(first: "2026-01-01", last: last, days: UInt32(dates.count), evaluatedDays: 324, longest: WrappedStreak(start: "2026-06-03", days: 9), current: isCurrent ? 3 : nil, dates: dates),
			completedGoalIds: ["vet", "tires"],
			netWorthStart: 7_200_000, netWorthEnd: 11_183_000, netWorthChange: 3_983_000,
			priorYear: WrappedPriorYear(totalIn: .basisPoints(value: 750), totalOut: .basisPoints(value: -320), net: .basisPoints(value: 4_100), from: nil),
			shares: WrappedShares(
				outVsIn: 729, net: 271, debtPaydown: 62, savings: 114, biggestMonth: 132, netWorthChange: 553,
				otherCategories: 219, otherGroups: 219, otherPayees: 224, typicalMonth: 94
			),
			funFacts: [
				.priorYear(measure: .totalIn, basisPoints: 750),
				.biggestMonthVsTypical(month: Month(year: 2026, month: 3), basisPoints: 4_122),
				.topCategoryShare(categoryId: "rent", tenths: 361),
				.noSpendShare(days: 87, evaluatedDays: 324, tenths: 269)
			],
			months: series,
			budgetName: "Household",
			monthEnds: ends,
			lowestMonthEnd: ends.min { $0.netWorth < $1.netWorth },
			highestMonthEnd: ends.reduce(nil) { (best: WrappedMonthEnd?, end) in (best?.netWorth ?? .min) >= end.netWorth ? best : end },
			typicalMonth: 575_000,
			reachedGoals: [
				WrappedReachedGoal(goalId: "vet", name: "Vet bill", deleted: false, month: Month(year: 2026, month: 4)),
				WrappedReachedGoal(goalId: "tires", name: "Winter tires", deleted: false, month: Month(year: 2026, month: 10))
			].filter { $0.month.month <= Month(date: last)?.month ?? 12 }
		)
	}

	/// The budget's first year: it starts in August, so last year is new
	/// throughout and opening net worth is zero, which leaves its share n/a.
	static let y2025: WrappedReport = {
		let series = months(2025, [(8, 520_000, 158), (9, 610_000, 185), (10, 640_000, 194), (11, 630_000, 191), (12, 900_000, 273)], through: "2025-12-31")
		let ends = monthEnds(series, start: 0, last: "2025-12-31", values: [8: 1_500_000, 9: 2_900_000, 10: 4_300_000, 11: 5_800_000, 12: 7_200_000])
		let dates = noSpendDates(run: ("2025-10-12", 6), every: 4, from: "2025-08-04", count: 40, last: "2025-12-31")
		return WrappedReport(
		year: 2025, first: "2025-01-01", last: "2025-12-31", hasData: true,
		totalIn: 4_100_000, totalOut: 3_300_000, net: 800_000, debtPaydown: 0, savings: 300_000,
		topCategories: [category("rent", "Rent", 990_000, 639), category("groceries", "Groceries", 560_000, 361)],
		otherCategories: 0,
		topGroups: [
			WrappedGroup(groupId: "housing", name: "Housing and utilities", total: 990_000, share: 639),
			WrappedGroup(groupId: "everyday", name: "Everyday", total: 560_000, share: 361)
		],
		otherGroups: 0,
		topPayees: [WrappedPayee(payeeId: "landlord", name: "Landlord", total: 990_000, count: 5, share: 1_000)],
		otherPayees: 0,
		biggestMonth: WrappedMonth(month: Month(year: 2025, month: 12), total: 900_000),
		biggestVsTypical: 4_286,
		noSpend: WrappedNoSpend(first: "2025-08-03", last: "2025-12-31", days: UInt32(dates.count), evaluatedDays: 151, longest: WrappedStreak(start: "2025-10-12", days: 6), current: nil, dates: dates),
		completedGoalIds: [],
		netWorthStart: 0, netWorthEnd: 7_200_000, netWorthChange: 7_200_000,
		priorYear: WrappedPriorYear(totalIn: .newThisYear, totalOut: .newThisYear, net: .newThisYear, from: nil),
		shares: WrappedShares(
			outVsIn: 805, net: 195, debtPaydown: 0, savings: 73, biggestMonth: 273, netWorthChange: nil,
			otherCategories: nil, otherGroups: nil, otherPayees: nil, typicalMonth: 191
		),
		funFacts: [
			.biggestMonthVsTypical(month: Month(year: 2025, month: 12), basisPoints: 4_286),
			.topCategoryShare(categoryId: "rent", tenths: 639),
			.noSpendShare(days: 40, evaluatedDays: 151, tenths: 265)
		],
		months: series,
		budgetName: "Household",
		monthEnds: ends,
		lowestMonthEnd: ends.first,
		highestMonthEnd: ends.last,
		typicalMonth: 630_000,
		reachedGoals: []
		)
	}()

	static func empty(year: Int32, last: String) -> WrappedReport {
		WrappedReport(
			year: year, first: "\(year)-01-01", last: last, hasData: false,
			totalIn: 0, totalOut: 0, net: 0, debtPaydown: 0, savings: 0,
			topCategories: [], otherCategories: 0, topGroups: [], otherGroups: 0, topPayees: [], otherPayees: 0,
			biggestMonth: nil, biggestVsTypical: nil,
			noSpend: WrappedNoSpend(first: nil, last: nil, days: 0, evaluatedDays: 0, longest: nil, current: nil),
			completedGoalIds: [], netWorthStart: 0, netWorthEnd: 0, netWorthChange: 0,
			priorYear: WrappedPriorYear(totalIn: .newThisYear, totalOut: .newThisYear, net: .newThisYear, from: nil),
			shares: WrappedShares(outVsIn: nil, net: nil, debtPaydown: nil, savings: nil, biggestMonth: nil, netWorthChange: nil, otherCategories: nil, otherGroups: nil, otherPayees: nil),
			funFacts: []
		)
	}
}
#endif
