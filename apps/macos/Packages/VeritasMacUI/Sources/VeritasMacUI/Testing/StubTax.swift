#if DEBUG
import Foundation
import VeritasCore

/// The stub budget's tax summary. Filled in by the Tax work.
enum StubTax {
	static func summary(year: Int32) throws -> TaxSummary {
		throw FfiError.Invalid(message: "The stub has no tax summary yet.")
	}
}
#endif
