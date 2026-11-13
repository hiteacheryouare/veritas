import Charts
import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Net Profit (R-REP-2, design/mockups/net-profit.html): the range's Net
/// Profit, income against expenses by month, and the ST-1 table. Every number
/// opens its transactions.
struct NetProfitScreen: View {
	@Bindable var workspace: WorkspaceModel
	@State private var model: NetProfitModel
	@State private var drilldown: DrilldownModel?
	@State private var isChoosingRange = false
	@FocusState private var hasKeyFocus: Bool

	init(workspace: WorkspaceModel, model: NetProfitModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? NetProfitModel(workspace: workspace))
	}

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: Spacing.s4) {
				if let report = model.report {
					HStack(alignment: .firstTextBaseline, spacing: Spacing.s3) {
						ReportAmount(amount: report.total.netProfit.total, tone: report.total.netProfit.total < 0 ? .overspent : .neutral, format: model.format, weight: .bold, size: .display)
							.accessibilityIdentifier("netProfit.hero")
						Text(model.period.title)
							.font(.text(.heading, weight: .semibold))
							.foregroundStyle(.textSecondary)
					}
					NetProfitChart(model: model)
						.padding(Spacing.s4)
						.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
						.overlay { RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped) }
					NetProfitTable(model: model) { cell in
						drilldown = DrilldownModel(workspace: workspace, title: cell.title, drill: cell.drill)
					}
				} else {
					ProgressView().frame(maxWidth: .infinity, minHeight: 200)
				}
			}
			.padding(.top, Spacing.s2)
			.padding(.horizontal, Spacing.s5)
			.padding(.bottom, Spacing.s6)
			.frame(maxWidth: .infinity, alignment: .leading)
		}
		.background(.surfaceWindow)
		.focusable()
		.focusEffectDisabled()
		.focused($hasKeyFocus)
		.defaultFocus($hasKeyFocus, true)
		.focusedValue(\.budgetMonths, BudgetMonthActions(
			canGoBack: true,
			canGoForward: model.canGoForward,
			previous: { Task { await model.previous() } },
			next: { Task { await model.next() } }
		))
		.toolbar {
			ToolbarItem(placement: .navigation) {
				PeriodStepper(canGoForward: model.canGoForward, previous: { Task { await model.previous() } }, next: { Task { await model.next() } }, id: "netProfit")
			}
			ToolbarItem(placement: .primaryAction) {
				SpanPicker(selection: model.span, choices: MonthSpan.allCases, title: \.title, id: "netProfit.range") { span in
					if span == .custom {
						isChoosingRange = true
					} else {
						Task { await model.choose(span) }
					}
				}
				.popover(isPresented: $isChoosingRange, arrowEdge: .bottom) {
					MonthRangePicker(period: model.period, thisMonth: model.thisMonth) { first, last in
						Task { await model.setCustom(first: first, last: last) }
					}
				}
			}
		}
		.sheet(item: $drilldown) { DrilldownSheet(model: $0) }
		.task(id: workspace.changeCount) { await model.load() }
		.containerIdentifier("screen.netProfit")
	}
}

/// A segmented range control whose Custom choice opens a picker each time.
struct SpanPicker<Span: Hashable>: View {
	let selection: Span
	let choices: [Span]
	let title: KeyPath<Span, String>
	let id: String
	let choose: (Span) -> Void

	var body: some View {
		Picker("Range", selection: Binding(get: { selection }, set: choose)) {
			ForEach(choices, id: \.self) { Text($0[keyPath: title]).tag($0) }
		}
		.pickerStyle(.segmented)
		.labelsHidden()
		.fixedSize()
		.accessibilityIdentifier(id)
	}
}

private struct NetProfitChart: View {
	let model: NetProfitModel

	private struct Bar: Identifiable {
		let id: String
		let month: String
		let series: String
		let amount: Int64
	}

	private var months: [NetProfitColumn] { model.report?.months ?? [] }

	private var bars: [Bar] {
		months.flatMap { column -> [Bar] in
			let month = model.heading(column)
			return [
				Bar(id: "\(month).income", month: month, series: "Income", amount: column.income.total),
				Bar(id: "\(month).expenses", month: month, series: "Expenses", amount: column.expenses.total)
			]
		}
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			HStack(spacing: Spacing.s4) {
				legend("Income", .moneyIncomeFill)
				legend("Expenses", .chartExpense)
			}
			.font(.text(.caption))
			.foregroundStyle(.textSecondary)
			Chart(bars) { bar in
				BarMark(
					x: .value("Month", bar.month),
					y: .value("Amount", Double(bar.amount) / 100),
					width: .fixed(26)
				)
				.position(by: .value("Series", bar.series))
				.foregroundStyle(bar.series == "Income" ? Color.moneyIncomeFill : Color.chartExpense)
				.clipShape(.rect(cornerRadius: 4))
			}
			.chartLegend(.hidden)
			.chartYAxis {
				AxisMarks(position: .leading) { value in
					AxisGridLine().foregroundStyle(Color.chartGrid)
					AxisValueLabel {
						if let amount = value.as(Double.self) {
							Text("\(Int(amount / 1000))k").font(.figures(.caption)).foregroundStyle(.textTertiary)
						}
					}
				}
			}
			.chartXAxis {
				AxisMarks { value in
					AxisValueLabel {
						if let month = value.as(String.self) {
							VStack(spacing: 2) {
								Text(month).foregroundStyle(.textSecondary)
								if let column = months.first(where: { model.heading($0) == month }) {
									Text(Self.net(column.netProfit.total, format: model.format))
										.font(.figures(.caption, weight: .semibold))
										.foregroundStyle(column.netProfit.total < 0 ? Color.moneyOverspent : Color.textSecondary)
								}
							}
						}
					}
				}
			}
			.frame(height: 230)
			.accessibilityLabel("Income and expenses by month, \(model.period.title)")
		}
	}

	private func legend(_ title: String, _ color: Color) -> some View {
		HStack(spacing: 6) {
			RoundedRectangle(cornerRadius: 3).fill(color).frame(width: 12, height: 10)
			Text(title)
		}
	}

	/// The mockup's chart shorthand: whole units with a sign, "+1,024", "−414".
	static func net(_ amount: Int64, format: MoneyFormat) -> String {
		let whole = format.wholeCurrency(amount).replacingOccurrences(of: format.currencySymbol, with: "")
		return amount < 0 ? whole : "+\(whole)"
	}
}

