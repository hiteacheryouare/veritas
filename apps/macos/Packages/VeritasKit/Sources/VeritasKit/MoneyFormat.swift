import Foundation
import VeritasCore

/// Formats amounts that cross the bridge as integer minor units, with the
/// budget's number format and its currency's row from the core (CLS-12.3).
/// Negatives use a real minus sign, U+2212. Integer arithmetic only (CLS-12.5).
public struct MoneyFormat: Hashable, Sendable {
	public static let minus = "\u{2212}"

	/// D63 stores a 0-place budget's numbers on two scales 100 times apart, so
	/// every call names the scale of the number it shows (CLS-12.9).
	public enum Scale: Hashable, Sendable {
		/// Transaction amounts and everything summed from them alone, such as
		/// account balances: always two implied places.
		case register
		/// Budget cells, holds and goal amounts, and every number on the
		/// envelope sheet, mixed ones included (CLS-12.10): the currency's places.
		case budget
	}

	public var currencySymbol: String
	public var symbolFirst: Bool
	/// A narrow no-break space (U+202F) between the symbol and the digits.
	public var symbolSpace: Bool
	public var groupingSeparator: String
	public var decimalSeparator: String
	/// Lakh and crore grouping (`1,00,000.33`): three digits, then twos.
	public var groupsInTwosAfterThousands: Bool
	/// The currency's places from Actual's table: 2 for most, 0 for JPY, KRW and IRR.
	public var decimalPlaces: UInt8
	/// Actual's hide-fraction preference: amounts show no decimals at all.
	public var hidesDecimals: Bool

	public init(
		currencySymbol: String = "$",
		symbolFirst: Bool = true,
		symbolSpace: Bool = false,
		groupingSeparator: String = ",",
		decimalSeparator: String = ".",
		groupsInTwosAfterThousands: Bool = false,
		decimalPlaces: UInt8 = 2,
		hidesDecimals: Bool = false
	) {
		self.currencySymbol = currencySymbol
		self.symbolFirst = symbolFirst
		self.symbolSpace = symbolSpace
		self.groupingSeparator = groupingSeparator
		self.decimalSeparator = decimalSeparator
		self.groupsInTwosAfterThousands = groupsInTwosAfterThousands
		self.decimalPlaces = decimalPlaces
		self.hidesDecimals = hidesDecimals
	}

	/// Before a budget is open: the Mac's separators, two places.
	public init(locale: Locale, currencySymbol: String = "$") {
		self.init(
			currencySymbol: currencySymbol,
			groupingSeparator: locale.groupingSeparator ?? ",",
			decimalSeparator: locale.decimalSeparator ?? "."
		)
	}

	/// The budget's own preferences and its currency's row in `currencies()`.
	/// A budget with no currency, or one Actual's table doesn't list, has no
	/// symbol and two places, as in Actual (`getCurrency`, CLS-12.7). The
	/// symbol's position and spacing are the budget's preferences, which
	/// `overview()` always sets; unset reads as Actual's default, before with
	/// no space.
	public init(_ format: NumberFormat, currency: Currency?) {
		let separators: (grouping: String, decimal: String) = switch format.style {
		case .commaDot: (",", ".")
		case .dotComma: (".", ",")
		case .spaceComma: ("\u{202F}", ",")
		case .apostropheDot: ("'", ".")
		case .commaDotIn: (",", ".")
		}
		self.init(
			currencySymbol: currency?.symbol ?? "",
			symbolFirst: format.symbolFirst ?? true,
			symbolSpace: format.symbolSpace ?? false,
			groupingSeparator: separators.grouping,
			decimalSeparator: separators.decimal,
			groupsInTwosAfterThousands: format.style == .commaDotIn,
			decimalPlaces: currency?.decimalPlaces ?? 2,
			hidesDecimals: format.hideDecimals
		)
	}

	/// `2,140.18`, `−612.33`: rows, columns and the sidebar.
	public func amount(_ minorUnits: Int64, on scale: Scale) -> String {
		let shown = shownPlaces
		let magnitude = Self.rounded(minorUnits.magnitude, from: impliedPlaces(scale), to: shown)
		return sign(minorUnits, magnitude) + digits(magnitude, places: shown)
	}

	/// `2,140.18`, `(612.33)`: a statement shows a negative in parentheses (ST-3).
	public func statement(_ minorUnits: Int64, on scale: Scale) -> String {
		let shown = shownPlaces
		let magnitude = Self.rounded(minorUnits.magnitude, from: impliedPlaces(scale), to: shown)
		let text = digits(magnitude, places: shown)
		return sign(minorUnits, magnitude).isEmpty ? text : "(\(text))"
	}

