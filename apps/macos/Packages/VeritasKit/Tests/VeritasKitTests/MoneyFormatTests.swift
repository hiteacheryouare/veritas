import Foundation
import Testing
import VeritasCore
import VeritasKit

struct MoneyFormatTests {
	let format = MoneyFormat()

	@Test(arguments: [
		(Int64(214_018), "2,140.18"),
		(-61_233, "\u{2212}612.33"),
		(0, "0.00"),
		(5, "0.05"),
		(-5, "\u{2212}0.05"),
		(99_999, "999.99"),
		(100_000, "1,000.00"),
		(-980_000, "\u{2212}9,800.00"),
		(123_456_789_012, "1,234,567,890.12")
	])
	func amountsUseGroupingAndARealMinusSign(minorUnits: Int64, expected: String) {
		#expect(format.amount(minorUnits, on: .register) == expected)
		#expect(format.amount(minorUnits, on: .budget) == expected, "a 2-place currency has one scale")
	}

	@Test func currencyPutsTheMinusBeforeTheSymbol() {
		#expect(format.currency(31_240, on: .budget) == "$312.40")
		#expect(format.currency(-3_475, on: .budget) == "\u{2212}$34.75")
	}

	@Test func theExtremesOfTheRangeDontTrap() {
		#expect(format.amount(.min, on: .register) == "\u{2212}92,233,720,368,547,758.08")
		#expect(format.amount(.max, on: .register) == "92,233,720,368,547,758.07")
		#expect(MoneyFormat.yen.amount(.min, on: .register) == "\u{2212}92,233,720,368,547,758")
	}

	@Test func aHyphenNeverStandsInForMinus() {
		#expect(!format.amount(-1, on: .register).contains("-"))
	}

	@Test func separatorsFollowTheLocale() {
		let german = MoneyFormat(locale: Locale(identifier: "de_DE"), currencySymbol: "€")
		#expect(german.amount(214_018, on: .register) == "2.140,18")
	}
}

/// CLS-12.3, CLS-12.9 and CLS-12.12, on made-up JPY amounts.
struct ZeroPlaceMoneyFormatTests {
	let yen = MoneyFormat.yen

	@Test func aBudgetCellShowsWholeYen() {
		#expect(yen.currency(5_000, on: .budget) == "¥5,000")
		#expect(yen.amount(30_000, on: .budget) == "30,000")
	}

	@Test func theEnvelopeSheetShowsItsMixedNumbersOnTheBudgetScale() {
		#expect(yen.amount(-50_000, on: .budget) == "\u{2212}50,000")
		#expect(yen.currency(-20_000, on: .budget) == "\u{2212}¥20,000")
	}

	@Test func aRegisterAmountIsDividedByAHundredAndRoundedHalfAwayFromZero() {
		#expect(yen.currency(50_000, on: .register) == "¥500")
		#expect(yen.currency(50_049, on: .register) == "¥500")
		#expect(yen.currency(50_050, on: .register) == "¥501")
		#expect(yen.currency(-50_050, on: .register) == "\u{2212}¥501")
	}

	@Test func anAmountThatRoundsToZeroHasNoMinus() {
		#expect(yen.amount(-49, on: .register) == "0")
	}

	@Test func wholeUnitsRoundOnTheNumbersOwnScale() {
		#expect(yen.wholeCurrency(7_089, on: .budget) == "¥7,089", "B is already whole yen (CLS-12.4)")
		#expect(yen.wholeCurrency(708_950, on: .register) == "¥7,090")
	}
}

struct BudgetMoneyFormatTests {
	static let none = NumberFormat(currency: nil, style: .commaDot, hideDecimals: false)

	@Test(arguments: [
		(NumberStyle.commaDot, "2,140.18"),
		(.dotComma, "2.140,18"),
		(.spaceComma, "2\u{202F}140,18"),
		(.apostropheDot, "2'140.18")
	])
	func theBudgetsStyleSetsTheSeparators(style: NumberStyle, expected: String) {
		#expect(MoneyFormat(NumberFormat(currency: nil, style: style, hideDecimals: false), currency: nil).amount(214_018, on: .budget) == expected)
	}

	@Test func noCurrencyMeansNoSymbolAndTwoPlaces() {
		let format = MoneyFormat(Self.none, currency: nil)
		#expect(format.currency(31_240, on: .budget) == "312.40")
		#expect(format.decimalPlaces == 2)
	}

	@Test func theSymbolGoesWhereTheCurrencyTableSays() {
		let euro = MoneyFormat(
			NumberFormat(currency: "EUR", style: .dotComma, hideDecimals: false),
			currency: Currency(code: "EUR", name: "Euro", symbol: "€", decimalPlaces: 2, numberFormat: .dotComma, symbolFirst: false)
		)
		#expect(euro.currency(-123_456, on: .budget) == "\u{2212}1.234,56€")
	}

	@Test func hiddenDecimalsShowNone() {
		let hidden = MoneyFormat(NumberFormat(currency: "USD", style: .commaDot, hideDecimals: true), currency: .dollar)
		#expect(hidden.currency(41_250, on: .budget) == "$413")
		#expect(hidden.amount(-41_249, on: .register) == "\u{2212}412")
		#expect(hidden.decimalPlaces == 2, "amount fields still read the currency's places")
	}

	@Test func theCurrencyRowSetsThePlaces() {
		let yen = MoneyFormat(NumberFormat(currency: "JPY", style: .commaDot, hideDecimals: false), currency: .yen)
		#expect(yen == MoneyFormat.yen)
	}
}

struct IndianGroupingTests {
	@Test(arguments: [(Int64(10_000_033), "1,00,000.33"), (99_999_999, "9,99,999.99"), (123_456_789_00, "12,34,56,789.00"), (100_000, "1,000.00"), (5, "0.05")])
	func lakhAndCroreGrouping(minorUnits: Int64, expected: String) {
		#expect(MoneyFormat(NumberFormat(currency: nil, style: .commaDotIn, hideDecimals: false), currency: nil).amount(minorUnits, on: .register) == expected)
	}
}

extension Currency {
	static let dollar = Currency(code: "USD", name: "US Dollar", symbol: "$", decimalPlaces: 2, numberFormat: .commaDot, symbolFirst: true)
	static let yen = Currency(code: "JPY", name: "Japanese Yen", symbol: "¥", decimalPlaces: 0, numberFormat: .commaDot, symbolFirst: true)
}

extension MoneyFormat {
	static let yen = MoneyFormat(currencySymbol: "¥", decimalPlaces: 0)
}