private struct NetProfitTable: View {
	let model: NetProfitModel
	let open: (NetProfitCell) -> Void

	var body: some View {
		let columns = model.columns
		Grid(alignment: .trailing, horizontalSpacing: 0, verticalSpacing: 0) {
			GridRow {
				Text("").gridColumnAlignment(.leading)
				ForEach(Array(columns.enumerated()), id: \.offset) { _, column in
					Text(model.heading(column))
						.font(.text(.caption))
						.foregroundStyle(.textTertiary)
						.frame(minWidth: 96, alignment: .trailing)
						.padding(.horizontal, Spacing.s4)
				}
			}
			.frame(height: 30)
			ForEach(model.rows) { row in
				Divider()
					.overlay(row.style == .total ? Color.textTertiary : Color.separator)
				GridRow {
					title(row)
						.frame(maxWidth: .infinity, alignment: .leading)
					ForEach(Array(row.cells.enumerated()), id: \.offset) { index, cell in
						CellButton(cell: cell, format: model.format, isTotal: row.style == .total, isRange: index == row.cells.count - 1) {
							open(cell)
						}
						.accessibilityIdentifier("netProfit.cell.\(row.id).\(index)")
					}
				}
				.frame(minHeight: 38)
				if row.style == .note {
					GridRow {
						Text(ReportWords.unreviewedNote)
							.font(.text(.caption))
							.foregroundStyle(.textSecondary)
							.padding(.leading, 28)
							.padding(.bottom, Spacing.s2)
							.gridCellColumns(columns.count + 1)
							.frame(maxWidth: .infinity, alignment: .leading)
					}
				}
			}
		}
		.padding(.vertical, Spacing.s1)
		.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
		.overlay { RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped) }
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier("netProfit.table")
	}

	@ViewBuilder private func title(_ row: NetProfitRow) -> some View {
		let label = HStack(spacing: 5) {
			if row.canExpand {
				Image(systemName: model.expanded.contains(row.line) ? "chevron.down" : "chevron.right")
					.font(.system(size: 10, weight: .semibold))
					.foregroundStyle(.textTertiary)
					.frame(width: 12)
			}
			if row.line == .income, row.style == .line {
				Image(systemName: "tray.and.arrow.down").imageScale(.small).foregroundStyle(Color.moneyIncome)
			}
			Text(row.title)
				.font(.text(.body, weight: weight(row)))
				.lineLimit(1)
				.fixedSize()
				.foregroundStyle(row.style == .below || row.style == .category || row.style == .note ? Color.textSecondary : (row.line == .income && row.style == .line ? Color.moneyIncome : Color.textPrimary))
		}
		.padding(.leading, row.style == .below || row.style == .note ? 28 : (row.style == .category ? 44 : Spacing.s4))
		if row.canExpand {
			Button { model.toggle(row.line) } label: { label.contentShape(.rect) }
				.buttonStyle(.plain)
				.accessibilityLabel("\(row.title), \(model.expanded.contains(row.line) ? "hide" : "show") categories")
				.accessibilityIdentifier("netProfit.expand.\(row.id)")
		} else {
			label
		}
	}

	private func weight(_ row: NetProfitRow) -> Font.Weight {
		switch row.style {
		case .total: .bold
		case .line: .medium
		default: .regular
		}
	}
}

private struct CellButton: View {
	let cell: NetProfitCell
	let format: MoneyFormat
	let isTotal: Bool
	let isRange: Bool
	let action: () -> Void
	@State private var isHovered = false

	var body: some View {
		Button {
			if cell.count > 0 { action() }
		} label: {
			HStack(spacing: 4) {
				ReportAmount(amount: cell.amount, tone: cell.tone, format: format, weight: isTotal ? .bold : (isRange ? .semibold : .medium))
					.underline(isHovered && cell.count > 0, color: .accent)
				if isHovered, cell.count > 0 {
					Image(systemName: "arrow.up.right.square").imageScale(.small).foregroundStyle(Color.accent)
				}
			}
			.frame(minWidth: 96, maxHeight: .infinity, alignment: .trailing)
			.padding(.horizontal, Spacing.s4)
			.background(isHovered && cell.count > 0 ? Color.accentSoft : .clear)
			.contentShape(.rect)
		}
		.buttonStyle(.plain)
		.onHover { isHovered = $0 }
		.help(ReportWords.showTransactions(cell.count) ?? "")
		.accessibilityLabel("\(cell.title), \(format.amount(cell.amount))")
		.accessibilityHint(ReportWords.showTransactions(cell.count) ?? "")
	}
}
