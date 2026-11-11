import Foundation
import Observation
import VeritasCore

/// New-budget setup (R-IO-0, D53, D56): a name, currency and number format,
/// then accounts with starting balances, then the paycheck savings plans.
@MainActor
@Observable
public final class NewBudgetSetup: Identifiable {
	public enum Step: Int, CaseIterable, Sendable {
		case basics
		case accounts
		case paycheckSavings
	}

	public struct AccountDraft: Hashable, Identifiable, Sendable {
		public let id = UUID()
		public var name = ""
		public var balance = ""
		public var type = AccountType.checking
		public var problem: String?

		public init() {}

		var isBlank: Bool {
			name.trimmingCharacters(in: .whitespaces).isEmpty && balance.trimmingCharacters(in: .whitespaces).isEmpty
		}
	}

	public private(set) var step = Step.basics
	public var name = ""
	public private(set) var currencies: [CurrencyOption] = []
	public private(set) var currencyCode = ""
	public var numberStyle = NumberStyle.commaDot
	public var accounts = [AccountDraft()]
	public var hasRetirementPlan = false
	public var hasHealthSavingsAccount = false
	public private(set) var isWorking = false
	public private(set) var problem: String?

	@ObservationIgnored private let library: any BudgetLibrary
	@ObservationIgnored private let preferredCurrencyCode: String

	public init(library: any BudgetLibrary, preferredCurrencyCode: String = Locale.current.currency?.identifier ?? "USD") {
		self.library = library
		self.preferredCurrencyCode = preferredCurrencyCode
	}

	public var canContinue: Bool {
		guard !isWorking else { return false }
		switch step {
		case .basics: return !name.trimmingCharacters(in: .whitespaces).isEmpty && !currencyCode.isEmpty
		case .accounts, .paycheckSavings: return true
		}
	}

	public func loadCurrencies() async {
		do {
			currencies = try await library.currencies()
			let preferred = currencies.first { $0.code == preferredCurrencyCode } ?? currencies.first
			if let preferred { choose(currency: preferred.code) }
		} catch {
			problem = Self.message(for: error)
		}
	}

	/// Picking a currency also picks the number style it usually uses. The user can change it after.
	public func choose(currency code: String) {
		guard let currency = currencies.first(where: { $0.code == code }) else { return }
		currencyCode = currency.code
		numberStyle = currency.numberStyle
	}

	public func addAccount() {
		accounts.append(AccountDraft())
	}

	public func removeAccount(_ id: AccountDraft.ID) {
		accounts.removeAll { $0.id == id }
	}

	public func back() {
		problem = nil
		step = Step(rawValue: step.rawValue - 1) ?? .basics
	}

	/// Moves to the next step, or on the last one returns the budget to create.
	public func advance() async -> NewBudget? {
		guard canContinue else { return nil }
		problem = nil
		switch step {
		case .basics:
			step = .accounts
		case .accounts:
			if await checkAccounts() { step = .paycheckSavings }
		case .paycheckSavings:
			guard let accounts = await startingAccounts() else {
				step = .accounts
				return nil
			}
			return NewBudget(
				name: name.trimmingCharacters(in: .whitespaces),
				currencyCode: currencyCode,
				numberStyle: numberStyle,
				accounts: accounts,
				hasRetirementPlan: hasRetirementPlan,
				hasHealthSavingsAccount: hasHealthSavingsAccount
			)
		}
		return nil
	}

	private func checkAccounts() async -> Bool {
		await startingAccounts() != nil
	}

	/// The core reads each balance, arithmetic included. Blank rows are dropped.
	private func startingAccounts() async -> [StartingAccount]? {
		isWorking = true
		defer { isWorking = false }
		var result: [StartingAccount] = []
		var allGood = true
		for index in accounts.indices {
			accounts[index].problem = nil
			let draft = accounts[index]
			guard !draft.isBlank else { continue }
			let name = draft.name.trimmingCharacters(in: .whitespaces)
			guard !name.isEmpty else {
				accounts[index].problem = "Name this account."
				allGood = false
				continue
			}
			let text = draft.balance.trimmingCharacters(in: .whitespaces)
			do {
				let balance = text.isEmpty ? 0 : try await library.evaluateAmount(text)
				result.append(StartingAccount(name: name, type: draft.type, balance: balance))
			} catch {
				accounts[index].problem = Self.message(for: error)
				allGood = false
			}
		}
		return allGood ? result : nil
	}

	private static func message(for error: any Error) -> String {
		switch ErrorPresentation(error, during: .creating) {
		case .inline(let message): message
		case .alert(let alert): [alert.title + ".", alert.message].compactMap(\.self).joined(separator: " ")
		default: "Veritas hit a bug."
		}
	}
}
