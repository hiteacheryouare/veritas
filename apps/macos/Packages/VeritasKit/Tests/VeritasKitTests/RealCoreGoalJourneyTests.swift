import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// The Goals screen's model and the Budget screen's goal cards against the
/// real core on fixtures/synthetic/all-features (R-GOAL-1, R-GOAL-3,
/// R-GOAL-4). Goals plan against today, so these run in the current month.
@MainActor
struct RealCoreGoalJourneyTests {
	typealias F = AllFeatures

	let today = WorkspaceModel.today()
	var current: Month { BudgetScreenModel.currentMonth() }

	/// The 15th of the month `months` after this one.
	func date(monthsAhead months: Int) throws -> (date: Date, text: String) {
		let month = BudgetScreenModel.shift(current, by: months)
		let date = try #require(Calendar.current.date(from: DateComponents(year: Int(month.year), month: Int(month.month), day: 15)))
		return (date, String(format: "%04d-%02d-15", month.year, month.month))
	}

	func purchase(_ amount: Int64, in categoryID: String, payee: String) -> Command {
		.addTransaction(transaction: TransactionDraft(
			id: nil, accountId: F.checking, date: today, amount: -amount, payeeId: nil, payeeName: payee,
			categoryId: categoryID, notes: nil, cleared: true, splits: []
		))
	}

	@Test func aGoalGoesFromCreatedToReachedToSpentToRetargetedToClosedAndBack() async throws {
		// §10: a $1,500 goal due in 6 months suggests $250/mo; buying moves it to reached/spent;
		// closing releases the leftover as a budget move and touches no transaction.
		try await RealBudget.with { budget in
			let goals = await budget.goalsScreen()
			#expect(goals.goals.isEmpty)
			let due = try date(monthsAhead: 5)

			#expect(await goals.create(name: "New laptop", target: 150_000, date: due.date, icon: "laptopcomputer", color: "#34c759"))
			var laptop = try #require(goals.open.first { $0.name == "New laptop" })
			#expect(laptop.state == .active)
			#expect(laptop.targetDate == due.text)
			#expect(laptop.icon == "laptopcomputer")
			#expect(laptop.color == "#34c759")
			#expect(laptop.monthsLeft == 6)
			#expect(laptop.suggestedMonthly == 25_000)
			#expect(laptop.progress == 0)

			let screen = await budget.screen(current)
			#expect(screen.group(of: laptop.categoryId)?.name == "Goals", "a goal is an envelope in the Goals group")
			#expect(screen.goals.map(\.id) == [laptop.id], "and gets a card on the Budget screen")

			screen.beginEditing(laptop.categoryId)
			screen.editing?.type("1,500")
			await screen.commitEditing()
			await goals.load()
			laptop = try #require(goals.goals.first { $0.id == laptop.id })
			#expect(laptop.state == .reached)
			#expect(laptop.reachedMonth == current)
			#expect(laptop.ringProgress == 1)
			#expect(laptop.suggestedMonthly == nil, "a reached goal gets no suggestion")

			#expect(await budget.workspace.perform(purchase(148_000, in: laptop.categoryId, payee: "Apple Store")))
			await goals.load()
			laptop = try #require(goals.goals.first { $0.id == laptop.id })
			#expect(laptop.state == .reachedSpent)
			#expect(laptop.available == 2_000)
			#expect(laptop.spent == 148_000)
			#expect(laptop.wasReached && laptop.ringProgress == 1 && laptop.shownProgress == 150_000)
			#expect(!laptop.canRetarget, "a spent goal is closed or reopened, not re-targeted")

			#expect(await goals.create(name: "Bigger laptop", target: 1, date: nil, icon: nil, color: nil))
			let bigger = try #require(goals.open.first { $0.name == "Bigger laptop" })
			#expect(bigger.suggestedMonthly == nil, "no target date, no suggestion")

			let transactions = try budget.transactionRows()
			await screen.load()
			let groceries = try #require(screen.sheet).row(F.groceries)
			let preview = try #require(await goals.preview(close: laptop, releaseTo: F.groceries))
			#expect(preview.leftover == 2_000)
			#expect(preview.destinationAvailableAfter == groceries.available + 2_000)
			#expect(preview.laterBudget.isEmpty)

			#expect(await goals.close(laptop, releaseTo: F.groceries, alsoMoveLater: false))
			laptop = try #require(goals.archive.first { $0.id == laptop.id })
			#expect(laptop.state == .closed)
			#expect(laptop.closedMonth == current)
			#expect(!goals.open.contains { $0.id == laptop.id })
			await screen.load()
			#expect(screen.envelope(laptop.categoryId) == nil, "closing hides the goal's envelope")
			#expect(!screen.goals.contains { $0.id == laptop.id }, "and drops its card")
			let after = try #require(screen.sheet)
			#expect(try after.row(laptop.categoryId).available == 0)
			#expect(try after.row(laptop.categoryId).budgeted == 150_000 - 2_000)
			#expect(try after.row(F.groceries).budgeted == groceries.budgeted + 2_000)
			#expect(try after.row(F.groceries).available == preview.destinationAvailableAfter)
			#expect(try budget.transactionRows() == transactions, "GOAL-1: closing never touches a transaction")

			await goals.reopen(laptop)
			laptop = try #require(goals.open.first { $0.id == laptop.id })
			#expect(laptop.state == .active, "the money went at close, so it comes back active")
			#expect(laptop.reachedMonth == current, "the first reached month stays as history")
			await screen.load()
			#expect(screen.envelope(laptop.categoryId) != nil)
			#expect(try await budget.goalsFromDisk() == goals.goals)
		}
	}

