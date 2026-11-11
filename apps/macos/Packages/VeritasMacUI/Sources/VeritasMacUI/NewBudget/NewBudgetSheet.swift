import SwiftUI
import VeritasDesign
import VeritasKit

/// New-budget setup, one sheet with three steps (R-IO-0, D56).
struct NewBudgetSheet: View {
	@Bindable var setup: NewBudgetSetup
	let create: (NewBudget) -> Void
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s5) {
			Text(title)
				.font(.text(.title, weight: .semibold))
			Group {
				switch setup.step {
				case .basics: BasicsStep(setup: setup)
				case .accounts: AccountsStep(setup: setup)
				case .paycheckSavings: PaycheckSavingsStep(setup: setup)
				}
			}
			.frame(maxWidth: .infinity, alignment: .leading)
			if let problem = setup.problem {
				ProblemNote(text: problem)
			}
			actions
		}
		.padding(Spacing.s5)
		.frame(width: 540)
		.task { await setup.loadCurrencies() }
		.accessibilityIdentifier("newBudget.\(stepName)")
	}

	private var title: String {
		switch setup.step {
		case .basics: "New budget"
		case .accounts: "Accounts"
		case .paycheckSavings: "401(k) and HSA"
		}
	}

	private var stepName: String {
		switch setup.step {
		case .basics: "basics"
		case .accounts: "accounts"
		case .paycheckSavings: "paycheckSavings"
		}
	}

	private var actions: some View {
		HStack {
			if setup.step == .basics {
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
			} else {
				Button("Back", action: setup.back)
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
			}
			Spacer()
			if setup.isWorking {
				ProgressView()
					.controlSize(.small)
			}
			Button(setup.step == .paycheckSavings ? "Create budget" : "Continue") {
				Task {
					if let budget = await setup.advance() {
						create(budget)
					}
				}
			}
			.buttonStyle(.borderedProminent)
			.keyboardShortcut(.defaultAction)
			.disabled(!setup.canContinue)
			.accessibilityIdentifier("newBudget.continue")
		}
	}
}

struct ProblemNote: View {
	let text: String

	var body: some View {
		Label {
			Text(text)
		} icon: {
			Image(systemName: MoneyTone.overspent.symbol ?? "exclamationmark.triangle")
				.imageScale(.small)
		}
		.font(.text(.caption))
		.foregroundStyle(.moneyOverspent)
	}
}
