import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// The Bills and Rules screens' models doing what their menus and sheets do,
/// against the real core on fixtures/synthetic/all-features: Post for a
/// date against Post today, Received with an edited amount, an amount kind
/// changed in the editor, a bill only Actual can edit, the schedule
/// problem banner, and Apply to existing around a reconciled row.
@MainActor
struct RealCoreBillsScreenTests {
	typealias F = AllFeatures

	private func create(
		_ model: BillsModel, _ name: String, amount: BillAmount, start: String, isIncome: Bool = false, autoPost: Bool = false
	) async throws -> BillSummary {
		let payee = try #require(model.names.payees.first { $0.transferAccountId == nil }).id
		model.startNew(isIncome: isIncome)
		model.editing?.draft.name = name
		model.editing?.draft.payeeId = payee
		model.editing?.draft.accountId = F.checking
		model.editing?.draft.amount = amount
		model.editing?.draft.autoPost = autoPost
		model.editing?.firstDate = start
		#expect(await model.save(), "\(name): \(String(describing: model.editing?.problem))")
		return try #require(model.bills.first { $0.name == name }, "\(name) is listed")
	}

	private func posted(_ budget: RealBudget, _ bill: String) async throws -> [RegisterRow] {
		try await budget.fromDisk { try RealCoreBillsTests.rows($0, schedule: bill) }
	}

	@Test func postForItsDateAndPostTodayEachPostOnTheirOwnDayAndReadAsActualCountsThem() async throws {
		try await RealBudget.with { budget in
			let model = BillsModel(workspace: budget.workspace)
			await model.load()
			let today = model.today
			let inTwo = try RealCoreBillsTests.day(today, plus: 2)
			let inThree = try RealCoreBillsTests.day(today, plus: 3)
			let gym = try await create(model, "Gym bag", amount: .exact(amount: 3_000), start: inTwo)
			let fine = try await create(model, "Library fine", amount: .exact(amount: 1_250), start: inThree)

			let gymDue = try #require(model.tray.first { $0.billId == gym.id })
			#expect(gymDue.date == inTwo)
			#expect(gymDue.date != today, "the menu offers \(BillWords.postFor(gymDue.date))")
			#expect(await model.postNow(gym.id, datedToday: false), "\(String(describing: model.actionProblem?.message))")
			#expect(try await posted(budget, gym.id).map(\.date) == [inTwo], "Post for the date posts on that date")
			#expect(try await posted(budget, gym.id).map(\.amount) == [-3_000])
			#expect(model.bill(gym.id)?.status == .paid, "Actual counts the occurrence paid")
			#expect(!model.tray.contains { $0.billId == gym.id }, "so it leaves the tray")
			#expect(model.bill(gym.id)?.nextDate == inTwo, "the next date moves at the service's next run, as in Actual")

			#expect(await model.postNow(fine.id, datedToday: true), "\(String(describing: model.actionProblem?.message))")
			#expect(try await posted(budget, fine.id).map(\.date) == [today], "Post today posts today")
			// Actual's getStatus counts a post only from two days before the
			// next date (getScheduleOccurrenceMatchStartDate), so a row posted
			// today for an occurrence three days out leaves it upcoming.
			#expect(model.bill(fine.id)?.status == .upcoming, "as Actual reads it")
			#expect(model.tray.first { $0.billId == fine.id }?.date == inThree)
			#expect(budget.workspace.alert == nil, "\(String(describing: budget.workspace.alert))")
		}
	}

