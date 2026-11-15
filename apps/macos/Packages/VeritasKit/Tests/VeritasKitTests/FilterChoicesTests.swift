import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

@MainActor
struct FilterChoicesTests {
	@Test func idFieldsPickFromAListAndStoreActualsJSON() async {
		let session = TransferFakeSession()
		let workspace = WorkspaceModel(library: TransferFakeLibraryForFilters(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let choices = FilterChoices(workspace: workspace)
		await choices.load()

		var condition = ReportCondition(field: "category", op: "is", value: "\"\"")
		#expect(choices.options(for: condition)?.map(\.id) == ["savings-a", "savings-b", "carloan-cat", "interest"])
		condition.text = "carloan-cat"
		#expect(condition.value == "\"carloan-cat\"", "an id is stored as Actual's JSON string")
		#expect(condition.text == "carloan-cat")

		condition.op = "contains"
		#expect(choices.options(for: condition) == nil, "contains matches typed text")
		#expect(choices.options(for: ReportCondition(field: "account", op: "isNot", value: "\"\""))?.count == 4)
		#expect(choices.options(for: ReportCondition(field: "payee", op: "is", value: "\"\""))?.isEmpty == true, "transfer payees aren't offered")

		var amount = ReportCondition(field: "amount", op: "gt", value: "0")
		amount.text = "-5000"
		#expect(amount.value == "-5000", "amounts stay bare numbers")
	}

	@Test func aShareWithNothingToDivideByIsBlank() {
		// Never "NaN%" or "∞%".
		#expect(DashboardCard.figure(.nan, percentage: true) == .undefined)
		#expect(DashboardCard.figure(.infinity, percentage: true) == .undefined)
		#expect(DashboardCard.figure(18.4, percentage: true) == .percent(18.4))
		#expect(DashboardCard.figure(-12_345.6, percentage: false) == .money(-12_346))
	}
}

private final class TransferFakeLibraryForFilters: BudgetLibrary, @unchecked Sendable {
	let session: TransferFakeSession
	init(session: TransferFakeSession) { self.session = session }
	func newBudgetDirectory() -> URL { URL(filePath: "/tmp/budgets/new") }
	func open(directory: URL) async throws -> any BudgetSession { session }
	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession { session }
	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession { session }
	func currencies() async throws -> [CurrencyOption] { FakeAmounts.currencies }
	func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64 { try FakeAmounts.evaluate(text) }
}
