import Testing
import VeritasCore
@testable import VeritasKit

struct BillAndRuleWordsTests {
	let names = BudgetNames(payees: [], accounts: [], categoryGroups: [])
	let format = MoneyFormat()

	@Test func incomeReadsReceived() {
		#expect(BillWords.paid(isIncome: false) == "Paid")
		#expect(BillWords.paid(isIncome: true) == "Received")
		#expect(BillWords.markPaid(isIncome: true) == "Mark received")
		#expect(BillWords.paidOn(isIncome: true) == "Received on")
		#expect(BillWords.status(.paid, isIncome: true) == "Received")
		#expect(BillWords.status(.paid, isIncome: false) == "Paid")
		#expect(BillWords.status(.due, isIncome: true) == "Due")
	}

	@Test func postForNamesTheOccurrenceDate() {
		#expect(BillWords.postFor("2026-11-12") == "Post for \(BillWords.day("2026-11-12"))")
	}

	@Test func theEstimateLineFollowsTheAmountKind() {
		#expect(BillWords.estimate(.exact) == nil)
		#expect(BillWords.estimate(.about) == "Auto-post adds a transaction for this amount.")
		#expect(BillWords.estimate(.between) == "Auto-post adds a transaction for the middle of the range.")
	}

	@Test func anEmptyPayeeOrCategoryReadsByName() {
		#expect(RuleWords.change(.id(id: nil), on: .category, names: names, format: format) == "No category")
		#expect(RuleWords.change(.id(id: nil), on: .payee, names: names, format: format) == "No payee")
		#expect(RuleWords.change(.nothing, on: .notes, names: names, format: format) == "No notes")
		#expect(RuleWords.change(.text(text: ""), on: .notes, names: names, format: format) == "No notes")
		#expect(RuleWords.change(.text(text: "Rent"), on: .notes, names: names, format: format) == "\u{201C}Rent\u{201D}")
		#expect(RuleWords.change(.id(id: "household"), on: .category, names: names, format: format) == "household")
	}

	@Test func aReconciledRowMovesItsBalanceWhenAmountOrAccountChanges() {
		func row(reconciled: Bool, _ fields: RuleChangedField...) -> RulePreviewRow {
			let changes = fields.map { RuleFieldChange(field: $0, before: .nothing, after: .nothing) }
			let match = RuleMatch(
				transactionId: "tx", parentId: nil, date: "2026-11-01", accountId: "visa", amount: -1_000,
				payeeId: nil, categoryId: nil, notes: nil, changes: changes, splitLines: []
			)
			return RulePreviewRow(match: match, reconciled: reconciled)
		}
		#expect(row(reconciled: true, .amount).movesReconciledBalance)
		#expect(row(reconciled: true, .account).movesReconciledBalance)
		#expect(row(reconciled: true, .category, .account).movesReconciledBalance)
		#expect(!row(reconciled: true, .category, .notes).movesReconciledBalance)
		#expect(!row(reconciled: false, .amount, .account).movesReconciledBalance)
	}
}
