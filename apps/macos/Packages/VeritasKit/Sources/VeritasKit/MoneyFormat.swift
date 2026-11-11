import Foundation

/// Formats amounts that cross the bridge as integer minor units (cents).
/// Negatives use a real minus sign, U+2212.
public struct MoneyFormat: Hashable, Sendable {
	public static let minus = "\u{2212}"

	public var currencySymbol: String
	public var groupingSeparator: String
	public var decimalSeparator: String

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
			if index > 0, (whole.count - index) % 3 == 0 {
				grouped += groupingSeparator
			}
			grouped.append(digit)
		}
		return grouped + decimalSeparator + (cents < 10 ? "0" : "") + String(cents)
	}
}
