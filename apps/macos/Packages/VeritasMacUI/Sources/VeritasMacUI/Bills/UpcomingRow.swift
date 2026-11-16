import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// One occurrence in the upcoming tray. Paid opens the amount and date in
/// place; Return there marks it paid and Esc puts the row back.
struct UpcomingRow: View {
	let item: UpcomingBill
	let model: BillsModel
	let isSelected: Bool
	let isPaying: Bool
	let openPay: () -> Void
	let closePay: () -> Void

	private var bill: BillSummary? { model.bill(item.billId) }
	private var name: String { item.name ?? item.payeeName ?? "Unnamed bill" }
	private var problem: String? {
		guard let problem = model.actionProblem, problem.billId == item.billId else { return nil }
		return problem.message
	}

	private var amount: BillAmount? {
		bill?.draft?.amount ?? item.scheduledAmount.map { .exact(amount: $0) }
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			HStack(spacing: Spacing.s4) {
				VStack(alignment: .leading, spacing: 1) {
					Text(BillWords.day(item.date))
						.font(.figures(.body, weight: .semibold))
					Text(BillWords.status(item.status))
						.font(.text(.caption))
						.foregroundStyle(item.status == .missed ? AnyShapeStyle(.moneyOverspent) : AnyShapeStyle(.textSecondary))
				}
				.frame(width: 76, alignment: .leading)
				Text(name)
					.font(.text(.body, weight: .semibold))
					.lineLimit(1)
					.frame(maxWidth: .infinity, alignment: .leading)
				if isPaying {
					PayForm(item: item, model: model, close: closePay)
				} else {
					Text(bill.map(model.route) ?? "")
						.foregroundStyle(.textSecondary)
						.lineLimit(1)
						.frame(width: 200, alignment: .leading)
					Group {
						if let amount {
							BillAmountLabel(amount: amount, isIncome: item.isIncome, format: model.format, weight: .semibold)
						}
					}
					.frame(width: 190, alignment: .trailing)
					HStack(spacing: Spacing.s2) {
						Button("Paid", action: openPay)
							.controlSize(.small)
							.accessibilityIdentifier("upcoming.\(item.billId).paid")
						actions
					}
					.disabled(model.isReadOnly)
					.frame(width: 110, alignment: .trailing)
				}
			}
			if let problem {
				ProblemNote(text: problem)
					.padding(.leading, 76 + Spacing.s4)
					.accessibilityIdentifier("upcoming.\(item.billId).problem")
			}
		}
		.font(.text(.body))
		.padding(.horizontal, Spacing.s4)
		.padding(.vertical, isPaying ? Spacing.s3 : Spacing.s2)
		.frame(minHeight: 52)
		.background(isPaying || isSelected ? AnyShapeStyle(.accentSoft) : AnyShapeStyle(.clear))
		.motion(.fast, value: isSelected)
		.accessibilityElement(children: .contain)
		.accessibilityLabel("\(name), \(BillWords.day(item.date)), \(BillWords.status(item.status))")
		.accessibilityAddTraits(isSelected ? .isSelected : [])
		.accessibilityIdentifier("upcoming.\(item.billId)")
	}

	private var actions: some View {
		Menu {
			Button("Skip") { Task { await model.skip(item.billId) } }
			Button("Post Now") { Task { await model.postNow(item.billId, datedToday: false) } }
			Button("Post Today") { Task { await model.postNow(item.billId, datedToday: true) } }
			Divider()
			Button("Pause") { Task { await model.pause(item.billId) } }
			Button("End") { Task { await model.end(item.billId) } }
		} label: {
			Image(systemName: "ellipsis")
		}
		.menuStyle(.borderlessButton)
		.menuIndicator(.hidden)
		.fixedSize()
		.accessibilityLabel("More for \(name)")
		.accessibilityIdentifier("upcoming.\(item.billId).more")
	}
}

/// Paid's amount, opened on the scheduled amount, and its date, opened on today.
private struct PayForm: View {
	let item: UpcomingBill
	let model: BillsModel
	let close: () -> Void
	@State private var field: AmountFieldModel
	@State private var date: Date

	init(item: UpcomingBill, model: BillsModel, close: @escaping () -> Void) {
		self.item = item
		self.model = model
		self.close = close
		_field = State(initialValue: AmountFieldModel(amount: abs(item.scheduledAmount ?? 0), style: model.numberStyle, reader: model.reader))
		_date = State(initialValue: CivilDate.date(model.today) ?? .now)
	}

	var body: some View {
		VStack(alignment: .trailing, spacing: Spacing.s1) {
			fields
			if let problem = field.problem {
				ProblemNote(text: problem)
			}
		}
		.frame(width: 200 + 190 + 110 + 2 * Spacing.s4, alignment: .trailing)
	}

	private var fields: some View {
		HStack(spacing: Spacing.s2) {
			DatePicker("Paid on", selection: $date, displayedComponents: .date)
				.labelsHidden()
				.fixedSize()
				.accessibilityIdentifier("upcoming.\(item.billId).date")
			AmountTextField(field: field) { exit in
				switch exit {
				case .next, .done: markPaid()
				case .cancel: close()
				case .previous: break
				}
			}
			.frame(width: 120, height: Size.controlSmall)
			Button("Mark paid", action: markPaid)
				.buttonStyle(.borderedProminent)
				.controlSize(.small)
				.accessibilityIdentifier("upcoming.\(item.billId).markPaid")
			Button("Cancel", action: close)
				.controlSize(.small)
				.accessibilityIdentifier("upcoming.\(item.billId).cancelPay")
		}
	}

	private func markPaid() {
		Task {
			guard let amount = await field.commit() else { return }
			if await model.pay(item, magnitude: abs(amount), date: CivilDate.text(date)) {
				close()
			}
		}
	}
}
