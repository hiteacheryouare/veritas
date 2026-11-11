import SwiftUI
import VeritasDesign
import VeritasKit

/// D56: an off-budget account for each plan checked, so paycheck
/// contributions to it count as savings (CLS-10).
struct PaycheckSavingsStep: View {
	@Bindable var setup: NewBudgetSetup

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s3) {
			Toggle("I have a 401(k)", isOn: $setup.hasRetirementPlan)
				.accessibilityIdentifier("newBudget.retirementPlan")
			Toggle("I have an HSA", isOn: $setup.hasHealthSavingsAccount)
				.accessibilityIdentifier("newBudget.healthSavingsAccount")
			Text("Veritas adds an off-budget account for each one you check, so paycheck contributions to it count as savings.")
				.font(.text(.body))
				.foregroundStyle(.textSecondary)
				.fixedSize(horizontal: false, vertical: true)
		}
		.toggleStyle(.checkbox)
	}
}