	@Test func receivedWithAnEditedAmountPostsThatIncomeAndTheEditorKeepsEachKindsSign() async throws {
		try await RealBudget.with { budget in
			let model = BillsModel(workspace: budget.workspace)
			await model.load()
			let today = model.today
			let salary = try await create(model, "Salary", amount: .between(low: 240_000, high: 260_000), start: today, isIncome: true)
			let due = try #require(model.tray.first { $0.billId == salary.id })
			#expect(due.isIncome)
			#expect(BillWords.paid(isIncome: due.isIncome) == "Received")
			#expect(BillWords.markPaid(isIncome: due.isIncome) == "Mark received")
			#expect(due.scheduledAmount == 250_000, "the form opens on the middle of the range")
			#expect(await model.pay(due, magnitude: 251_234, date: today), "\(String(describing: model.actionProblem?.message))")
			let rows = try await posted(budget, salary.id)
			#expect(rows.map(\.amount) == [251_234], "income posts positive, at the amount typed")
			#expect(rows.map(\.date) == [today])

			// About, then exact, then back to a range, all saved with the income sign.
			for amount in [BillAmount.about(amount: 255_000), .exact(amount: 250_500), .between(low: 245_000, high: 260_000)] {
				await model.edit(try #require(model.bill(salary.id)))
				#expect(model.editing?.draft.amount.low ?? -1 >= 0, "the editor shows magnitudes")
				model.editing?.draft.amount = amount
				#expect(await model.save(), "\(amount): \(String(describing: model.editing?.problem))")
				let saved = try #require(model.bill(salary.id)?.draft?.amount)
				#expect(saved.kind == amount.kind, "\(amount)")
				#expect(saved.low > 0 && saved.high > 0, "\(saved) stays income")
			}

			// v13: a range is ordered by size, and the larger amount first is refused.
			await model.edit(try #require(model.bill(salary.id)))
			model.editing?.draft.amount = .between(low: 260_000, high: 245_000)
			#expect(await model.save() == false)
			#expect(model.editing?.problem == "Put the lower amount first")
			model.editing = nil

			let power = try await create(model, "Power", amount: .about(amount: 9_000), start: today)
			await model.edit(power)
			model.editing?.draft.amount = .between(low: 8_000, high: 11_000)
			#expect(await model.save(), "\(String(describing: model.editing?.problem))")
			let range = try #require(model.bill(power.id)?.draft?.amount)
			#expect(range.kind == .between)
			#expect(range.low < 0 && range.high < 0, "a bill's range is saved negative: \(range)")
			#expect(model.bill(power.id)?.scheduledAmount == -9_500)
		}
	}

	@Test func aBillOnlyActualCanEditIsListedButNeverOpensInTheEditor() async throws {
		try await RealBudget.with { budget in
			let model = BillsModel(workspace: budget.workspace)
			await model.load()
			let water = try await create(model, "Water", amount: .exact(amount: 4_200), start: model.today)
			let directory = try budget.directory
			let rule = try #require(try budget.sqlite("SELECT rule FROM schedules WHERE id = '\(water.id)'").first?.first)
			await budget.workspace.closeBudget()
			// Actual's schedule editor lets the amount condition be deleted
			// (shared/schedules.ts getScheduledAmount guards for it); the bill
			// editor has no shape for a bill without an amount.
			try ActualDatabase.execute(
				directory.appending(path: "db.sqlite"),
				"""
				UPDATE rules SET conditions = (SELECT json_group_array(json(value)) FROM json_each(rules.conditions) \
				WHERE json_extract(value, '$.field') != 'amount') WHERE id = '\(rule)'
				"""
			)
			await budget.workspace.reopen(directory: directory)
			try #require(budget.workspace.phase == .open)
			let reopened = BillsModel(workspace: budget.workspace)
			await reopened.load()
			let listed = try #require(reopened.bill(water.id), "still listed")
			#expect(listed.draft == nil, "Veritas can't show this bill's rule, so it's read-only")
			await reopened.edit(listed)
			#expect(reopened.editing == nil, "Edit in Actual: the editor never opens on it")
			if let due = reopened.tray.first(where: { $0.billId == water.id }) {
				_ = await reopened.pay(due, magnitude: 4_000, date: reopened.today)
				#expect(
					reopened.actionProblem == nil || reopened.actionProblem?.billId == water.id,
					"a refusal stays beside the row: \(String(describing: reopened.actionProblem))"
				)
			}
			#expect(budget.workspace.alert == nil, "\(String(describing: budget.workspace.alert))")
		}
	}

	@Test func aScheduleRunThatFailsOnOpenShowsItsBannerRightAwayAndTheNextGoodRunClearsIt() async throws {
		let budgets = URL.temporaryDirectory.appending(path: "veritas-real-\(UUID().uuidString)", directoryHint: .isDirectory)
		defer { try? FileManager.default.removeItem(at: budgets) }
		let library = CoreBudgetLibrary(budgetsDirectory: budgets)
		let today = WorkspaceModel.today()
		let setup = WorkspaceModel(library: library, format: MoneyFormat())
		await setup.importActualZip(RealBudget.fixture("all-features"))
		let directory = try #require(setup.overview?.directory)
		let payee = try await BillsOnOpenTests.disk(directory) { session in
			try #require(try session.payees().first { $0.transferAccountId == nil }).id
		}
		let due = BillDraft(
			name: "Storage unit", payeeId: payee, accountId: F.checking,
			amount: .exact(amount: -150_000), date: .once(date: today), categoryId: F.rent, autoPost: true
		)
		#expect(await setup.perform(.createBill(billId: "storage-today", bill: due)), "\(String(describing: setup.alert))")
		await setup.closeBudget()
		let database = directory.appending(path: "db.sqlite")
		try ActualDatabase.execute(
			database,
			"CREATE TRIGGER no_posting BEFORE INSERT ON transactions BEGIN SELECT RAISE(ABORT, 'disk refused the write'); END"
		)

		let workspace = WorkspaceModel(library: library, format: MoneyFormat())
		await workspace.reopen(directory: directory)
		try #require(workspace.phase == .open)
		#expect(try await BillsOnOpenTests.posts(directory, "storage-today").isEmpty, "the failed run posted nothing")
		#expect(
			workspace.overview?.scheduleProblem != nil,
			"the run on open failed, so the overview the banner reads says why as soon as open returns"
		)
		await workspace.refresh()
		#expect(workspace.overview?.scheduleProblem != nil, "the core knew: a refresh shows it")
		#expect(workspace.alert == nil, "a failed run is a banner, not an alert: \(String(describing: workspace.alert))")
		await workspace.closeBudget()

		try ActualDatabase.execute(database, "DROP TRIGGER no_posting")
		let ticking = WorkspaceModel(
			library: library, format: MoneyFormat(),
			maintenanceInterval: .milliseconds(20), checkingInterval: .milliseconds(20)
		)
		await ticking.reopen(directory: directory)
		try #require(ticking.phase == .open)
		for _ in 0..<200 where ticking.overview?.scheduleProblem != nil {
			try await Task.sleep(for: .milliseconds(10))
		}
		#expect(ticking.overview?.scheduleProblem == nil, "a run that works clears the banner")
		#expect(try await BillsOnOpenTests.posts(directory, "storage-today") == [today])
		await ticking.closeBudget()
	}