	@Test func retargetingAReachedGoalMakesItActiveAndKeepsItsReachedMonth() async throws {
		// R-GOAL-3, D71
		try await RealBudget.with { budget in
			let goals = await budget.goalsScreen()
			let due = try date(monthsAhead: 2)
			#expect(await goals.create(name: "Bike", target: 60_000, date: due.date, icon: "bicycle", color: nil))
			var bike = try #require(goals.open.first)
			_ = await budget.screen(current).moveBudget(amount: 60_000, from: nil, to: bike.categoryId)
			await goals.load()
			bike = try #require(goals.open.first)
			#expect(bike.state == .reached)

			let later = try date(monthsAhead: 4)
			let field = AmountFieldModel(amount: bike.target, style: goals.numberStyle, reader: goals.reader)
			#expect(field.text == "600.00")
			field.type("+")
			field.type("600.00+300")
			let target = try #require(await field.commit())
			#expect(await goals.retarget(bike, target: target, date: later.date))
			bike = try #require(goals.open.first)
			#expect(bike.state == .active)
			#expect(bike.target == 90_000)
			#expect(bike.targetDate == later.text)
			#expect(bike.reachedMonth == current)
			#expect(bike.monthsLeft == 5)
			#expect(bike.suggestedMonthly == 18_000, "DEBT-8.1: this month's own budget isn't carry, so 900.00 over five months")
		}
	}

	@Test func closingAnOverspentGoalCoversItFromTheChosenEnvelope() async throws {
		// R-GOAL-4, DEBT-8.3 R-2 and GOAL-3: the destination may go over, and the sheet says so first.
		try await RealBudget.with { budget in
			let goals = await budget.goalsScreen()
			#expect(await goals.create(name: "Concert", target: 20_000, date: nil, icon: nil, color: nil))
			let concert = try #require(goals.open.first)
			let screen = await budget.screen(current)
			_ = await screen.moveBudget(amount: 20_000, from: nil, to: concert.categoryId)
			#expect(await budget.workspace.perform(purchase(25_000, in: concert.categoryId, payee: "Ticket Desk")))
			await goals.load()
			let overspent = try #require(goals.open.first)
			#expect(overspent.available == -5_000)

			await screen.load()
			let fun = try #require(screen.sheet).row(F.funMoney)
			let preview = try #require(await goals.preview(close: overspent, releaseTo: F.funMoney))
			#expect(preview.leftover == -5_000)
			#expect(preview.destinationAvailableAfter == fun.available - 5_000)
			#expect(preview.destinationAvailableAfter < 0, "Fun Money will be over, which the sheet warns about")

			#expect(await goals.close(overspent, releaseTo: F.funMoney, alsoMoveLater: false))
			await screen.load()
			let after = try #require(screen.sheet)
			#expect(try after.row(concert.categoryId).available == 0)
			#expect(try after.row(F.funMoney).budgeted == fun.budgeted - 5_000)
			#expect(try after.row(F.funMoney).available == preview.destinationAvailableAfter)
		}
	}

	@Test func closingToToBudgetAndMovingLaterMonthsToo() async throws {
		// R-GOAL-4, GOAL-10: later months' budget moves only when asked.
		try await RealBudget.with { budget in
			let goals = await budget.goalsScreen()
			#expect(await goals.create(name: "Holiday", target: 100_000, date: try date(monthsAhead: 3).date, icon: nil, color: nil))
			let holiday = try #require(goals.open.first)
			let screen = await budget.screen(current)
			_ = await screen.moveBudget(amount: 30_000, from: nil, to: holiday.categoryId)
			let next = BudgetScreenModel.shift(current, by: 1)
			let nextScreen = await budget.screen(next)
			_ = await nextScreen.moveBudget(amount: 20_000, from: nil, to: holiday.categoryId)
			await goals.load()
			await screen.load()
			let toBudget = try #require(screen.sheet).toBudget

			let funded = try #require(goals.open.first)
			let preview = try #require(await goals.preview(close: funded, releaseTo: nil))
			#expect(preview.leftover == 30_000)
			#expect(preview.destinationAvailableAfter == toBudget + 30_000)
			#expect(preview.laterBudget == [MonthAmount(month: next, amount: 20_000)])

			#expect(await goals.close(funded, releaseTo: nil, alsoMoveLater: true))
			await screen.load()
			#expect(screen.sheet?.toBudget == toBudget + 30_000)
			#expect(try screen.sheet?.row(holiday.categoryId).available == 0)
			await nextScreen.load()
			#expect(try nextScreen.sheet?.row(holiday.categoryId).budgeted == 0, "next month's 200.00 went back too")
			await goals.load()
			#expect(goals.archive.first?.laterBudget.isEmpty == true, "so no closed-goal-has-budget flag")
		}
	}

	@Test func undoingACloseBringsTheGoalAndItsMoneyBackOnScreen() async throws {
		// R-REL-5 through the Goals screen: it reloads when the change listener fires.
		try await RealBudget.with { budget in
			let goals = await budget.goalsScreen()
			#expect(await goals.create(name: "Camera", target: 80_000, date: nil, icon: nil, color: nil))
			let camera = try #require(goals.open.first)
			let screen = await budget.screen(current)
			_ = await screen.moveBudget(amount: 10_000, from: nil, to: camera.categoryId)
			await goals.load()
			let open = goals.goals
			await screen.load()
			let sheet = screen.sheet

			let start = budget.workspace.changeCount
			let funded = try #require(goals.open.first)
			#expect(await goals.close(funded, releaseTo: F.groceries, alsoMoveLater: false))
			#expect(await budget.changeAnnounced(after: start + 1))
			let count = budget.workspace.changeCount
			await budget.workspace.undo()
			#expect(await budget.changeAnnounced(after: count))
			await goals.load()
			await screen.load()
			#expect(goals.goals == open)
			#expect(screen.sheet == sheet)
		}
	}
}
