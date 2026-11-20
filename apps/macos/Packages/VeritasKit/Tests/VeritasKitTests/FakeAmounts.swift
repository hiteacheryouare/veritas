import Foundation
import VeritasCore
import VeritasKit

/// Stands in for the core's amount reader in tests: plain decimals only.
enum FakeAmounts {
	static let currencies = [
		CurrencyOption(code: "USD", name: "US Dollar", symbol: "$", numberStyle: .commaDot),
		CurrencyOption(code: "EUR", name: "Euro", symbol: "€", numberStyle: .dotComma)
	]

	static let reader = Reader()

	struct Reader: AmountReader {
		func editText(_ amount: Int64, style: NumberStyle) -> String {
			String(format: "%.2f", Double(amount) / 100)
		}

		func fieldText(original: String, typed: String) -> String { typed }

		func evaluate(_ text: String, style: NumberStyle) async throws -> Int64 {
			try FakeAmounts.evaluate(text)
		}
	}

	static func evaluate(_ text: String) throws -> Int64 {
		let trimmed = text.replacing(",", with: "").trimmingCharacters(in: .whitespaces)
		guard trimmed.wholeMatch(of: /-?\d+(\.\d+)?/) != nil, let value = Decimal(string: trimmed) else {
			throw FfiError.Invalid(message: "Can't read \(text) as an amount.")
		}
		return NSDecimalNumber(decimal: value * 100).int64Value
	}
}
