import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Move money (R-XFER-1, design/mockups/move-money.html). Editing a
/// transfer opens it too, since either side edits the pair (R-XFER-3).
struct MoveMoneySheet: View {
	@Bindable var model: MoveMoneyModel
	@State private var isConfirmingGroup = false
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text("Move money")
				.font(.text(.title, weight: .semibold))
			HStack(alignment: .top, spacing: Spacing.s3) {
				end("From", selection: $model.fromID, balance: model.preview?.from, id: "moveMoney.from")
				Text("→")
					.font(.text(.title, weight: .semibold))
					.foregroundStyle(.textSecondary)
					.padding(.top, 26)
					.accessibilityHidden(true)
				end("To", selection: Binding(get: { model.toID ?? "" }, set: { model.toID = $0.isEmpty ? nil : $0 }), balance: model.preview?.to, id: "moveMoney.to")
			}
			GroupedRows {
				LabeledRow("Amount") {
					AmountTextField(field: model.amount, takesFocus: model.isNew) { _ in }
						.frame(width: 140)
						.accessibilityIdentifier("moveMoney.amount")
				}
				if let problem = model.amount.problem {
					Text(problem)
						.font(.text(.caption))
						.foregroundStyle(Color.moneyOverspent)
						.padding(.horizontal, Spacing.s4)
						.padding(.bottom, Spacing.s2)
				}
				Divider()
				LabeledRow("Date") {
					DatePicker("Date", selection: $model.date, displayedComponents: .date)
						.labelsHidden()
						.accessibilityIdentifier("moveMoney.date")
				}
				Divider()
				LabeledRow("Notes") {
					TextField("Notes", text: $model.notes)
						.labelsHidden()
						.multilineTextAlignment(.trailing)
						.textFieldStyle(.plain)
						.frame(width: 300)
						.accessibilityIdentifier("moveMoney.notes")
				}
			}
			if model.isLoanPayment && model.isNew {
				LoanSplit(model: model)
			} else if model.asksPurpose {
				TransferPurposeSection(purpose: model.purpose)
			}
			if let problem = model.problem, model.amount.problem == nil {
				Text(problem).foregroundStyle(Color.moneyOverspent)
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button(model.isNew ? "Move money" : "Save") {
					if model.asksPurpose, model.purpose.unhidePrompt != nil {
						isConfirmingGroup = true
					} else {
						Task { if await model.save() { dismiss() } }
					}
				}
				.buttonStyle(.borderedProminent)
				.keyboardShortcut(.defaultAction)
				.disabled(!model.canSave)
				.accessibilityIdentifier("moveMoney.save")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 600)
		.unhideGroupConfirmation(model.purpose.unhidePrompt, isPresented: $isConfirmingGroup) {
			Task { if await model.save() { dismiss() } }
		}
		.task { await model.load() }
		.task(id: model.previewKey) { await model.refreshPreview() }
		.containerIdentifier("sheet.moveMoney")
	}

	private func end(_ title: String, selection: Binding<String>, balance: BalanceAfter?, id: String) -> some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			Text(title)
				.font(.text(.caption, weight: .semibold))
				.foregroundStyle(.textSecondary)
			Picker(title, selection: selection) {
				if selection.wrappedValue.isEmpty {
					Text("Choose an account").tag("")
				}
				ForEach(model.accounts) { account in
					Text("\(account.name)  \(model.format.currency(account.total, on: .register))").tag(account.id)
				}
			}
			.labelsHidden()
			.accessibilityIdentifier(id)
			if let balance {
				Grid(alignment: .leading, horizontalSpacing: Spacing.s3, verticalSpacing: 2) {
					GridRow {
						Text("Now")
						Text(model.format.currency(balance.now, on: .register)).font(.figures(.body))
					}
					if model.showsAfter {
						GridRow {
							Text("After")
							Text(model.format.currency(balance.after, on: .register))
								.font(.figures(.body, weight: .semibold))
								.foregroundStyle(.textPrimary)
						}
					}
				}
				.foregroundStyle(.textSecondary)
				.padding(.horizontal, Spacing.s3)
				.padding(.vertical, Spacing.s2)
				.frame(maxWidth: .infinity, alignment: .leading)
				.background(.surfaceHover, in: .rect(cornerRadius: Radius.control))
				// A combined label isn't rebuilt when the After row is inserted, so it's spelled out.
				.accessibilityElement(children: .ignore)
				.accessibilityLabel(balanceLabel(balance))
				.accessibilityIdentifier("\(id).balance")
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
	}

	func balanceLabel(_ balance: BalanceAfter) -> String {
		let now = "Now \(model.format.currency(balance.now, on: .register))"
		guard model.showsAfter else { return now }
		return "\(now), After \(model.format.currency(balance.after, on: .register))"
	}
}

