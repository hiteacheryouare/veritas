#if DEBUG
import Foundation
import VeritasCore
import VeritasKit

/// UI tests launch the app with `-VeritasStub empty` or `-VeritasStub open`, plus
/// optionally `-VeritasStubReadOnly <kind>` or `-VeritasStubImportError <kind>`.
/// The stub stands in for the core until FFI v1 exports `Session`.
struct StubLaunch {
	static let budgetDirectory = URL(filePath: "/stub/budget", directoryHint: .isDirectory)
	static let zip = URL(filePath: "/stub/Budget.zip")

	static let current: StubLaunch? = StubLaunch(defaults: .standard)

	let opensBudgetOnLaunch: Bool
	let library: StubLibrary

	init?(defaults: UserDefaults) {
		guard let mode = defaults.string(forKey: "VeritasStub") else { return nil }
		opensBudgetOnLaunch = mode == "open"
		library = StubLibrary(
			readOnly: defaults.string(forKey: "VeritasStubReadOnly").flatMap(Self.readOnlyKind),
			importError: defaults.string(forKey: "VeritasStubImportError").flatMap(Self.error)
		)
	}

	private static func readOnlyKind(_ name: String) -> ReadOnlyKind? {
		switch name {
		case "newerActualVersion": .newerActualVersion
		case "trackingBudget": .trackingBudget
		case "openElsewhere": .openElsewhere
		case "auditFailed": .auditFailed
		case "newerVeritasSchema": .newerVeritasSchema
		default: nil
		}
	}

	private static func error(_ name: String) -> FfiError? {
		switch name {
		case "upgradeInActualFirst": .UpgradeInActualFirst(required: "26.11.0")
		case "notAnActualBudget": .NotAnActualBudget(message: "The zip has no db.sqlite.")
		case "storage": .Storage(message: "disk I/O error")
		case "io": .Io(message: "Can't read /stub/Budget.zip. Check that the file still exists.")
		case "internal": .Internal(message: "stub bug")
		case "auditFailed": .AuditFailed(violations: 2)
		default: nil
		}
	}
}

struct StubLibrary: BudgetLibrary {
	let readOnly: ReadOnlyKind?
	let importError: FfiError?

	func newBudgetDirectory() -> URL {
		StubLaunch.budgetDirectory
	}

	func open(directory: URL) async throws -> any BudgetSession {
		StubSession(readOnly: readOnly)
	}

	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession {
		if let importError { throw importError }
		return StubSession(readOnly: readOnly)
	}

	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession {
		StubSession(readOnly: readOnly, name: budget.name, accounts: budget.accounts.enumerated().map { index, account in
			AccountSummary(id: "new-\(index)", name: account.name, balance: account.balance)
		})
	}

	func currencies() async throws -> [CurrencyOption] {
		[
			CurrencyOption(code: "USD", name: "US Dollar", symbol: "$", numberFormat: .commaDot),
			CurrencyOption(code: "EUR", name: "Euro", symbol: "€", numberFormat: .dotComma),
			CurrencyOption(code: "CHF", name: "Swiss Franc", symbol: "Fr.", numberFormat: .apostropheDot)
		]
	}

	/// Plain decimals only. The real reader is the core's.
	func evaluateAmount(_ text: String) async throws -> Int64 {
		let plain = text.replacing(",", with: "").trimmingCharacters(in: .whitespaces)
		guard let value = Decimal(string: plain.isEmpty ? "0" : plain) else {
			throw FfiError.Invalid(message: "Can't read \(text) as an amount. Fix it or clear it.")
		}
		return NSDecimalNumber(decimal: value * 100).int64Value
	}
}

final class StubSession: BudgetSession {
	let readOnly: ReadOnlyKind?
	let name: String
	let accountGroups: [AccountGroup]

	init(readOnly: ReadOnlyKind?) {
		self.readOnly = readOnly
		name = "Stub"
		accountGroups = [
			AccountGroup(name: "Cash", accounts: [AccountSummary(id: "checking", name: "Checking", balance: 0)]),
			AccountGroup(name: "Credit", accounts: [AccountSummary(id: "card", name: "Card", balance: 0)])
		]
	}

	init(readOnly: ReadOnlyKind?, name: String, accounts: [AccountSummary]) {
		self.readOnly = readOnly
		self.name = name
		accountGroups = accounts.isEmpty ? [] : [AccountGroup(name: "Accounts", accounts: accounts)]
	}

	let canExport = true

	func overview() async throws -> BudgetOverview {
		BudgetOverview(name: name, directory: StubLaunch.budgetDirectory, readOnly: readOnly, accountGroups: accountGroups)
	}

	func undo() async throws {}

	func redo() async throws {}

	func maintain() async throws {}

	func exportActualZip(to destination: URL) async throws {}

	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {}

	func close() async {}
}
#endif
