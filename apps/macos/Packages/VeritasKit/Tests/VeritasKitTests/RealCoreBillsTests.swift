import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// The Bills screen's model against the real core on
/// fixtures/synthetic/all-features (R-BILL-1 to R-BILL-5): what the editor
/// saves, what Paid posts, and the list, tray and timeline in the core's order.
@MainActor
struct RealCoreBillsTests {
	@Test func billsCreatedEditedPaidSkippedPausedEndedAndResumed() async throws {
		try await RealBudget.with { budget in
			let model = BillsModel(workspace: budget.workspace)
			await model.load()
			let today = model.today
			let payee = try #require(model.names.payees.first { $0.transferAccountId == nil }).id

			let phone = try await create(model, "Phone line", payee: payee, amount: .exact(amount: 4_500), start: today)
			#expect(phone.scheduledAmount == -4_500, "a bill's amount is saved negative")
			#expect(!phone.isIncome)
			#expect(phone.draft?.amount == .exact(amount: -4_500))
			#expect(phone.draft.map { BillWords.schedule($0.date) } != nil)

			let power = try await create(model, "Power estimate", payee: payee, amount: .about(amount: 9_000), start: today)
			#expect(power.draft?.amount == .about(amount: -9_000))
			#expect(power.draft?.amount.isEstimate == true)

			let pay = try await create(model, "Salary", payee: payee, amount: .between(low: 240_000, high: 260_000), start: today, isIncome: true)
			#expect(pay.isIncome, "R-BILL-5")
			#expect(pay.scheduledAmount == 250_000, "income is positive and posts the middle of its range")

			// Edit: the editor opens on magnitudes and saves with the bill's sign.
			await model.edit(phone)
			let editing = try #require(model.editing)
			#expect(editing.draft.amount == .exact(amount: 4_500))
			#expect(editing.detail?.nextDates.isEmpty == false)
			model.editing?.draft.amount = .exact(amount: 5_000)
			model.editing?.draft.name = "Phone and internet"
			#expect(await model.save(), "\(String(describing: model.editing?.problem))")
			#expect(model.editing == nil)
			let edited = try #require(model.bill(phone.id))
			#expect(edited.name == "Phone and internet")
			#expect(edited.scheduledAmount == -5_000)

			// Paid with an edited amount and date posts that row through the rules.
			let due = try #require(model.tray.first { $0.billId == phone.id }, "a bill starting today is in the tray")
			let paidOn = try Self.day(today, plus: -1)
			#expect(await model.pay(due, magnitude: 4_321, date: paidOn), "\(String(describing: model.actionProblem?.message))")
			let posted = try await budget.fromDisk { session in
				try Self.rows(session, schedule: phone.id)
			}
			#expect(posted.map(\.amount) == [-4_321])
			#expect(posted.map(\.date) == [paidOn])
			#expect(posted.first?.accountId == AllFeatures.checking)

			// Skip moves the next date on.
			let before = try #require(model.bill(power.id)?.nextDate)
			#expect(await model.skip(power.id), "\(String(describing: model.actionProblem?.message))")
			let after = try #require(model.bill(power.id)?.nextDate)
			#expect(after > before)

			#expect(await model.pause(power.id))
			#expect(model.bill(power.id)?.state == .paused)
			#expect(model.stoppedBills.contains { $0.id == power.id })
			#expect(!model.tray.contains { $0.billId == power.id })

			#expect(await model.end(pay.id))
			#expect(model.bill(pay.id)?.state == .ended)

			#expect(await model.resume(power.id))
			#expect(model.bill(power.id)?.state == .active)
			#expect(model.activeBills.contains { $0.id == power.id })

			#expect(budget.workspace.alert == nil, "\(String(describing: budget.workspace.alert))")
		}
	}

	@Test func aRangeWithTwoEqualAmountsIsShownInTheEditorNotAsAnAlert() async throws {
		try await RealBudget.with { budget in
			let model = BillsModel(workspace: budget.workspace)
			await model.load()
			let payee = try #require(model.names.payees.first { $0.transferAccountId == nil }).id
			let count = model.bills.count
			model.startNew()
			model.editing?.draft.name = "Flat range"
			model.editing?.draft.payeeId = payee
			model.editing?.draft.accountId = AllFeatures.checking
			model.editing?.draft.amount = .between(low: 5_000, high: 5_000)
			#expect(await model.save() == false)
			#expect(model.editing?.problem?.isEmpty == false, "R-BILL-4: the core refuses it and says why")
			#expect(budget.workspace.alert == nil)
			await model.load()
			#expect(model.bills.count == count, "nothing was written")
		}
	}

