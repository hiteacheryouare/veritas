import Testing
import VeritasCore
@testable import VeritasKit

/// ST-3's balance sheet headings, from Fred's example on M19 (cents, made up).
struct BalanceSheetLinesTests {
	private func account(_ id: String, _ amount: Int64) -> SheetAccount {
		SheetAccount(accountId: id, name: id, amount: amount, closed: false, deleted: false)
	}

	@Test func accountsInNoGroupSitUnderOtherAccountsWithASubtotal() {
		let cash = SheetSection(groupId: "g-cash", groupName: "Cash", accounts: [account("CHK", 3_902_774), account("SAV", 565_060)], total: 4_467_834)
		let none = SheetSection(groupId: nil, groupName: nil, accounts: [account("WAL", 12_000), account("BRK", 2_738_850)], total: 2_750_850)
		let card = SheetSection(groupId: nil, groupName: nil, accounts: [account("VISA", 122_304)], total: 122_304)
		let sheet = BalanceSheet(asOf: "2026-11-22", assets: [cash, none], totalAssets: 7_218_684, liabilities: [card], totalLiabilities: 122_304, netWorth: 7_096_380)
		let lines = StatementsModel.lines(sheet).map { "\($0.title) \($0.amount.map(String.init) ?? "")".trimmingCharacters(in: .whitespaces) }
		#expect(lines == [
			"Assets",
			"Cash", "CHK 3902774", "SAV 565060", "Total cash 4467834",
			"Other accounts", "WAL 12000", "BRK 2738850", "Total other accounts 2750850",
			"Total assets 7218684",
			"Liabilities",
			"Other liabilities", "VISA 122304",
			"Total liabilities 122304",
			"Net worth 7096380"
		])
		#expect(cash.total + none.total == sheet.totalAssets)
		#expect(sheet.totalAssets - sheet.totalLiabilities == sheet.netWorth)
	}
}
