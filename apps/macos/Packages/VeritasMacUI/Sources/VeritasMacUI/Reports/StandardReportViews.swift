import Charts
import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Net worth, Cash flow, Spending by category and Income vs expense (R-REP-5).
struct StandardReportView: View {
	let model: StandardReportModel
	let open: (DrilldownModel) -> Void

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			switch model.kind {
			case .netWorth:
				if let last = model.netWorth.last {
					Hero(amount: last.netWorth, caption: "Net worth at the end of \(ReportPeriod.monthYear(last.month))", format: model.format)
				}
				Card {
					Chart(model.netWorth, id: \.month) { point in
						LineMark(x: .value("Month", ReportPeriod.shortMonth(point.month)), y: .value("Net worth", Double(point.netWorth) / 100))
							.foregroundStyle(Color.moneyIncomeFill)
							.lineStyle(StrokeStyle(lineWidth: 2.5))
						PointMark(x: .value("Month", ReportPeriod.shortMonth(point.month)), y: .value("Net worth", Double(point.netWorth) / 100))
							.foregroundStyle(Color.moneyIncomeFill)
					}
					.chartYAxis { thousandsAxis }
					.frame(height: 220)
				}
				RowsTable(headings: model.headings, rows: model.netWorthRows, format: model.format, id: "standard.netWorth")
			case .cashFlow:
				RowsTable(headings: model.cashFlowHeadings, rows: model.cashFlowRows, format: model.format, id: "standard.cashFlow")
			case .spendingByCategory:
				Hero(amount: model.spendingTotal, caption: "spent, \(model.period.title)", format: model.format)
				SpendingList(model: model, open: open)
			case .incomeVsExpense:
				Card {
					Chart(model.netProfit?.months ?? [], id: \.first) { column in
						let month = column.month.map(ReportPeriod.shortMonth) ?? ""
						BarMark(x: .value("Month", month), y: .value("Amount", Double(column.income.total) / 100), width: .fixed(22))
							.position(by: .value("Series", "Income"))
							.foregroundStyle(Color.moneyIncomeFill)
						BarMark(x: .value("Month", month), y: .value("Amount", Double(column.expenses.total) / 100), width: .fixed(22))
							.position(by: .value("Series", "Expenses"))
							.foregroundStyle(Color.chartExpense)
					}
					.chartYAxis { thousandsAxis }
					.frame(height: 220)
				}
				RowsTable(headings: model.headings + [model.rangeHeading], rows: model.incomeVsExpenseRows, format: model.format, id: "standard.incomeVsExpense")
			}
		}
		.containerIdentifier("report.\(model.kind.rawValue)")
	}

	private var thousandsAxis: some AxisContent {
		AxisMarks(position: .leading) { value in
			AxisGridLine().foregroundStyle(Color.chartGrid)
			AxisValueLabel {
				if let amount = value.as(Double.self) {
					Text("\(Int(amount / 1000))k").font(.figures(.caption)).foregroundStyle(.textTertiary)
				}
			}
		}
	}
}

struct Hero: View {
	let amount: Int64
	let caption: String
	let format: MoneyFormat

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: Spacing.s3) {
			ReportAmount(amount: amount, tone: .neutral, format: format, weight: .bold, size: .display)
			Text(caption)
				.font(.text(.heading, weight: .semibold))
				.foregroundStyle(.textSecondary)
		}
	}
}

struct Card<Content: View>: View {
	@ViewBuilder let content: Content

	var body: some View {
		content
			.padding(Spacing.s4)
			.frame(maxWidth: .infinity, alignment: .leading)
			.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
			.overlay { RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped) }
	}
}

/// Labeled rows of figures under month headings, totals ruled off.
struct RowsTable: View {
	let headings: [String]
	let rows: [ReportRow]
	let format: MoneyFormat
	let id: String

	var body: some View {
		VStack(spacing: 0) { grid }
			.containerIdentifier(id)
	}

	private var grid: some View {
		Grid(alignment: .trailing, horizontalSpacing: 0, verticalSpacing: 0) {
			GridRow {
				Text("").gridColumnAlignment(.leading)
				ForEach(Array(headings.enumerated()), id: \.offset) { _, heading in
					Text(heading)
						.font(.text(.caption))
						.foregroundStyle(.textTertiary)
						.frame(minWidth: 96, alignment: .trailing)
						.padding(.horizontal, Spacing.s4)
				}
			}
			.frame(height: 30)
			ForEach(rows) { row in
				Divider().overlay(row.isTotal ? Color.textTertiary : Color.separator)
				GridRow {
					Text(row.title)
						.font(.text(.body, weight: row.isTotal ? .bold : .medium))
						.lineLimit(1)
						.fixedSize()
						.padding(.leading, Spacing.s4)
						.padding(.trailing, Spacing.s3)
						.frame(maxWidth: .infinity, alignment: .leading)
					ForEach(Array(row.values.enumerated()), id: \.offset) { index, value in
						ReportAmount(amount: value, tone: row.tones[index], format: format, weight: row.isTotal ? .bold : .medium)
							.frame(minWidth: 96, alignment: .trailing)
							.padding(.horizontal, Spacing.s4)
					}
				}
				.frame(minHeight: 38)
				.accessibilityIdentifier("\(id).\(row.id)")
			}
		}
		.padding(.vertical, Spacing.s1)
		.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
		.overlay { RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped) }
	}
}

private struct SpendingList: View {
	let model: StandardReportModel
	let open: (DrilldownModel) -> Void

	var body: some View {
		let largest = max(model.spending.map(\.amount).max() ?? 1, 1)
		GroupedRows {
			ForEach(model.spending) { line in
				Button {
					if line.count > 0 { open(model.drilldown(line)) }
				} label: {
					HStack(spacing: Spacing.s3) {
						Text(line.title)
							.frame(width: 220, alignment: .leading)
						GeometryReader { proxy in
							Capsule()
								.fill(Color.chartExpense)
								.frame(width: max(proxy.size.width * CGFloat(max(line.amount, 0)) / CGFloat(largest), 2), height: Size.bar)
								.frame(maxHeight: .infinity, alignment: .center)
						}
						ReportAmount(amount: line.amount, format: model.format)
							.frame(width: 110, alignment: .trailing)
					}
					.padding(.horizontal, Spacing.s4)
					.frame(height: Size.row)
					.contentShape(.rect)
				}
				.buttonStyle(.plain)
				.help(ReportWords.showTransactions(line.count) ?? "")
				.accessibilityLabel("\(line.title), \(model.format.amount(line.amount))")
				.accessibilityIdentifier("spending.\(line.id)")
				Divider().padding(.leading, Spacing.s4)
			}
			HStack {
				Text("Total").font(.text(.body, weight: .bold))
				Spacer()
				ReportAmount(amount: model.spendingTotal, format: model.format, weight: .bold)
			}
			.padding(.horizontal, Spacing.s4)
			.frame(height: Size.row)
			.accessibilityElement(children: .combine)
			.accessibilityIdentifier("spending.total")
		}
	}
}
