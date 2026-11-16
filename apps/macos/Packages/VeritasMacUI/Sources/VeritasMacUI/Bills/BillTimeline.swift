import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Every unpaid occurrence in the next 60 days, oldest first, one group per month.
struct BillTimeline: View {
	let model: BillsModel

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s5) {
			if model.timeline.isEmpty {
				Text("Nothing due in the next 60 days.")
					.font(.text(.body))
					.foregroundStyle(.textSecondary)
					.padding(.leading, Spacing.s1)
			}
			ForEach(model.timelineMonths, id: \.month) { month in
				VStack(alignment: .leading, spacing: Spacing.s2) {
					SectionTitle(BillWords.month(month.month))
					GroupedRows {
						ForEach(Array(month.bills.enumerated()), id: \.offset) { index, item in
							TimelineRow(item: item, model: model)
							if index < month.bills.count - 1 {
								Divider().padding(.leading, Spacing.s4)
							}
						}
					}
				}
			}
		}
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier("bills.timeline")
	}
}

private struct TimelineRow: View {
	let item: UpcomingBill
	let model: BillsModel

	private var bill: BillSummary? { model.bill(item.billId) }
	private var name: String { item.name ?? item.payeeName ?? "Unnamed bill" }
	private var amount: BillAmount? { bill?.draft?.amount ?? item.scheduledAmount.map { .exact(amount: $0) } }

	var body: some View {
		HStack(spacing: Spacing.s4) {
			Text(BillWords.day(item.date))
				.font(.figures(.body))
				.frame(width: 64, alignment: .leading)
			Text(name)
				.font(.text(.body, weight: .medium))
				.lineLimit(1)
				.frame(maxWidth: .infinity, alignment: .leading)
			Text(bill.map(model.route) ?? "")
				.foregroundStyle(.textSecondary)
				.lineLimit(1)
				.frame(width: 210, alignment: .leading)
			Text(item.status == .upcoming ? "" : BillWords.status(item.status, isIncome: item.isIncome))
				.font(.text(.caption))
				.foregroundStyle(item.status == .missed ? AnyShapeStyle(.moneyOverspent) : AnyShapeStyle(.textSecondary))
				.frame(width: 64, alignment: .leading)
			Group {
				if let amount {
					BillAmountLabel(amount: amount, isIncome: item.isIncome, format: model.format)
				}
			}
			.frame(width: 190, alignment: .trailing)
		}
		.font(.text(.body))
		.padding(.horizontal, Spacing.s4)
		.frame(height: Size.rowDense + Spacing.s2)
		.contentShape(.rect)
		.onTapGesture {
			if let bill, bill.draft != nil, !model.isReadOnly { Task { await model.edit(bill) } }
		}
		.accessibilityElement(children: .ignore)
		.accessibilityLabel([BillWords.day(item.date), name, amount.map { BillAmountLabel.words($0, isIncome: item.isIncome, format: model.format) }].compactMap(\.self).joined(separator: ", "))
		.accessibilityIdentifier("timeline.\(item.billId).\(item.date)")
	}
}
