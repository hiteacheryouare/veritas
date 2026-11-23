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

	@Test func aBillWithMoreThanTheEditorShowsNeverSaysRule() {
		// PLAN §10: Ryan never sees the word "rule" while creating or editing a bill.
		#expect(BillWords.customParts == "Has settings only Actual shows")
		let words = [BillWords.customParts] + [BillAmountKind.about, .between].compactMap(BillWords.estimate)
		#expect(!words.contains { $0.localizedCaseInsensitiveContains("rule") })
	}

	@Test func postForNamesTheOccurrenceDate() {
		#expect(BillWords.postFor("2026-11-12") == "Post for \(BillWords.day("2026-11-12"))")
	}

	@Test func aBillsRuleSaysItsScheduleFromTheCoresBillDate() {
		// The audit's B3: a rule linked to a bill showed Actual's recurrence JSON.
		// The Mac never reads that JSON; the bill's date comes from bills(today:).
		let raw = RuleCondition(field: .date, op: .isApprox, value: .raw(json: #"{"frequency":"monthly"}"#))
		let monthly = BillDate.repeat(repeat: BillRepeat(start: "2026-01-31", interval: 1, frequency: .monthly, patterns: [], weekend: .fridayBefore, end: .afterOccurrences(count: 24)))
		let words = RuleWords.condition(raw, names: .empty, format: MoneyFormat(), schedule: monthly)
		#expect(words.hasSuffix("31st of month, from Jan 31, 2026, 24 times, a weekend date moves to the Friday before"), "\(words)")
		#expect(!words.contains("{"))
		let yearly = BillDate.repeat(repeat: BillRepeat(start: "2024-02-29", interval: 1, frequency: .yearly, patterns: [], weekend: nil, end: .onDate(date: "2030-12-31")))
		#expect(BillWords.sentence(yearly) == "yearly, from Feb 29, 2024, until Dec 31, 2030")
		#expect(BillWords.sentence(.once(date: "2026-12-05")) == "once, Dec 5, 2026")
		let unknown = RuleWords.condition(raw, names: .empty, format: MoneyFormat())
		#expect(unknown.hasSuffix("a repeating date"), "no bill, still no JSON: \(unknown)")
		#expect(RuleWords.value(.raw(json: "\"{{imported_payee}} via card\""), on: .notes, names: .empty, format: MoneyFormat()) == "\"{{imported_payee}} via card\"", "other raw values are unchanged")
	}

	@Test func aNextDateInALaterYearShowsItsYear() {
		// Fred on M13: yearly from 2024-02-29 next falls on 2028-02-29, which must not read as a near date.
		#expect(BillWords.day("2028-02-29", today: "2026-11-22") == "Feb 29, 2028")
		#expect(BillWords.day("2026-12-03", today: "2026-11-22") == "Dec 3")
		#expect(BillWords.day("2027-01-03", today: "2026-11-22") == "Jan 3, 2027")
	}

	@Test func postTodayAsksFirstWhenItWontPayTheOccurrence() {
		#expect(BillWords.postTodayConfirm("2026-11-21", isIncome: false) == "This won't pay the \(BillWords.day("2026-11-21")) bill. Post today anyway?")
		#expect(BillWords.postTodayConfirm("2026-11-21", isIncome: true) == "This won't count as the \(BillWords.day("2026-11-21")) income. Post today anyway?")
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
				payeeId: nil, categoryId: nil, notes: nil, changes: changes, splitLines: [], reconciled: reconciled
			)
			return RulePreviewRow(match: match)
		}
		#expect(row(reconciled: true, .amount).movesReconciledBalance)
		#expect(row(reconciled: true, .account).movesReconciledBalance)
		#expect(row(reconciled: true, .category, .account).movesReconciledBalance)
		#expect(!row(reconciled: true, .category, .notes).movesReconciledBalance)
		#expect(!row(reconciled: false, .amount, .account).movesReconciledBalance)
	}

	@Test func theEditorsMenusAreTheCoresRuleChoices() throws {
		let core = try ruleChoices()
		#expect(RuleMenus.conditionFields == core.conditionFields.map(\.field))
		for choice in core.conditionFields {
			#expect(RuleMenus.ops(for: choice.field) == choice.ops.map(\.op), "\(choice.field)")
		}
		#expect(RuleMenus.ops(for: .amountInflow).contains(.isBetween) == false, "inflow drops isBetween, as Actual's editor does")
		#expect(RuleMenus.actions == [
			.set(.category), .set(.payee), .set(.notes), .set(.cleared), .set(.account), .set(.date), .set(.amount),
			.prependNotes, .appendNotes, .deleteTransaction
		])
		#expect(RuleMenus.splitLineActions == core.splitLineFields.map { .set($0.field) })
		#expect(RuleMenus.allocations == [.fixedAmount, .percent, .remainder])
	}

	@Test func aNewConditionOrActionStartsBlankAtTheKindTheCoreAsks() {
		let today = "2026-11-20"
		#expect(RuleMenus.blank(.payee, .oneOf, today: today) == .ids(ids: []))
		#expect(RuleMenus.blank(.importedPayee, .oneOf, today: today) == .texts(texts: []))
		#expect(RuleMenus.blank(.payee, .contains, today: today) == .text(text: ""))
		#expect(RuleMenus.blank(.date, .is, today: today) == .date(date: today))
		#expect(RuleMenus.blank(.amount, .isBetween, today: today) == .amountRange(num1: 0, num2: 0))
		#expect(RuleMenus.blank(.account, .offBudget, today: today) == .nothing)
		#expect(RuleMenus.blank(.set(.category), line: 0, today: today) == RuleAction(op: .set, field: .category, value: .id(id: nil), splitIndex: 0))
		#expect(RuleMenus.blank(.set(.cleared), line: 0, today: today).value == .bool(value: true))
		#expect(RuleMenus.blank(.appendNotes, line: 0, today: today) == RuleAction(op: .appendNotes, field: nil, value: .text(text: ""), splitIndex: 0))
		#expect(RuleMenus.blank(.deleteTransaction, line: 0, today: today).value == .nothing)
		#expect(RuleMenus.blank(.set(.notes), line: 2, today: today) == RuleAction(op: .set, field: .notes, value: .text(text: ""), splitIndex: 2))
	}

	@Test func choosingTheSameFieldOrOpAgainKeepsWhatWasTyped() {
		let typed = RuleCondition(field: .importedPayee, op: .contains, value: .text(text: "AMZN"))
		var editing = RuleEditing(id: nil, draft: RuleDraft(stage: .default, conditionsOp: .and, conditions: [typed], actions: []))
		editing.setField(.importedPayee, ofCondition: 0, today: "2026-11-20")
		editing.setOp(.contains, ofCondition: 0, today: "2026-11-20")
		#expect(editing.draft.conditions == [typed])
	}
}