	/// `$312.40`, `−$34.75`, `12,50€`: titles and hero figures.
	public func currency(_ minorUnits: Int64, on scale: Scale) -> String {
		let shown = shownPlaces
		let magnitude = Self.rounded(minorUnits.magnitude, from: impliedPlaces(scale), to: shown)
		return sign(minorUnits, magnitude) + withSymbol(digits(magnitude, places: shown))
	}

	/// `$34.75` for both 3475 and -3475: prose that names the direction in words.
	public func currencyMagnitude(_ minorUnits: Int64, on scale: Scale) -> String {
		let shown = shownPlaces
		return withSymbol(digits(Self.rounded(minorUnits.magnitude, from: impliedPlaces(scale), to: shown), places: shown))
	}

	/// `$1,120`: goal cards, rounded half away from zero to the whole unit (CLS-12.4).
	public func wholeCurrency(_ minorUnits: Int64, on scale: Scale) -> String {
		let magnitude = Self.rounded(minorUnits.magnitude, from: impliedPlaces(scale), to: 0)
		return sign(minorUnits, magnitude) + withSymbol(digits(magnitude, places: 0))
	}

	/// A figure for a chart's axis, in whole units of the currency: register
	/// amounts always carry 2 places, budget amounts the budget's own.
	public func chartValue(_ minorUnits: Int64, on scale: Scale) -> Double {
		chartValue(Double(minorUnits), on: scale)
	}

	public func chartValue(_ minorUnits: Double, on scale: Scale) -> Double {
		minorUnits / pow(10, Double(impliedPlaces(scale)))
	}

	/// `$12k`, `−$800`: a chart axis label for a `chartValue`, with the symbol where the budget puts it.
	public func axisLabel(_ units: Double) -> String {
		let whole = Int64(units.rounded())
		let digits = whole.magnitude >= 1000 ? "\(whole.magnitude / 1000)k" : "\(whole.magnitude)"
		return (whole < 0 ? "\u{2212}" : "") + withSymbol(digits)
	}

	/// A formula card's number, in whole units, at the currency's places:
	/// rounded only to be shown, half up as Actual's `amountToInteger` rounds.
	/// `nil` when it can't be shown, such as NaN or infinity.
	public func number(_ units: Double) -> String? {
		let scaled = (units * pow(10, Double(shownPlaces)) + 0.5).rounded(.down)
		guard scaled.isFinite, scaled.magnitude < 9.0e18 else { return nil }
		let minorUnits = Int64(scaled)
		return sign(minorUnits, minorUnits.magnitude) + digits(minorUnits.magnitude, places: shownPlaces)
	}

	private var shownPlaces: Int {
		hidesDecimals ? 0 : Int(decimalPlaces)
	}

	private func impliedPlaces(_ scale: Scale) -> Int {
		switch scale {
		case .register: 2
		case .budget: Int(decimalPlaces)
		}
	}

	private func withSymbol(_ digits: String) -> String {
		guard !currencySymbol.isEmpty else { return digits }
		let space = symbolSpace ? "\u{202F}" : ""
		return symbolFirst ? currencySymbol + space + digits : digits + space + currencySymbol
	}

	/// No minus on an amount that rounds to zero.
	private func sign(_ minorUnits: Int64, _ shownMagnitude: UInt64) -> String {
		minorUnits < 0 && shownMagnitude > 0 ? Self.minus : ""
	}

	/// The magnitude with `implied` decimals, re-expressed with `shown`
	/// decimals, rounding half away from zero when places are dropped.
	static func rounded(_ magnitude: UInt64, from implied: Int, to shown: Int) -> UInt64 {
		guard shown < implied else { return magnitude * power(of: 10, shown - implied) }
		let divisor = power(of: 10, implied - shown)
		return magnitude / divisor + (magnitude % divisor >= divisor / 2 ? 1 : 0)
	}

	/// `magnitude` already carries exactly `places` decimals.
	private func digits(_ magnitude: UInt64, places: Int) -> String {
		let unit = Self.power(of: 10, places)
		let whole = String(magnitude / unit)
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
		guard places > 0 else { return grouped }
		let fraction = String(magnitude % unit)
		return grouped + decimalSeparator + String(repeating: "0", count: places - fraction.count) + fraction
	}

	private static func power(of base: UInt64, _ exponent: Int) -> UInt64 {
		(0..<exponent).reduce(1) { result, _ in result * base }
	}
}