/// R-DEBT-2: the payment's interest is typed, and principal is what's left.
private struct LoanSplit: View {
	@Bindable var model: MoveMoneyModel

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			Text("How much is principal, and how much is interest?")
				.font(.text(.heading, weight: .semibold))
			GroupedRows {
				HStack(spacing: Spacing.s3) {
					Text("Interest")
					Spacer()
					AmountTextField(field: model.interest, takesFocus: false) { _ in }
						.frame(width: 120)
						.accessibilityIdentifier("moveMoney.interest")
					EnvelopeMenu(groups: model.envelopes, selection: $model.interestCategoryID)
						.accessibilityIdentifier("moveMoney.interestEnvelope")
				}
				.padding(.horizontal, Spacing.s4)
				.frame(minHeight: Size.row)
				Divider()
				HStack(spacing: Spacing.s3) {
					Text("Principal")
					Spacer()
					Text(model.loan.map { model.format.amount($0.principal, on: .register) } ?? "")
						.font(.figures(.body))
						.foregroundStyle(.textSecondary)
						.frame(width: 120, alignment: .trailing)
						.accessibilityIdentifier("moveMoney.principal")
					EnvelopeMenu(groups: model.envelopes, selection: $model.principalCategoryID)
						.accessibilityIdentifier("moveMoney.principalEnvelope")
				}
				.padding(.horizontal, Spacing.s4)
				.frame(minHeight: Size.row)
				if model.showsEscrowAndFees {
					Divider()
					part("Escrow", field: model.escrow, envelope: $model.escrowCategoryID, id: "moveMoney.escrow")
					Divider()
					part("Fees", field: model.fees, envelope: $model.feesCategoryID, id: "moveMoney.fees")
				}
			}
			if !model.showsEscrowAndFees {
				Button("Add Escrow or Fees") { model.showsEscrowAndFees = true }
					.buttonStyle(.link)
					.accessibilityIdentifier("moveMoney.showEscrow")
			}
			if let loan = model.loan {
				switch loan.problem {
				case .interestAbovePayment(let shortBy)?:
					Text("The interest is \(model.format.currency(shortBy, on: .register)) more than the payment.")
						.foregroundStyle(Color.moneyOverspent)
						.accessibilityIdentifier("moveMoney.loanProblem")
				case .paysOff(let credit)?:
					Text("This pays off the loan and leaves a credit of \(model.format.currency(credit, on: .register)).")
						.accessibilityIdentifier("moveMoney.loanProblem")
				case nil:
					if let estimate = loan.estimate, let rate = loan.rateBps {
						Text("About \(model.format.currency(estimate, on: .register)) is probably interest at \(Self.percent(rate)) a year.")
							.font(.text(.caption))
							.foregroundStyle(.textSecondary)
							.accessibilityIdentifier("moveMoney.estimate")
					}
				}
			}
		}
		.containerIdentifier("moveMoney.loan")
	}

	private func part(_ title: String, field: AmountFieldModel, envelope: Binding<String?>, id: String) -> some View {
		HStack(spacing: Spacing.s3) {
			Text(title)
			Spacer()
			AmountTextField(field: field, takesFocus: false) { _ in }
				.frame(width: 120)
				.accessibilityIdentifier(id)
			EnvelopeMenu(groups: model.envelopes, selection: envelope)
				.accessibilityIdentifier("\(id)Envelope")
		}
		.padding(.horizontal, Spacing.s4)
		.frame(minHeight: Size.row)
	}

	/// Basis points as the mockup writes them: 980 is "9.80%".
	static func percent(_ bps: Int64) -> String {
		String(format: "%lld.%02lld%%", bps / 100, abs(bps % 100))
	}
}

