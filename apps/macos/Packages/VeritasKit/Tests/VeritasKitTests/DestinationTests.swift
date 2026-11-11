import Testing
import VeritasKit

struct DestinationTests {
	@Test func everyDestinationSurvivesSceneStorage() {
		let destinations = Screen.allCases.map(Destination.screen) + [.account(id: "a1b2"), .account(id: "with:colon")]
		for destination in destinations {
			#expect(Destination(rawValue: destination.rawValue) == destination)
		}
	}

	@Test func unknownStoredValuesAreIgnored() {
		#expect(Destination(rawValue: "") == nil)
		#expect(Destination(rawValue: "ledger") == nil)
		#expect(Destination(rawValue: "account:") == nil)
	}

	@Test func commandDigitsFollowTheSidebarOrder() {
		#expect(Screen.sections.map(\.shortcutNumber) == [1, 2, 3, 4, 5, 6])
		#expect(Screen.sections.map(\.title) == ["Budget", "Accounts", "Bills", "Goals", "Reports", "Insights"])
		#expect(Screen.settings.shortcutNumber == nil)
		#expect(Screen.netProfit.shortcutNumber == nil)
	}

	@Test func pagesBelongToTheirSection() {
		#expect(Screen.reports.pages.map(\.title) == ["Dashboard", "Net Profit", "Sankey", "Custom reports"])
		#expect(Screen.insights.pages.map(\.title) == ["Forecast", "What-ifs", "Wrapped", "Statements", "Tax"])
		#expect(Destination.screen(.sankey).section == .reports)
		#expect(Destination.screen(.tax).section == .insights)
		#expect(Destination.account(id: "x").section == .accounts)
		#expect(Destination.screen(.settings).section == .settings)
	}
}
