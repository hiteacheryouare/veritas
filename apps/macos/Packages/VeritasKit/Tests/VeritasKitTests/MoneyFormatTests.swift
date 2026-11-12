import Foundation
import Testing
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
