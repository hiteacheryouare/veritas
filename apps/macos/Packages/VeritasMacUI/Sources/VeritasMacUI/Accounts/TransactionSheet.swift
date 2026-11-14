import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Quick entry (⌘N) and the transaction editor (R-ACC-2, R-ACC-4,
/// design/mockups/quick-entry.html). Choosing a known payee fills its last
/// envelope and amount when those are still empty (D38).
struct TransactionSheet: View {
	@Bindable var model: TransactionEditorModel
	@FocusState private var payeeFocused: Bool
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text(model.isNew ? "Add transaction" : "Edit transaction")
				.font(.text(.title, weight: .semibold))
			GroupedRows {
				LabeledRow("Payee") {
					TextField("Payee", text: Binding(get: { model.payeeText }, set: model.typePayee))
						.labelsHidden()
						.multilineTextAlignment(.trailing)
						.textFieldStyle(.plain)
						.focused($payeeFocused)
						.frame(width: 260)
						.accessibilityIdentifier("transaction.payee")
				}
				if !model.suggestions.isEmpty {
					Divider()
					ForEach(model.suggestions) { payee in
						Button {
							Task { await model.choose(payee) }
						} label: {
							HStack {
								Text(payee.transferAccountId == nil ? payee.name : "Move money to or from \(payee.name)")
								Spacer()
								if payee.favorite { Image(systemName: "star.fill").foregroundStyle(.textTertiary).accessibilityLabel("Favorite") }
							}
							.padding(.horizontal, Spacing.s4)
							.frame(height: 28)
							.contentShape(.rect)
						}
						.buttonStyle(.plain)
						.accessibilityIdentifier("transaction.suggestion.\(payee.id)")
					}
				}
				Divider()
				LabeledRow("Amount") {
					Picker("Direction", selection: $model.isDeposit) {
						Text("Payment").tag(false)
						Text("Deposit").tag(true)
					}
					.pickerStyle(.segmented)
					.labelsHidden()
					.fixedSize()
					.accessibilityIdentifier("transaction.direction")
					AmountTextField(field: model.amount, takesFocus: false) { _ in }
						.frame(width: 120)
				}
				if let problem = model.amount.problem {
					Text(problem)
						.font(.text(.caption))
						.foregroundStyle(Color.moneyOverspent)
						.padding(.horizontal, Spacing.s4)
						.padding(.bottom, Spacing.s2)
				}
				if !model.isSplit && !model.choseTransfer {
					Divider()
					LabeledRow("Envelope") {
						EnvelopeMenu(groups: model.envelopes, selection: $model.categoryID)
							.accessibilityIdentifier("transaction.envelope")
					}
				}
				Divider()
				LabeledRow("Account") {
					Picker("Account", selection: $model.accountID) {
						ForEach(model.accounts) { Text($0.name).tag($0.id) }
					}
					.labelsHidden()
					.frame(width: 220)
					.accessibilityIdentifier("transaction.account")
				}
				Divider()
				LabeledRow("Date") {
					DatePicker("Date", selection: $model.date, displayedComponents: .date)
						.labelsHidden()
						.accessibilityIdentifier("transaction.date")
				}
				Divider()
				LabeledRow("Notes") {
					TextField("Notes", text: $model.notes, prompt: Text("#tags work here"))
						.labelsHidden()
						.multilineTextAlignment(.trailing)
						.textFieldStyle(.plain)
						.frame(width: 300)
						.accessibilityIdentifier("transaction.notes")
				}
				Divider()
				LabeledRow(model.isLocked ? "Reconciled" : "Cleared") {
					if model.isLocked {
						Button("Unlock") { Task { await model.unlock() } }
							.accessibilityIdentifier("transaction.unlock")
					} else {
						Toggle("Cleared", isOn: $model.cleared)
							.labelsHidden()
							.toggleStyle(.switch)
							.accessibilityIdentifier("transaction.cleared")
					}
				}
			}
			if model.isSplit {
				SplitLines(model: model)
			}
			if let problem = model.problem, model.amount.problem == nil {
				Text(problem).foregroundStyle(Color.moneyOverspent)
			}
			HStack {
				if !model.choseTransfer {
					Button(model.isSplit ? "Add Split Line" : "Split") { model.addSplitLine() }
						.accessibilityIdentifier("transaction.split")
				}
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button(model.isNew ? "Add" : "Save") {
					Task { if await model.save() { dismiss() } }
				}
				.buttonStyle(.borderedProminent)
				.keyboardShortcut(.defaultAction)
				.accessibilityIdentifier("transaction.save")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 560)
		.task {
			await model.load()
			if model.isNew { payeeFocused = true }
		}
		.containerIdentifier("sheet.transaction")
	}
}

/// Envelopes by group, income last, as the budget lists them.
struct EnvelopeMenu: View {
	let groups: [EnvelopeGroup]
	@Binding var selection: String?

	var body: some View {
		Picker("Envelope", selection: $selection) {
			Text("Uncategorized").tag(String?.none)
			ForEach(groups, id: \.id) { group in
				Section(group.name) {
					ForEach(group.envelopes.filter { !$0.hidden }, id: \.id) { Text($0.name).tag(String?.some($0.id)) }
				}
			}
		}
		.labelsHidden()
		.frame(width: 220)
	}
}

private struct SplitLines: View {
	let model: TransactionEditorModel

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			Text("Split")
				.font(.text(.heading, weight: .semibold))
			GroupedRows {
				ForEach(model.splits) { line in
					SplitLineRow(line: line, groups: model.envelopes) { model.removeSplitLine(line) }
					if line.id != model.splits.last?.id { Divider() }
				}
			}
		}
		.containerIdentifier("transaction.splits")
	}
}

private struct SplitLineRow: View {
	@Bindable var line: SplitLineEditor
	let groups: [EnvelopeGroup]
	let remove: () -> Void

	var body: some View {
		HStack(spacing: Spacing.s3) {
			EnvelopeMenu(groups: groups, selection: $line.categoryID)
			TextField("Notes", text: $line.notes, prompt: Text("Notes"))
				.labelsHidden()
				.textFieldStyle(.plain)
			AmountTextField(field: line.amount, takesFocus: false) { _ in }
				.frame(width: 110)
			Button(action: remove) { Image(systemName: "minus.circle") }
				.buttonStyle(.borderless)
				.accessibilityLabel("Remove line")
		}
		.padding(.horizontal, Spacing.s4)
		.frame(minHeight: Size.row)
	}
}
