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
					Hero(amount: last.netWorth, caption: "Net worth at the end of \(ReportPeriod.monthYear(last.month))", format: model.format, tone: last.netWorth < 0 ? .overspent : .neutral)
				}
				Card {
					let stop = ZeroSplit.lineStop(model.netWorth.map { model.format.chartValue($0.netWorth, on: .register) })
					Chart(model.netWorth, id: \.month) { point in
						LineMark(x: .value("Month", ReportPeriod.shortMonth(point.month)), y: .value("Net worth", model.format.chartValue(point.netWorth, on: .register)))
							.foregroundStyle(LinearGradient(stops: [.init(color: .chartMark(.moneyIncomeFill), location: 0), .init(color: .chartMark(.moneyIncomeFill), location: stop), .init(color: .moneyOverspent, location: stop), .init(color: .moneyOverspent, location: 1)], startPoint: .top, endPoint: .bottom))
							.lineStyle(StrokeStyle(lineWidth: 2.5))
						PointMark(x: .value("Month", ReportPeriod.shortMonth(point.month)), y: .value("Net worth", model.format.chartValue(point.netWorth, on: .register)))
							.foregroundStyle(point.netWorth < 0 ? Color.moneyOverspent : Color.chartMark(.moneyIncomeFill))
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
						BarMark(x: .value("Month", month), y: .value("Amount", model.format.chartValue(column.income.total, on: .register)), width: .fixed(22))
							.position(by: .value("Series", "Income"))
							.foregroundStyle(Color.chartMark(.moneyIncomeFill))
						BarMark(x: .value("Month", month), y: .value("Amount", model.format.chartValue(column.expenses.total, on: .register)), width: .fixed(22))
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
					Text(model.format.axisLabel(amount)).font(.figures(.caption)).foregroundStyle(.textTertiary)
				}
			}
		}
	}
}

struct Hero: View {
	let amount: Int64
	let caption: String
	let format: MoneyFormat
	var tone: VeritasCore.MoneyTone = .neutral

	var body: some View {
		ViewThatFits(in: .horizontal) {
			HStack(alignment: .firstTextBaseline, spacing: Spacing.s3) { figure; label }
			VStack(alignment: .leading, spacing: Spacing.s1) { figure; label.lineLimit(2) }
		}
	}

	private var figure: some View {
		ReportAmount(amount: amount, tone: tone, format: format, weight: .bold, size: .display)
	}

	private var label: some View {
		Text(caption)
			.font(.text(.heading, weight: .semibold))
			.foregroundStyle(.textSecondary)
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
		PinnedTable(headings: headings, lines: rows.map(line))
			.containerIdentifier(id)
	}

	private func line(_ row: ReportRow) -> TableLine {
		let weight: Font.Weight = row.isTotal ? .bold : .medium
		return TableLine(
			id: row.id,
			height: 38,
			rule: row.isTotal ? .strong : .light,
			leading: AnyView(
				Text(row.title)
					.font(.text(.body, weight: weight))
					.padding(.leading, Spacing.s4)
					.padding(.trailing, Spacing.s3)
					.accessibilityIdentifier("\(id).\(row.id)")
			),
			cells: row.values.indices.map { index in
				AnyView(
					ReportAmount(amount: row.values[index], tone: row.tones[index], format: format, weight: weight)
						.frame(minWidth: 96, alignment: .trailing)
						.padding(.horizontal, Spacing.s4)
						.accessibilityElement(children: .ignore)
						.accessibilityLabel("\(row.title), \(headings.indices.contains(index) ? headings[index] : ""): \(format.amount(row.values[index], on: .register))")
				)
			}
		)
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
							.lineLimit(1)
							.frame(minWidth: 120, idealWidth: 220, maxWidth: 220, alignment: .leading)
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
				.accessibilityLabel("\(line.title), \(model.format.amount(line.amount, on: .register))")
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
