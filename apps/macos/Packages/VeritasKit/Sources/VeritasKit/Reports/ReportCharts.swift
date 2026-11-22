import CoreGraphics
import Foundation
import VeritasCore

/// One series of a custom report's graph: one of the report's groups, under
/// the name the core gives it. The graph draws these and nothing else, as
/// Actual's graph draws one bar per legend entry: an interval's stacked
/// amounts also hold every empty or hidden category at zero, which have no
/// legend entry.
public struct ReportSeries: Identifiable, Hashable, Sendable {
	public struct Point: Hashable, Sendable {
		/// The interval's key: `YYYY-MM`, `YYYY` or a first day.
		public let interval: String
		/// Minor units, signed so money spent draws upward.
		public let amount: Int64
	}

	/// Actual's `dataKey`: the group's id, or its name for the Uncategorized entries.
	public let id: String
	public let name: String
	/// 0 to 8, the group's place in the legend, as Actual colors it.
	public let color: UInt8
	public let total: Int64
	public let points: [Point]

	public static let palette = 9

	/// Payments are stored negative and drawn upward, as money spent.
	public static func drawnUpward(_ balance: ReportBalanceKind) -> Bool {
		balance == .payment || balance == .netPayment
	}

	public static func all(_ data: CustomReportData, balance: ReportBalanceKind) -> [ReportSeries] {
		let sign: Int64 = drawnUpward(balance) ? -1 : 1
		return data.groups.enumerated().map { index, group in
			ReportSeries(
				id: group.id.isEmpty ? group.name : group.id,
				name: group.name,
				color: UInt8(index % palette),
				total: group.amount * sign,
				points: group.intervals.map { Point(interval: $0.interval, amount: $0.amount * sign) }
			)
		}
	}
}

/// Where a chart changes from its positive tone to its negative one, as a
/// stop in a top-to-bottom gradient over a mark that spans `low...high`.
/// 1 keeps the whole mark positive and 0 makes it all negative, as
/// Actual's `gradientOffset` does for the net worth and forecast graphs.
public enum ZeroSplit {
	public static func stop(low: Double, high: Double) -> Double {
		if high <= 0 { return 0 }
		if low >= 0 { return 1 }
		return high / (high - low)
	}

	/// The value range a chart shows, a little past the data so a line
	/// isn't drawn on the plot's edge. `includesZero` keeps the zero line in view.
	public static func domain(_ values: [Double], includesZero: Bool) -> ClosedRange<Double> {
		var low = values.min() ?? 0
		var high = values.max() ?? 0
		if includesZero {
			low = min(low, 0)
			high = max(high, 0)
		}
		let pad = high > low ? (high - low) * 0.05 : max(abs(high) * 0.05, 1)
		let keepsLow = includesZero && low == 0
		let keepsHigh = includesZero && high == 0
		if keepsLow && keepsHigh { return 0...1 }
		return (keepsLow ? 0 : low - pad)...(keepsHigh ? 0 : high + pad)
	}

	/// Where an area is filled to: zero when it's in view, else the nearer edge.
	public static func baseline(in domain: ClosedRange<Double>) -> Double {
		min(max(0, domain.lowerBound), domain.upperBound)
	}

	/// The stop for an area filled from `values` to `baseline`.
	public static func areaStop(_ values: [Double], baseline: Double) -> Double {
		stop(low: min(values.min() ?? baseline, baseline), high: max(values.max() ?? baseline, baseline))
	}

	/// The stop for a line through `values`.
	public static func lineStop(_ values: [Double]) -> Double {
		stop(low: values.min() ?? 0, high: values.max() ?? 0)
	}
}

/// The spending card's lines as Actual's `SpendingGraph` draws them: each
/// running total turned positive (`getVal`), so money spent rises from zero.
/// Days 28 to 31 share the 28th, labeled "28+".
public struct SpendingPoint: Hashable, Sendable {
	public let day: Int
	public let label: String
	/// The compared month so far, in minor units; `nil` after today.
	public let compare: Int64?
	/// The comparison so far, in minor units (fractional for the budget line).
	public let comparison: Double?

	public static func all(_ days: [SpendingCardDay], mode: SpendingCardMode) -> [SpendingPoint] {
		days.map { day in
			let comparison: Double? = switch mode {
			case .singleMonth: day.compareTo.map { -Double($0) }
			case .average: -Double(day.average)
			case .budget: day.budget.isFinite ? -day.budget : nil
			}
			return SpendingPoint(
				day: Int(day.day),
				label: day.day >= 28 ? "28+" : "\(day.day)",
				compare: day.compare.map { -$0 },
				comparison: comparison
			)
		}
	}
}
