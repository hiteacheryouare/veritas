import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// "Apply to existing": the rows the rule matches, each showing only what
/// would change. Reconciled rows start unticked and stay pickable.
struct RuleApplySheet: View {
	@Bindable var model: RulesModel

	var body: some View {
		let applying = model.applying
		let rows = applying?.rows ?? []
		let count = applying?.selected.count ?? 0
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text("Apply to existing")
				.font(.text(.title, weight: .semibold))
			if rows.isEmpty {
				Text("No transactions match this rule.")
					.foregroundStyle(.textSecondary)
					.accessibilityIdentifier("applyRule.empty")
			} else {
				ScrollView {
					GroupedRows {
						ForEach(rows) { row in
							if row.id != rows.first?.id { Divider().padding(.leading, Spacing.s4) }
							ApplyRow(model: model, row: row, isSelected: applying?.selected.contains(row.id) == true)
						}
					}
				}
				.frame(maxHeight: 440)
				.fixedSize(horizontal: false, vertical: true)
			}
			if let problem = applying?.problem {
				ProblemNote(text: problem)
					.accessibilityIdentifier("applyRule.problem")
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { model.applying = nil }
					.keyboardShortcut(.cancelAction)
					.accessibilityIdentifier("applyRule.cancel")
				Button("Apply to \(count)") {
					Task { await model.apply() }
				}
				.buttonStyle(.borderedProminent)
				.keyboardShortcut(.defaultAction)
				.disabled(count == 0 || model.isReadOnly)
				.accessibilityIdentifier("applyRule.apply")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 620)
		.containerIdentifier("sheet.applyRule")
	}
}

private struct ApplyRow: View {
	let model: RulesModel
	let row: RulePreviewRow
	let isSelected: Bool

	private var movesReconciledBalance: Bool { row.reconciled && row.changesAmount }

	var body: some View {
		let match = row.match
		HStack(alignment: .firstTextBaseline, spacing: Spacing.s3) {
			Toggle("Apply to this transaction", isOn: Binding(get: { isSelected }, set: { _ in model.toggle(row.id) }))
				.toggleStyle(.checkbox)
				.labelsHidden()
				.accessibilityIdentifier("applyRule.row.\(row.id).tick")
			VStack(alignment: .leading, spacing: Spacing.s1) {
				HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
					Text(BillWords.day(match.date))
						.frame(width: 96, alignment: .leading)
					Text(model.names.payee(match.payeeId) ?? "")
						.font(.text(.body, weight: .medium))
						.lineLimit(1)
					Text(model.names.account(match.accountId) ?? "")
						.foregroundStyle(.textSecondary)
						.lineLimit(1)
					Spacer(minLength: Spacing.s2)
					if row.reconciled {
						Label("Reconciled", systemImage: "lock.fill")
							.labelStyle(.iconOnly)
							.foregroundStyle(.textSecondary)
							.accessibilityIdentifier("applyRule.row.\(row.id).locked")
					}
					Text(model.format.amount(match.amount, on: .register))
						.font(.figures(.body))
				}
				ForEach(Array(match.changes.enumerated()), id: \.offset) { _, change in
					changeLine(change)
				}
				ForEach(Array(match.splitLines.enumerated()), id: \.offset) { index, line in
					splitLine(index + 1, line)
				}
				if movesReconciledBalance {
					Text("Changes a reconciled balance")
						.font(.text(.caption, weight: .semibold))
						.foregroundStyle(.moneyOverspent)
						.accessibilityIdentifier("applyRule.row.\(row.id).reconciledWarning")
				}
			}
		}
		.font(.text(.body))
		.padding(.horizontal, Spacing.s4)
		.padding(.vertical, Spacing.s2)
		.background(movesReconciledBalance ? AnyShapeStyle(Color.moneyOverspent.opacity(0.08)) : AnyShapeStyle(.clear))
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier("applyRule.row.\(row.id)")
	}

	private func changeLine(_ change: RuleFieldChange) -> some View {
		let isAmount = change.field == .amount
		return HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
			Text(RuleWords.changed(change.field))
				.foregroundStyle(.textSecondary)
				.frame(width: 96, alignment: .leading)
			Text(shown(change.before, on: change.field))
				.foregroundStyle(.textSecondary)
			Text("→").foregroundStyle(.textTertiary).accessibilityHidden(true)
			Text(shown(change.after, on: change.field))
				.font(.text(.body, weight: .medium))
		}
		.font(isAmount ? .figures(.body) : .text(.body))
		.foregroundStyle(isAmount && row.reconciled ? Color.moneyOverspent : Color.textPrimary)
	}

	private func splitLine(_ number: Int, _ line: RuleSplitLine) -> some View {
		let parts = [
			model.names.payee(line.payeeId),
			model.names.category(line.categoryId),
			line.notes
		].compactMap(\.self)
		return HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
			Text("Split \(number)")
				.foregroundStyle(.textSecondary)
				.frame(width: 96, alignment: .leading)
			Text(model.format.amount(line.amount, on: .register))
				.font(.figures(.body))
				.frame(minWidth: 70, alignment: .trailing)
			Text(parts.joined(separator: ", "))
		}
	}

	private func shown(_ value: RuleValue, on field: RuleChangedField) -> String {
		RuleWords.change(value, on: field, names: model.names, format: model.format)
	}
}
