import Foundation
import VeritasCore

/// Formats amounts that cross the bridge as integer minor units (cents).
/// Negatives use a real minus sign, U+2212.
public struct MoneyFormat: Hashable, Sendable {
	public static let minus = "\u{2212}"

	public var currencySymbol: String
	public var groupingSeparator: String
	public var decimalSeparator: String
	/// Lakh and crore grouping (`1,00,000.33`): three digits, then twos.
	public var groupsInTwosAfterThousands = false

	public init(currencySymbol: String = "$", groupingSeparator: String = ",", decimalSeparator: String = ".") {
		self.currencySymbol = currencySymbol
		self.groupingSeparator = groupingSeparator
		self.decimalSeparator = decimalSeparator
	}

	public init(locale: Locale, currencySymbol: String = "$") {
		self.init(
			currencySymbol: currencySymbol,
			groupingSeparator: locale.groupingSeparator ?? ",",
			decimalSeparator: locale.decimalSeparator ?? "."
		)
	}

	/// The budget's own preference from the core. The symbol comes from the Mac's currency data.
	public init(_ format: NumberFormat) {
		let separators: (grouping: String, decimal: String) = switch format.style {
		case .commaDot: (",", ".")
		case .dotComma: (".", ",")
		case .spaceComma: ("\u{202F}", ",")
		case .apostropheDot: ("'", ".")
		case .commaDotIn: (",", ".")
		}
		self.init(
			currencySymbol: format.currency.map(Self.symbol(forCurrency:)) ?? "",
			groupingSeparator: separators.grouping,
			decimalSeparator: separators.decimal
		)
		groupsInTwosAfterThousands = format.style == .commaDotIn
	}

	static func symbol(forCurrency code: String) -> String {
		let formatter = NumberFormatter()
		formatter.numberStyle = .currency
		formatter.currencyCode = code
		return formatter.currencySymbol
	}

	/// `2,140.18`, `−612.33`: rows, columns and the sidebar.
	public func amount(_ minorUnits: Int64) -> String {
		sign(minorUnits) + digits(minorUnits.magnitude)
	}

	/// `$312.40`, `−$34.75`: titles and hero figures.
	public func currency(_ minorUnits: Int64) -> String {
		sign(minorUnits) + currencySymbol + digits(minorUnits.magnitude)
	}

	private func sign(_ minorUnits: Int64) -> String {
		minorUnits < 0 ? Self.minus : ""
	}

	private func digits(_ magnitude: UInt64) -> String {
		let whole = String(magnitude / 100)
		let cents = magnitude % 100
		var grouped = ""
		for (index, digit) in whole.enumerated() {
			let remaining = whole.count - index
			let startsGroup = groupsInTwosAfterThousands
				? remaining == 3 || (remaining > 3 && (remaining - 3) % 2 == 0)
				: remaining % 3 == 0
			if index > 0, startsGroup {
				grouped += groupingSeparator
			}
			grouped.append(digit)
		}
		return grouped + decimalSeparator + (cents < 10 ? "0" : "") + String(cents)
	}
}
