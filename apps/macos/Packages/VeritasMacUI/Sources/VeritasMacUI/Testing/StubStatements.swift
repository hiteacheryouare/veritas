#if DEBUG
import Foundation
import VeritasCore

/// The stub budget's statements. Filled in by the Statements work.
enum StubStatements {
	static func balanceSheet(asOf: String) throws -> BalanceSheet {
		throw FfiError.Invalid(message: "The stub has no balance sheet yet.")
	}

	static func ownersEquity(first: String, last: String) throws -> OwnersEquity {
		throw FfiError.Invalid(message: "The stub has no owner's equity yet.")
	}
}
#endif