	@Test func theListTrayAndTimelineComeBackInTheCoresOrder() async throws {
		try await RealBudget.with { budget in
			let model = BillsModel(workspace: budget.workspace)
			await model.load()
			let payee = try #require(model.names.payees.first { $0.transferAccountId == nil }).id
			let today = model.today
			let soon = try Self.day(today, plus: 3)
			let later = try Self.day(today, plus: 40)
			_ = try await create(model, "Later bill", payee: payee, amount: .exact(amount: 1_000), start: later)
			_ = try await create(model, "Soon bill", payee: payee, amount: .exact(amount: 2_000), start: soon)
			let stopped = try await create(model, "Stopped bill", payee: payee, amount: .exact(amount: 3_000), start: soon)
			#expect(await model.pause(stopped.id))

			let (bills, tray, timeline) = try await budget.fromDisk { session in
				(try session.bills(today: today), try session.upcoming(days: BillsModel.trayDays, today: today), try session.upcoming(days: BillsModel.timelineDays, today: today))
			}
			#expect(model.bills.map(\.id) == bills.map(\.id))
			#expect(model.tray == tray)
			#expect(model.timeline == timeline)
			#expect(model.activeBills + model.stoppedBills == model.bills, "active first, then paused and ended")
			#expect(model.tray.contains { $0.name == "Soon bill" })
			#expect(!model.tray.contains { $0.name == "Later bill" })
			#expect(model.timeline.contains { $0.name == "Later bill" })
			#expect(model.timelineMonths.flatMap(\.bills) == model.timeline, "grouping by month keeps the core's order")
		}
	}

	@Test func aBillAskedForFromAnotherScreenOpensInTheEditor() async throws {
		try await RealBudget.with { budget in
			let model = BillsModel(workspace: budget.workspace)
			await model.load()
			let payee = try #require(model.names.payees.first { $0.transferAccountId == nil }).id
			let bill = try await create(model, "Water", payee: payee, amount: .exact(amount: 3_000), start: model.today)
			budget.workspace.pendingBillID = bill.id
			let screen = BillsModel(workspace: budget.workspace)
			await screen.load()
			#expect(screen.editing?.id == bill.id)
			#expect(budget.workspace.pendingBillID == nil, "asked for once")

			// Switching to once and back keeps the start date and saves.
			screen.editing?.repeats = false
			#expect(screen.editing?.draft.date == .once(date: model.today))
			screen.editing?.repeats = true
			#expect(screen.editing?.rule?.start == model.today)
			#expect(await screen.save(), "\(String(describing: screen.editing?.problem))")
		}
	}

	/// Creates a monthly confirm-first bill through the editor, from Checking, starting `start`.
	private func create(
		_ model: BillsModel, _ name: String, payee: String, amount: BillAmount, start: String, isIncome: Bool = false
	) async throws -> BillSummary {
		model.startNew(isIncome: isIncome)
		model.editing?.draft.name = name
		model.editing?.draft.payeeId = payee
		model.editing?.draft.accountId = AllFeatures.checking
		model.editing?.draft.amount = amount
		model.editing?.firstDate = start
		#expect(await model.save(), "\(name): \(String(describing: model.editing?.problem))")
		return try #require(model.bills.first { $0.name == name }, "\(name) is listed")
	}

	static func day(_ date: String, plus days: Int) throws -> String {
		let start = try #require(CivilDate.date(date))
		return CivilDate.text(try #require(Calendar.current.date(byAdding: .day, value: days, to: start)))
	}

	static func rows(_ session: Session, schedule: String) throws -> [RegisterRow] {
		try session.register(
			scope: .all,
			filter: RegisterFilter(search: nil, status: nil, from: nil, to: nil, categoryId: nil, payeeId: nil, uncategorized: false),
			sort: RegisterSort(key: .date, ascending: true),
			offset: 0, limit: 100_000
		).rows.filter { $0.scheduleId == schedule }
	}
}
