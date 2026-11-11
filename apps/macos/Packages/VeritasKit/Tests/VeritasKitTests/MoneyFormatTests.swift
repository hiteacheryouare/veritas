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
		#expect(format.amount(minorUnits) == expected)
	}

	@Test func currencyPutsTheMinusBeforeTheSymbol() {
		#expect(format.currency(31_240) == "$312.40")
		#expect(format.currency(-3_475) == "\u{2212}$34.75")
	}

	@Test func theExtremesOfTheRangeDontTrap() {
		#expect(format.amount(.min) == "\u{2212}92,233,720,368,547,758.08")
		#expect(format.amount(.max) == "92,233,720,368,547,758.07")
	}

	@Test func aHyphenNeverStandsInForMinus() {
		#expect(!format.amount(-1).contains("-"))
	}

	@Test func separatorsFollowTheLocale() {
		let german = MoneyFormat(locale: Locale(identifier: "de_DE"), currencySymbol: "€")
		#expect(german.amount(214_018) == "2.140,18")
	}
}

struct BudgetMoneyFormatTests {
	@Test(arguments: [
		(NumberStyle.commaDot, "2,140.18"),
		(.dotComma, "2.140,18"),
		(.spaceComma, "2\u{202F}140,18"),
		(.apostropheDot, "2'140.18")
	])
	func theBudgetsStyleSetsTheSeparators(style: NumberStyle, expected: String) {
		#expect(MoneyFormat(NumberFormat(currency: nil, style: style, hideDecimals: false)).amount(214_018) == expected)
	}

	@Test func noCurrencyMeansNoSymbol() {
		#expect(MoneyFormat(NumberFormat(currency: nil, style: .commaDot, hideDecimals: false)).currency(31_240) == "312.40")
	}
}
