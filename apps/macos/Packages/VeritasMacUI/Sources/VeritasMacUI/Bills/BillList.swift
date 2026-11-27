import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Every bill in one state group, as the mockup's columns. A row opens the
/// editor; a bill Actual holds in a shape Veritas can't show stays read-only.
struct BillList: View {
	let title: String
	let bills: [BillSummary]
	let model: BillsModel

	/// Each column's widest; every one but Next gives up to 40% of it before the bill's name truncates.
	enum Column {
		static let name: CGFloat = 140
		static let amount: CGFloat = 190
		static let when: CGFloat = 170
		static let route: CGFloat = 190
		static let posting: CGFloat = 170
		static let last: CGFloat = 110
	}

	private var isStopped: Bool { bills.contains { $0.state != .active } }

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			SectionTitle(title)
			if bills.isEmpty {
				Text("No bills yet.")
					.font(.text(.body))
					.foregroundStyle(.textSecondary)
					.padding(.leading, Spacing.s1)
			} else {
				header
				GroupedRows {
					ForEach(bills, id: \.id) { bill in
						BillRow(bill: bill, model: model)
						if bill.id != bills.last?.id {
							Divider().padding(.leading, Spacing.s4)
						}
					}
				}
			}
		}
	}

	private var header: some View {
		HStack(spacing: Spacing.s4) {
			Text("Bill").frame(minWidth: Column.name, maxWidth: .infinity, alignment: .leading)
			Text("Amount").billColumn(Column.amount, alignment: .trailing)
			Text("When").billColumn(Column.when, alignment: .leading)
			Text("From → To").billColumn(Column.route, alignment: .leading)
			Text(isStopped ? "State" : "Posting").billColumn(Column.posting, alignment: .leading)
			Text(isStopped ? "" : "Next").frame(width: Column.last, alignment: .trailing)
		}
		.font(.text(.caption))
		.foregroundStyle(.textTertiary)
		.padding(.horizontal, Spacing.s4)
		.accessibilityHidden(true)
	}
}

private struct BillRow: View {
	let bill: BillSummary
	let model: BillsModel

	/// MIG-6.2: the Veritas display name first, then Actual's, then the payee.
	private var name: String { bill.displayName ?? bill.name ?? bill.payeeName ?? "Unnamed bill" }
	private var isActive: Bool { bill.state == .active }
	private var canEdit: Bool { bill.draft != nil && !model.isReadOnly }
	private var amount: BillAmount? { bill.draft?.amount ?? bill.scheduledAmount.map { .exact(amount: $0) } }
	private var when: String { bill.draft.map { BillWords.schedule($0.date) } ?? bill.nextDate.map(BillWords.day) ?? "" }
	private var postsEstimate: Bool { bill.autoPost && amount?.isEstimate == true }
	private var posting: String {
		if !isActive { return BillWords.state(bill.state) }
		if postsEstimate { return "Auto-posts an estimate" }
		return bill.autoPost ? "Auto-post" : "Confirm first"
	}
	private var problem: String? {
		guard let problem = model.actionProblem, problem.billId == bill.id else { return nil }
		return problem.message
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s1) {
			HStack(spacing: Spacing.s4) {
				HStack(spacing: Spacing.s2) {
					if let icon = GoalIcon(bill.icon) {
						GoalIconView(icon: icon, size: 14)
					}
					Text(name)
						.font(.text(.body, weight: .semibold))
						.lineLimit(1)
				}
				.frame(minWidth: BillList.Column.name, maxWidth: .infinity, alignment: .leading)
				Group {
					if let amount {
						BillAmountLabel(amount: amount, isIncome: bill.isIncome, format: model.format)
					}
				}
				.billColumn(BillList.Column.amount, alignment: .trailing)
				Text(when)
					.foregroundStyle(.textSecondary)
					.lineLimit(1)
					.billColumn(BillList.Column.when, alignment: .leading)
				Text(model.route(bill))
					.foregroundStyle(.textSecondary)
					.lineLimit(1)
					.billColumn(BillList.Column.route, alignment: .leading)
				postingLabel
					.billColumn(BillList.Column.posting, alignment: .leading)
				Group {
					if isActive {
						Text(bill.nextDate.map(BillWords.day) ?? "")
							.font(.figures(.body))
							.foregroundStyle(.textSecondary)
							.lineLimit(1)
					} else {
						Button("Resume") { Task { await model.resume(bill.id) } }
							.controlSize(.small)
							.disabled(model.isReadOnly)
							.accessibilityIdentifier("bill.\(bill.id).resume")
					}
				}
				.frame(width: BillList.Column.last, alignment: .trailing)
			}
			.frame(height: Size.row)
			if let problem {
				ProblemNote(text: problem)
					.padding(.bottom, Spacing.s2)
					.accessibilityIdentifier("bill.\(bill.id).problem")
			}
		}
		.font(.text(.body))
		.padding(.horizontal, Spacing.s4)
		.contentShape(.rect)
		.onTapGesture { if canEdit { Task { await model.edit(bill) } } }
		.contextMenu { menu }
		.accessibilityElement(children: .contain)
		.accessibilityLabel(accessibilityText)
		.accessibilityAddTraits(canEdit ? .isButton : [])
		.accessibilityIdentifier("bill.\(bill.id)")
	}

	@ViewBuilder private var postingLabel: some View {
		if isActive {
			Label {
				Text(posting)
			} icon: {
				Image(systemName: postsEstimate ? "exclamationmark.triangle" : bill.autoPost ? "bolt" : "hand.raised")
					.imageScale(.small)
			}
			.foregroundStyle(postsEstimate ? AnyShapeStyle(.moneyLow) : AnyShapeStyle(.textSecondary))
			.lineLimit(1)
		} else {
			Text(posting)
				.foregroundStyle(.textSecondary)
		}
	}

	@ViewBuilder private var menu: some View {
		if canEdit {
			Button("Edit…") { Task { await model.edit(bill) } }
		} else if bill.draft == nil {
			Button("Edit in Actual") {}
				.disabled(true)
		}
		if isActive {
			Button("Pause") { Task { await model.pause(bill.id) } }
				.disabled(model.isReadOnly)
			Button("End") { Task { await model.end(bill.id) } }
				.disabled(model.isReadOnly)
		} else {
			Button("Resume") { Task { await model.resume(bill.id) } }
				.disabled(model.isReadOnly)
		}
	}

	private var accessibilityText: String {
		let amountWords = amount.map { BillAmountLabel.words($0, isIncome: bill.isIncome, format: model.format) }
		let next = isActive ? bill.nextDate.map { "next \(BillWords.day($0))" } : nil
		return [name, amountWords, when, model.route(bill), posting, next].compactMap(\.self).joined(separator: ", ")
	}
}

private extension View {
	/// A bill column at its width, narrowing to 60% of it when the list is narrow.
	func billColumn(_ width: CGFloat, alignment: Alignment) -> some View {
		frame(minWidth: width * 0.6, maxWidth: width, alignment: alignment)
	}
}
