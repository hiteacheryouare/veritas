import Foundation
import VeritasCore

/// Percentages, counts and dates as Wrapped writes them.
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
