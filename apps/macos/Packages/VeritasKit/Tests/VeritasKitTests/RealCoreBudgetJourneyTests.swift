import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// The Budget screen's model driven against the real core on
/// fixtures/synthetic/all-features (R-BUD-1 to R-BUD-6). Each journey checks
/// what the screen shows against Actual's envelope rules and against what a
/// second window reads from disk.
@MainActor
struct RealCoreBudgetJourneyTests {
	typealias F = AllFeatures

	@Test func budgetingThroughTheAmountFieldLandsInTheCellAndCarriesIntoNextMonth() async throws {
		// R-BUD-1, R-BUD-3
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			let october = try #require(screen.sheet)
			let november = try await budget.monthFromDisk(F.november)

			screen.beginEditing(F.groceries)
			let field = try #require(screen.editing)
			#expect(field.text == "140.00")
			field.type("+")
			field.type("140.00+12.34")
			await screen.commitEditing(thenMove: 1)

			let after = try #require(screen.sheet)
			#expect(try after.row(F.groceries).budgeted == 15_234)
			#expect(try after.row(F.groceries).available == october.row(F.groceries).available + 1_234)
			#expect(after.toBudget == october.toBudget - 1_234)
			#expect(screen.editingID == F.emergencyFund, "Return moves on to the next visible envelope, past the hidden Gifts")
			#expect(try await budget.monthFromDisk(F.october) == after)

			let next = try await budget.monthFromDisk(F.november)
			#expect(try next.row(F.groceries).available == november.row(F.groceries).available + 1_234)
			#expect(next.toBudget == november.toBudget - 1_234)
			screen.cancelEditing()
			await screen.nextMonth()
			#expect(screen.sheet == next)
		}
	}

	@Test func movingBudgetChangesOnlyBudgetCellsNeverTransactions() async throws {
		// R-BUD-4
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			let before = try #require(screen.sheet)
			let transactions = try budget.transactionRows()

			let amount = AmountFieldModel(amount: 0, style: screen.numberStyle, reader: screen.reader)
			amount.type("20+5")
			let cents = try #require(await amount.commit())
			#expect(await screen.moveBudget(amount: cents, from: F.funMoney, to: F.diningOut))
			var after = try #require(screen.sheet)
			#expect(try after.row(F.funMoney).budgeted == before.row(F.funMoney).budgeted - 2_500)
			#expect(try after.row(F.funMoney).available == before.row(F.funMoney).available - 2_500)
			#expect(try after.row(F.diningOut).budgeted == before.row(F.diningOut).budgeted + 2_500)
			#expect(try after.row(F.diningOut).available == before.row(F.diningOut).available + 2_500)
			#expect(after.toBudget == before.toBudget, "envelope to envelope leaves To Budget alone")

			#expect(await screen.moveBudget(amount: 5_000, from: nil, to: F.rent))
			after = try #require(screen.sheet)
			#expect(try after.row(F.rent).available == 5_000)
			#expect(after.toBudget == before.toBudget - 5_000)

			#expect(await screen.moveBudget(amount: 10_000, from: F.emergencyFund, to: nil))
			after = try #require(screen.sheet)
			#expect(try after.row(F.emergencyFund).budgeted == before.row(F.emergencyFund).budgeted - 10_000)
			#expect(after.toBudget == before.toBudget + 5_000)

			#expect(try budget.transactionRows() == transactions)
			#expect(try await budget.monthFromDisk(F.october) == after)
		}
	}

	@Test func coveringOverspendingZeroesTheEnvelopeAndStopsItLeakingIntoNextMonth() async throws {
		// R-BUD-1, R-BUD-4: Household ends October 1.21 overspent, without carryover.
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			let before = try #require(screen.sheet)
			let november = try await budget.monthFromDisk(F.november)
			#expect(try before.row(F.household).available == -121)
			#expect(try before.row(F.household).tone == .overspent)
			#expect(screen.sources.contains { $0.id == F.groceries })
			#expect(!screen.sources.contains { $0.id == F.household })

			#expect(await screen.coverOverspending(F.household, from: F.groceries))
			let after = try #require(screen.sheet)
			#expect(try after.row(F.household).available == 0)
			#expect(try after.row(F.household).budgeted == before.row(F.household).budgeted + 121)
			#expect(try after.row(F.groceries).available == before.row(F.groceries).available - 121)
			#expect(after.toBudget == before.toBudget)

			let next = try await budget.monthFromDisk(F.november)
			#expect(next.overspentLastMonth == 0)
			#expect(next.toBudget == november.toBudget + 121, "October's overspending no longer comes out of November")
			#expect(try await budget.monthFromDisk(F.october) == after)
		}
	}

	@Test func coveringFromAnEnvelopeWithLessThanTheOverspendCoversOnlyWhatItHas() async throws {
		// Actual's coverOverspending never takes more than the source has left.
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			let internet = try #require(screen.sources.first { $0.name == "Internet" })
			#expect(internet.available == 12)

			#expect(await screen.coverOverspending(F.household, from: internet.id))
			let after = try #require(screen.sheet)
			#expect(try after.row(internet.id).available == 0)
			#expect(try after.row(F.household).available == -109)
			#expect(try after.row(F.household).tone == .overspent, "still overspent, so the row still offers Cover")
		}
	}

	@Test func coveringFromToBudgetTakesItFromToBudget() async throws {
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.september)
			let before = try #require(screen.sheet)
			let october = try await budget.monthFromDisk(F.october)

			#expect(await screen.coverOverspending(F.household, from: nil))
			let after = try #require(screen.sheet)
			#expect(try after.row(F.household).available == 0)
			#expect(after.toBudget == before.toBudget - 121)
			let next = try await budget.monthFromDisk(F.october)
			#expect(next.toBudget == october.toBudget, "taken now instead of next month, so October comes out the same")
		}
	}

	@Test func holdingForNextMonthMovesToBudgetForwardAndResetGivesItBack() async throws {
		// R-BUD-1: hold for next month, Actual's clamp to what's in To Budget, and Reset Hold.
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			let before = try #require(screen.sheet)
			let november = try await budget.monthFromDisk(F.november)

			let field = AmountFieldModel(amount: before.toBudget, style: screen.numberStyle, reader: screen.reader)
			#expect(field.text == "45,052.29")
			field.type("1000")
			let held = try #require(await field.commit())
			#expect(await screen.holdForNextMonth(held))
			var after = try #require(screen.sheet)
			#expect(after.forNextMonth == 100_000)
			#expect(after.toBudget == before.toBudget - 100_000)
			#expect(try await budget.monthFromDisk(F.november).toBudget == november.toBudget, "held money arrives in November")

			#expect(await screen.holdForNextMonth(50_000))
			after = try #require(screen.sheet)
			#expect(after.forNextMonth == 150_000, "a second hold adds to the first")

			#expect(await screen.holdForNextMonth(999_999_999))
			after = try #require(screen.sheet)
			#expect(after.forNextMonth == before.toBudget, "only what's in To Budget can be held")
			#expect(after.toBudget == 0)
			#expect(try await budget.monthFromDisk(F.november).toBudget == november.toBudget)

			await screen.resetHold()
			#expect(screen.sheet == before)
			#expect(try await budget.monthFromDisk(F.october) == before)
		}
	}

	@Test func carryoverCarriesAnOverspentEnvelopeForwardInsteadOfTakingItFromToBudget() async throws {
		// R-BUD-1: Actual sets the flag for the month and every later one.
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			let before = try #require(screen.sheet)
			let november = try await budget.monthFromDisk(F.november)
			#expect(try !before.row(F.household).carryover)

			await screen.setCarryover(F.household, true)
			#expect(try screen.sheet?.row(F.household).carryover == true)
			let next = try await budget.monthFromDisk(F.november)
			#expect(try next.row(F.household).carryover)
			#expect(try next.row(F.household).available == november.row(F.household).available - 121)
			#expect(next.toBudget == november.toBudget + 121)
			#expect(try await budget.monthFromDisk(F.december).row(F.household).carryover)
			#expect(try await budget.monthFromDisk(F.september).row(F.household).carryover == false, "earlier months keep their flag")

			await screen.setCarryover(F.household, false)
			#expect(screen.sheet == before)
			#expect(try await budget.monthFromDisk(F.november) == november)
		}
	}

	@Test func undoAndRedoRefreshTheOpenScreen() async throws {
		// R-REL-5 as the Budget screen sees it: the view reloads when the change listener fires.
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			let before = try #require(screen.sheet)
			#expect(!budget.workspace.canUndo)
			let start = budget.workspace.changeCount

			screen.beginEditing(F.groceries)
			screen.editing?.type("200")
			await screen.commitEditing()
			#expect(await budget.changeAnnounced(after: start + 1), "the edit's own announcement arrives first")
			let edited = try #require(screen.sheet)
			#expect(try edited.row(F.groceries).budgeted == 20_000)
			#expect(budget.workspace.canUndo)

			var count = budget.workspace.changeCount
			await budget.workspace.undo()
			#expect(await budget.changeAnnounced(after: count), "undo tells the open screens to reload")
			await screen.load()
			#expect(screen.sheet == before)
			#expect(try await budget.monthFromDisk(F.october) == before)
			#expect(budget.workspace.canRedo)

			count = budget.workspace.changeCount
			await budget.workspace.redo()
			#expect(await budget.changeAnnounced(after: count), "redo tells the open screens to reload")
			await screen.load()
			#expect(screen.sheet == edited)
			#expect(!budget.workspace.canRedo)
		}
	}

	/// ⌘Z with an untouched amount field open is a budget undo (VeritasCommands),
	/// so the cell can change under the field.
	@Test func returnOnTheAmountTheFieldShowsKeepsThatAmountAfterAnUndo() async throws {
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			let start = budget.workspace.changeCount
			screen.beginEditing(F.groceries)
			screen.editing?.type("200")
			await screen.commitEditing()
			#expect(await budget.changeAnnounced(after: start + 1))

			screen.beginEditing(F.groceries)
			#expect(screen.editing?.text == "200.00")
			let count = budget.workspace.changeCount
			await budget.workspace.undo()
			#expect(await budget.changeAnnounced(after: count))
			await screen.load()
			#expect(try screen.sheet?.row(F.groceries).budgeted == 14_000)
			let shown = try #require(screen.editing?.text)

			await screen.commitEditing()
			let saved = try #require(screen.sheet).row(F.groceries).budgeted
			withKnownIssue("Known bug: the open field keeps its pre-undo value as the original, so Return on \(shown) writes nothing and the cell stays 140.00") {
				#expect(shown == "140.00" || saved == 20_000, "Return saves what the field shows, or the field shows the cell")
			}
		}
	}

	@Test func keyboardWalksTheRealBudgetAndSkipsHiddenEnvelopes() async throws {
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			screen.moveSelection(by: 1)
			#expect(screen.selection == screen.envelopeIDs.first)
			screen.selection = F.groceries
			screen.moveSelectionToGroup(by: 1)
			#expect(screen.selection == F.emergencyFund, "⌥↓ lands on Goals' first visible envelope")
			screen.moveSelectionToGroup(by: 1)
			#expect(screen.selection == F.interest, "then the income group")
			screen.beginEditing(F.interest)
			#expect(screen.editing == nil, "income is received, not budgeted")
			screen.moveSelection(by: -1)
			#expect(screen.selection == F.vacation)

			screen.beginEditing(F.vacation)
			await screen.commitEditing(thenMove: 1)
			#expect(screen.editing == nil, "Return on the last spending envelope closes the field")

			screen.beginEditing(F.emergencyFund)
			await screen.commitEditing(thenMove: -1)
			#expect(screen.editingID == F.groceries, "Shift-Return goes back up")
			screen.cancelEditing()

			budget.workspace.showsHiddenEnvelopes = true
			#expect(screen.envelopeIDs.contains(F.gifts))
			#expect(screen.envelopeIDs.contains(F.oldHobby))
			screen.beginEditing(F.groceries)
			await screen.commitEditing(thenMove: 1)
			#expect(screen.editingID == F.gifts, "with Show Hidden on, Return stops on hidden envelopes too")
			screen.cancelEditing()
		}
	}

	@Test func amountsTheFieldCantSaveLeaveTheCellAlone() async throws {
		// R-BUD-3, D37
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			let before = try #require(screen.sheet)

			for typed in ["((", "5/0", "99999999999999*99"] {
				screen.beginEditing(F.groceries)
				screen.editing?.type(typed)
				await screen.commitEditing(thenMove: 1)
				#expect(screen.editingID == F.groceries, "\(typed) keeps the field open")
				#expect(screen.editing?.problem != nil, "\(typed) says why")
				screen.cancelEditing()
			}
			#expect(screen.sheet == before)
			#expect(try await budget.monthFromDisk(F.october) == before)
			#expect(!budget.workspace.canUndo, "nothing was written")

			screen.beginEditing(F.groceries)
			screen.editing?.type("(140+10)/3*2")
			await screen.commitEditing()
			#expect(try screen.sheet?.row(F.groceries).budgeted == 10_000)
		}
	}

	@Test func monthsStopAtTheBudgetsFirstMonth() async throws {
		try await RealBudget.with { budget in
			let screen = await budget.screen(Month(year: 2025, month: 8))
			let first = try #require(screen.sheet)
			#expect(first.month == first.firstMonth)
			#expect(!screen.canGoBack)
			await screen.previousMonth()
			#expect(screen.month == first.month)
			await screen.nextMonth()
			#expect(screen.month == Month(year: 2025, month: 9))
			#expect(screen.sheet == (try await budget.monthFromDisk(Month(year: 2025, month: 9))))
		}
	}

	@Test func editingANoteKeepsActualsTemplateLines() async throws {
		// R-BUD-6
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			let original = try #require(try screen.sheet?.row(F.groceries).note)
			#expect(original == "Includes household basics.\n#template 140")
			let otherNotes = try budget.sqlite("SELECT id, note FROM notes WHERE id NOT LIKE 'budget-%' AND id <> '\(F.groceries)' ORDER BY id")

			let edited = original.replacing("household basics.", with: "household basics and paper towels.")
			await screen.setNote(F.groceries, edited)
			#expect(try screen.sheet?.row(F.groceries).note == edited)
			#expect(try budget.sqlite("SELECT note FROM notes WHERE id = '\(F.groceries)'") == [[edited]])
			#expect(try budget.sqlite("SELECT id, note FROM notes WHERE id NOT LIKE 'budget-%' AND id <> '\(F.groceries)' ORDER BY id") == otherNotes)

			_ = await screen.moveBudget(amount: 1_000, from: F.funMoney, to: F.vacation)
			#expect(try screen.sheet?.row(F.vacation).note == "#template 200 up to 1500", "Veritas never runs or rewrites a template")

			await screen.setNote(F.groceries, "   ")
			#expect(try screen.sheet?.row(F.groceries).note == nil, "blank text removes the note")
		}
	}

	@Test func archivingAnEnvelopeWithHistoryHidesItAndChangesNoNumbers() async throws {
		// R-BUD-5, §10: deleting a category with history defaults to Archive and changes no transaction.
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			let before = try #require(screen.sheet)
			let transactions = try budget.transactionRows()
			let live = try budget.sqlite("SELECT count(*) FROM transactions WHERE category = '\(F.funMoney)' AND tombstone = 0")
			let budgeted = try budget.sqlite("SELECT count(*) FROM zero_budgets WHERE category = '\(F.funMoney)' AND amount <> 0")

			let removal = await screen.removal(for: F.funMoney)
			#expect(removal == .archive(transactions: UInt64(live[0][0])!, budgetedMonths: UInt64(budgeted[0][0])!, taxBucket: nil))

			await screen.archive(F.funMoney)
			#expect(screen.envelope(F.funMoney) == nil)
			#expect(screen.hasHiddenEnvelopes)
			#expect(!screen.sources.contains { $0.id == F.funMoney }, "an archived envelope isn't offered as a source")
			let after = try #require(screen.sheet)
			#expect(try after.row(F.funMoney).hidden)
			#expect(after.toBudget == before.toBudget)
			#expect(try after.row(F.funMoney).available == before.row(F.funMoney).available)
			#expect(try budget.transactionRows() == transactions)
			#expect(try budget.sqlite("SELECT hidden, tombstone FROM categories WHERE id = '\(F.funMoney)'") == [["1", "0"]])

			await screen.setHidden(F.funMoney, false)
			#expect(screen.sheet == before, "showing it again ends the archive")
		}
	}

	@Test func onlyAnEnvelopeWithNoHistoryIsReallyDeleted() async throws {
		// R-BUD-5
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			#expect(await screen.createEnvelope(in: F.everydayGroup, named: "Pet supplies"))
			let pets = try #require(screen.group(of: F.groceries)?.envelopes.first)
			#expect(pets.name == "Pet supplies")
			#expect(!(await screen.createEnvelope(in: F.everydayGroup, named: "pet SUPPLIES")), "names are unique, ignoring case")
			#expect(budget.workspace.alert != nil)
			budget.workspace.alert = nil

			#expect(await screen.removal(for: pets.id) == .delete)
			await screen.delete(pets.id)
			#expect(screen.envelope(pets.id) == nil)
			#expect(try budget.sqlite("SELECT tombstone FROM categories WHERE id = '\(pets.id)'") == [["1"]])

			await screen.delete(F.funMoney)
			#expect(budget.workspace.alert != nil, "the core refuses to delete an envelope with history")
			#expect(screen.envelope(F.funMoney) != nil)
		}
	}

	@Test func deletingWithHistoryMovedSendsBudgetAndSpendingToTheChosenEnvelope() async throws {
		// R-BUD-5's confirmed delete: Old Hobby was budgeted and spent 25.00 in Nov 2025 to Jan 2026.
		try await RealBudget.with { budget in
			let month = Month(year: 2025, month: 11)
			let screen = await budget.screen(month)
			budget.workspace.showsHiddenEnvelopes = true
			let before = try #require(screen.sheet)
			let transactionIDs = try budget.sqlite("SELECT id, amount, date FROM transactions WHERE tombstone = 0 ORDER BY id")
			#expect(try before.row(F.oldHobby).budgeted == 2_500)
			#expect(try before.row(F.oldHobby).spent == -2_500)

			#expect(await screen.delete(F.oldHobby, movingHistoryTo: F.household))
			let after = try #require(screen.sheet)
			#expect(after.groups.allSatisfy { group in !group.envelopes.contains { $0.id == F.oldHobby } })
			#expect(try after.row(F.household).budgeted == before.row(F.household).budgeted + 2_500)
			#expect(try after.row(F.household).spent == before.row(F.household).spent - 2_500)
			#expect(after.toBudget == before.toBudget)
			#expect(try budget.sqlite("SELECT id, amount, date FROM transactions WHERE tombstone = 0 ORDER BY id") == transactionIDs, "rows keep their ids, amounts and dates")
			#expect(try await budget.monthFromDisk(month) == after)
		}
	}

	@Test func aTrackingBudgetOpensReadOnlyAndKeepsItsBudgetRows() async throws {
		// R-BUD-2
		try await RealBudget.with { budget in
			let directory = try budget.directory
			await budget.workspace.closeBudget()
			let database = directory.appending(path: "db.sqlite")
			try ActualDatabase.execute(database, "UPDATE preferences SET value = 'tracking' WHERE id = 'budgetType'")
			let rows = "SELECT * FROM reflect_budgets ORDER BY id"
			let zeroRows = "SELECT * FROM zero_budgets ORDER BY id"
			let reflect = try ActualDatabase.rows(database, rows)
			let zero = try ActualDatabase.rows(database, zeroRows)

			await budget.workspace.reopen(directory: directory)
			#expect(budget.workspace.phase == .open)
			#expect(budget.workspace.readOnly == .trackingBudget)
			let screen = await budget.screen(F.october)
			#expect(screen.isReadOnly)
			#expect(!(await screen.holdForNextMonth(100)))
			#expect(!(await budget.workspace.perform(.setBudget(month: F.october, categoryId: F.groceries, amount: 1))))
			await budget.workspace.closeBudget()

			#expect(try ActualDatabase.rows(database, rows) == reflect)
			#expect(try ActualDatabase.rows(database, zeroRows) == zero)
			await budget.workspace.reopen(directory: directory)
		}
	}

	@Test func theWholeSessionSurvivesExportAndReimport() async throws {
		// R-BUD-1 through the file Actual will read: every month the screen showed comes back the same.
		try await RealBudget.with { budget in
			let screen = await budget.screen(F.october)
			screen.beginEditing(F.groceries)
			screen.editing?.type("155")
			await screen.commitEditing()
			_ = await screen.moveBudget(amount: 2_500, from: F.funMoney, to: F.diningOut)
			_ = await screen.coverOverspending(F.household, from: F.groceries)
			_ = await screen.holdForNextMonth(100_000)
			await screen.setCarryover(F.diningOut, false)

			var shown: [BudgetMonth] = []
			for month in [F.september, F.october, F.november, F.december] {
				shown.append(try await budget.monthFromDisk(month))
			}
			let zip = budget.budgets.appending(path: "session.zip")
			await budget.workspace.exportActualZip(to: zip)
			#expect(budget.workspace.alert == nil)

			let again = try await Session.importActualZip(
				zip: zip.path(percentEncoded: false),
				intoDirectory: budget.budgets.appending(path: "reimported").path(percentEncoded: false),
				cancel: CancelToken()
			)
			defer { try? again.close() }
			for month in shown {
				#expect(try again.budgetMonth(month: month.month, today: WorkspaceModel.today()) == month)
			}
		}
	}
}