/// R-XFER-4's question, shared by Move money and quick entry.
struct TransferPurposeSection: View {
	@Bindable var purpose: TransferPurposeModel

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			Text("What is this money for?")
				.font(.text(.heading, weight: .semibold))
			Picker("What is this money for?", selection: $purpose.answer) {
				ForEach(TransferPurposeModel.Answer.allCases, id: \.self) { answer in
					Text(answer.title).tag(TransferPurposeModel.Answer?.some(answer))
				}
			}
			.pickerStyle(.radioGroup)
			.labelsHidden()
			.accessibilityIdentifier("purpose.answer")
			if purpose.answer != nil {
				GroupedRows {
					LabeledRow("Envelope") { envelope }
					if purpose.createsEnvelope {
						Divider()
						LabeledRow("Name") {
							TextField("Name", text: $purpose.newEnvelopeName)
								.labelsHidden()
								.multilineTextAlignment(.trailing)
								.textFieldStyle(.plain)
								.frame(width: 220)
								.accessibilityIdentifier("purpose.newName")
						}
					}
				}
				if let count = purpose.reclassifies, count > 0, let id = purpose.categoryID, let name = purpose.name(of: id) {
					Text("\(count) earlier \(count == 1 ? "transaction" : "transactions") in \(name) will count as \(purpose.answer == .saving ? "savings" : "debt paydown").")
						.font(.text(.caption))
						.foregroundStyle(.textSecondary)
						.accessibilityIdentifier("purpose.reclassifies")
				}
			}
		}
		.containerIdentifier("purpose")
	}

	@ViewBuilder private var envelope: some View {
		if let fixed = purpose.fixedEnvelope {
			Text(fixed.name)
				.foregroundStyle(.textSecondary)
				.accessibilityIdentifier("purpose.envelope")
		} else {
			Picker("Envelope", selection: Binding(
				get: { purpose.createsEnvelope ? "new" : purpose.categoryID ?? "" },
				set: { choice in
					if choice == "new" {
						Task { await purpose.chooseNewEnvelope() }
					} else {
						Task { await purpose.choose(choice.isEmpty ? nil : choice) }
					}
				}
			)) {
				Text("Choose an envelope").tag("")
				ForEach(groups, id: \.self) { group in
					Section(group) {
						ForEach(purpose.choices.filter { $0.group == group }) { Text($0.name).tag($0.id) }
					}
				}
				if purpose.needsKind {
					Divider()
					Text("New envelope").tag("new")
				}
			}
			.labelsHidden()
			.frame(width: 220)
			.accessibilityIdentifier("purpose.envelope")
		}
	}

	private var groups: [String] {
		purpose.choices.reduce(into: []) { names, choice in
			if names.last != choice.group { names.append(choice.group) }
		}
	}
}

extension View {
	/// CLS-4.1: a new envelope whose group is hidden is added only once the
	/// user agrees to show the group again. Cancel writes nothing.
	func unhideGroupConfirmation(_ prompt: String?, isPresented: Binding<Bool>, confirm: @escaping () -> Void) -> some View {
		confirmationDialog(prompt ?? "", isPresented: isPresented, titleVisibility: .visible) {
			Button("Show Group and Add", action: confirm)
			Button("Cancel", role: .cancel) {}
		}
	}
}