	@Test func applyingWithoutTheReconciledRowLeavesItUntouchedAndANotesChangeIsNotFlagged() async throws {
		try await RealBudget.with { budget in
			let directory = try budget.directory
			let refund = RealCoreRulesTests.hardwareRefund
			await budget.workspace.closeBudget()
			try ActualDatabase.execute(
				directory.appending(path: "db.sqlite"),
				"UPDATE transactions SET reconciled = 1, cleared = 1 WHERE id = '\(refund)'"
			)
			await budget.workspace.reopen(directory: directory)
			try #require(budget.workspace.phase == .open)
			let before = try await budget.fromDisk { try $0.registerRow(transactionId: refund) }

			let model = RulesModel(workspace: budget.workspace)
			await model.load()
			model.startNew()
			let draft = RuleDraft(
				stage: .default, conditionsOp: .and,
				conditions: [RuleCondition(field: .importedPayee, op: .contains, value: .text(text: RealCoreRulesTests.hardwareStore))],
				actions: [RuleAction(op: .set, field: .notes, value: .text(text: "returned"), splitIndex: 0)]
			)
			model.editing?.draft = draft
			await model.preview(.draft(draft: draft))
			let applying = try #require(model.applying, "\(String(describing: model.editing?.problem))")
			let locked = try #require(applying.rows.first { $0.id == refund })
			#expect(locked.reconciled)
			#expect(!locked.movesReconciledBalance, "a notes change doesn't move a reconciled balance")
			#expect(!applying.selected.contains(refund), "a reconciled row starts unticked whatever changes")
			#expect(applying.selected.count >= 1)
			#expect(await model.apply(), "\(String(describing: model.applying?.problem))")

			let after = try await budget.fromDisk { try $0.registerRow(transactionId: refund) }
			#expect(after == before, "the unticked reconciled row is untouched")
			let others = applying.rows.filter { $0.id != refund }.map(\.id)
			for id in others {
				let row = try await budget.fromDisk { try $0.registerRow(transactionId: id) }
				#expect(row.notes == "returned", "\(id) was applied")
			}
		}
	}
}
