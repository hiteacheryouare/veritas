import Foundation
import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// The data the Accounts, Bills and Rules journeys read from the joined stub,
/// so a change to one feature's stub can't quietly break another's journey.
struct StubJourneyDataTests {
	let session = StubSession(readOnly: nil)

	@Test func eachAccountAndPayeeIsListedOnce() async throws {
		let accounts = try await session.accounts().accounts
		#expect(Set(accounts.map(\.id)).count == accounts.count)
		#expect(Set(accounts.map(\.name)).count == accounts.count, "\(accounts.map(\.name))")
		#expect(accounts.contains { $0.id == "visa" } && accounts.contains { $0.id == "carloan" })
		let payees = try await session.payees()
		#expect(Set(payees.map(\.id)).count == payees.count)
	}

	@Test func theAccountsJourneysStillFindWhatTheyNeed() async throws {
		let defaults = try await session.loanPaymentDefaults("carloan", today: WorkspaceModel.today())
		#expect(defaults.total != nil, "the loan payment fills from the last payment")
		let fernwood = try #require(try await session.payees().first { $0.name == "Fernwood Market" })
		#expect(try await session.payeeMemory(fernwood.id) != nil, "quick entry fills from the payee's last transaction")
	}

	@Test func theStubsGoalSuggestionsFollowDebt81() async throws {
		// DEBT-8.1: ceil((target − available) ÷ months left), Nov 2026 through each target month.
		let goals = try await session.goals(today: "2026-11-20").goals
		for goal in goals where goal.state == .active {
			let monthly = try #require(goal.suggestedMonthly), months = Int64(try #require(goal.monthsLeft))
			let remaining = goal.target - goal.available
			#expect(monthly == (remaining + months - 1) / months, "\(goal.name)")
		}
		let japan = try #require(goals.first { $0.id == "japan" })
		#expect(japan.suggestedMonthly == 33_546 && japan.monthsLeft == 11, "369000 ÷ 11, Nov 2026 to Sep 2027")
		#expect(japan.pace == .behind(neededMonthly: 33_546))
	}

	@Test func aRowTheStubDoesntHoldIsNotFound() async throws {
		await #expect(throws: FfiError.self) { _ = try await session.registerRow("no-such-row") }
	}

	@Test func theRulePreviewMarksItsReconciledRowAsTheCoreDoes() async throws {
		let matches = try await session.rulePreview(.saved(ruleId: "amazon"))
		#expect(matches.filter(\.reconciled).map(\.transactionId) == [StubRules.reconciledRow])
	}
}
