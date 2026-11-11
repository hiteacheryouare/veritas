import Testing
import VeritasKit

struct CommandPaletteTests {
	let index = PaletteIndex(
		accounts: [
			SidebarAccount(id: "chk", name: "Checking", balance: 214_018),
			SidebarAccount(id: "sav", name: "Savings", balance: 841_200),
			SidebarAccount(id: "car", name: "Car loan", balance: -980_000)
		],
		format: MoneyFormat()
	)

	@Test func anEmptyQueryListsCommandsThenScreensThenAccounts() {
		let titles = index.results(for: "  ").map(\.title)
		#expect(titles.prefix(3) == ["Add transaction", "Move money", "New bill"])
		#expect(titles.suffix(3) == ["Checking", "Savings", "Car loan"])
		#expect(titles.contains("Net Profit"))
	}

	@Test func aTitlePrefixRanksAboveAWordPrefixAboveAMatchInside() {
		#expect(index.results(for: "s").map(\.title).prefix(3) == ["Sankey", "Statements", "Settings"])
		#expect(index.results(for: "lo").map(\.title) == ["Car loan"])
		#expect(index.results(for: "ank").map(\.title) == ["Sankey"])
	}

	@Test func matchingIgnoresCaseAndAccents() {
		#expect(index.results(for: "NET").first?.title == "Net Profit")
		#expect(index.results(for: "chéck").first?.title == "Checking")
	}

	@Test func accountsGoToTheirRegisterAndShowTheirBalance() throws {
		let checking = try #require(index.results(for: "check").first)
		#expect(checking.target == .go(.account(id: "chk")))
		#expect(checking.detail == "2,140.18")
	}

	@Test func addTransactionShowsItsShortcut() {
		#expect(index.results(for: "add").first?.detail == "⌘N")
		#expect(index.results(for: "add").first?.target == .perform(.addTransaction))
	}

	@Test func nothingMatchesNonsense() {
		#expect(index.results(for: "zzzz").isEmpty)
	}
}
