import SwiftUI
import VeritasDesign
import VeritasKit

/// Accounts with their starting balances. Each balance is read by the core,
/// so arithmetic works here as in every amount field.
struct AccountsStep: View {
	@Bindable var setup: NewBudgetSetup

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s3) {
			GroupedRows {
				ForEach($setup.accounts) { $account in
					AccountDraftRow(account: $account, canRemove: setup.accounts.count > 1) {
						setup.removeAccount(account.id)
					}
					if account.id != setup.accounts.last?.id {
						Divider().padding(.leading, Spacing.s4)
					}
				}
			}
			Button {
				setup.addAccount()
			} label: {
				Label("Add account", systemImage: "plus")
			}
			.accessibilityIdentifier("newBudget.addAccount")
		}
	}
}

private struct AccountDraftRow: View {
	@Binding var account: NewBudgetSetup.AccountDraft
	let canRemove: Bool
	let remove: () -> Void

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s1) {
			HStack(spacing: Spacing.s3) {
				TextField("Name", text: $account.name)
					.accessibilityIdentifier("newBudget.account.name")
				TextField("Starting balance", text: $account.balance)
					.font(.figures(.body))
					.multilineTextAlignment(.trailing)
					.frame(width: 150)
					.overlay {
						if account.problem != nil {
							RoundedRectangle(cornerRadius: Radius.field).strokeBorder(Color.moneyOverspent)
						}
					}
					.accessibilityIdentifier("newBudget.account.balance")
				Toggle("Off budget", isOn: $account.isOffBudget)
					.toggleStyle(.checkbox)
					.accessibilityIdentifier("newBudget.account.offBudget")
				Button(action: remove) {
					Image(systemName: "minus.circle")
				}
				.buttonStyle(.borderless)
				.foregroundStyle(.textSecondary)
				.disabled(!canRemove)
				.help("Remove account")
				.accessibilityLabel("Remove account")
			}
			.textFieldStyle(.roundedBorder)
			if let problem = account.problem {
				ProblemNote(text: problem)
			}
		}
		.padding(.horizontal, Spacing.s4)
		.padding(.vertical, Spacing.s2)
	}
}
