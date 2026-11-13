import Charts
import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Actual's dashboards on their 12-column grid (R-REP-4). Known widgets draw
/// from Veritas reports; the rest say "Open in Actual" and are kept as they are.
struct DashboardScreen: View {
	@Bindable var workspace: WorkspaceModel
	@State private var model: DashboardModel
	static let rowHeight: CGFloat = 92
	static let gap: CGFloat = Spacing.s3

	init(workspace: WorkspaceModel, model: DashboardModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? DashboardModel(workspace: workspace))
	}

	var body: some View {
		ScrollView {
			Group {
				if let page = model.page {
					if page.widgets.isEmpty {
						Text("No widgets on \(page.name)")
							.foregroundStyle(.textSecondary)
							.frame(maxWidth: .infinity, minHeight: 200)
					} else {
						DashboardGrid(page: page, model: model)
					}
				} else if model.pages.isEmpty {
					Text("No dashboards. Add one in Actual and it shows here.")
						.foregroundStyle(.textSecondary)
						.frame(maxWidth: .infinity, minHeight: 200)
				}
			}
			.padding(.top, Spacing.s2)
			.padding(.horizontal, Spacing.s5)
			.padding(.bottom, Spacing.s6)
		}
		.background(.surfaceWindow)
		.toolbar {
			if model.pages.count > 1 {
				ToolbarItem(placement: .navigation) {
					Picker("Dashboard", selection: Binding(get: { model.page?.id ?? "" }, set: { model.pageID = $0 })) {
						ForEach(model.pages) { Text($0.name).tag($0.id) }
					}
					.pickerStyle(.segmented)
					.labelsHidden()
					.accessibilityIdentifier("dashboard.pages")
				}
			}
			ToolbarItem(placement: .primaryAction) {
				Toggle(isOn: $model.isEditing) {
					Label(model.isEditing ? "Done" : "Edit layout", systemImage: model.isEditing ? "checkmark" : "rectangle.3.group")
				}
				.toggleStyle(.button)
				.labelStyle(.titleAndIcon)
				.disabled(model.isReadOnly || model.page == nil)
				.accessibilityIdentifier("dashboard.edit")
			}
		}
		.task(id: workspace.changeCount) { await model.load() }
		.containerIdentifier("screen.dashboard")
	}
}

private struct DashboardGrid: View {
	let page: DashboardPage
	let model: DashboardModel

	var body: some View {
		GeometryReader { proxy in
			let column = (proxy.size.width - DashboardScreen.gap * 11) / 12
			ZStack(alignment: .topLeading) {
				ForEach(page.widgets) { widget in
					WidgetCard(widget: widget, model: model, column: column)
						.frame(width: span(widget.width, column), height: height(widget.height))
						.offset(x: CGFloat(widget.x) * (column + DashboardScreen.gap), y: CGFloat(widget.y) * (DashboardScreen.rowHeight + DashboardScreen.gap))
				}
			}
		}
		.frame(height: height(page.widgets.map { $0.y + $0.height }.max() ?? 0))
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier("dashboard.grid")
	}

	private func span(_ columns: Int64, _ width: CGFloat) -> CGFloat {
		CGFloat(columns) * width + CGFloat(max(columns - 1, 0)) * DashboardScreen.gap
	}

	private func height(_ rows: Int64) -> CGFloat {
		CGFloat(rows) * DashboardScreen.rowHeight + CGFloat(max(rows - 1, 0)) * DashboardScreen.gap
	}
}

private struct WidgetCard: View {
	let widget: DashboardWidget
	let model: DashboardModel
	let column: CGFloat
	@State private var drag: CGSize = .zero

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			if let title = WidgetWords.title(widget) {
				Text(title)
					.font(.text(.body, weight: .semibold))
					.lineLimit(1)
			}
			WidgetBody(widget: widget, format: model.format)
				.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
		}
		.padding(Spacing.s3)
		.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
		.overlay {
			RoundedRectangle(cornerRadius: Radius.section)
				.strokeBorder(model.isEditing ? Color.accent : Color.edgeGrouped, style: StrokeStyle(lineWidth: 1, dash: model.isEditing ? [5, 3] : []))
		}
		.overlay(alignment: .bottomTrailing) {
			if model.isEditing { controls.padding(Spacing.s2) }
		}
		.offset(drag)
		.gesture(model.isEditing ? moveGesture : nil)
		.accessibilityElement(children: .contain)
		.accessibilityLabel(WidgetWords.title(widget) ?? widget.kind)
		.accessibilityIdentifier("widget.\(widget.id)")
	}

	private var moveGesture: some Gesture {
		DragGesture()
			.onChanged { drag = $0.translation }
			.onEnded { value in
				let columns = Int64((value.translation.width / (column + DashboardScreen.gap)).rounded())
				let rows = Int64((value.translation.height / (DashboardScreen.rowHeight + DashboardScreen.gap)).rounded())
				drag = .zero
				Task { await model.move(widget, columns: columns, rows: rows) }
			}
	}

	private var controls: some View {
		HStack(spacing: 2) {
			control("Move left", "arrow.left") { await model.move(widget, columns: -1) }
			control("Move right", "arrow.right") { await model.move(widget, columns: 1) }
			control("Move up", "arrow.up") { await model.move(widget, rows: -1) }
			control("Move down", "arrow.down") { await model.move(widget, rows: 1) }
			Divider().frame(height: 14)
			control("Narrower", "arrow.right.and.line.vertical.and.arrow.left") { await model.resize(widget, columns: -1) }
			control("Wider", "arrow.left.and.line.vertical.and.arrow.right") { await model.resize(widget, columns: 1) }
			control("Shorter", "arrow.down.and.line.horizontal.and.arrow.up") { await model.resize(widget, rows: -1) }
			control("Taller", "arrow.up.and.line.horizontal.and.arrow.down") { await model.resize(widget, rows: 1) }
		}
		.padding(3)
		.background(.surfaceRaised, in: .rect(cornerRadius: Radius.control))
		.shadow(radius: 2)
	}

	private func control(_ title: String, _ symbol: String, action: @escaping () async -> Void) -> some View {
		Button {
			Task { await action() }
		} label: {
			Image(systemName: symbol).frame(width: 18, height: 18)
		}
		.buttonStyle(.borderless)
		.help(title)
		.accessibilityLabel(title)
		.accessibilityIdentifier("widget.\(widget.id).\(title.lowercased().replacingOccurrences(of: " ", with: "-"))")
	}
}

enum WidgetWords {
	static func title(_ widget: DashboardWidget) -> String? {
		if let name = widget.name, !name.isEmpty { return name }
		switch widget.kind {
		case "net-worth-card": return "Net worth"
		case "cash-flow-card": return "Cash flow"
		case "spending-card": return "Spending"
		case "sankey-card": return "Sankey"
		case "markdown-card": return nil
		default: return nil
		}
	}

	static func placeholder(_ reason: PlaceholderReason, kind: String) -> String {
		switch reason {
		case .unknownType: "Veritas doesn't know this widget (\(kind)). It's kept as it is."
		case .notNative: "Veritas doesn't draw this widget yet. It's kept as it is."
		case .filtered: "This widget filters its transactions, which Veritas doesn't do yet. It's kept as it is."
		case .missingReport: "The custom report this widget showed was deleted."
		}
	}
}

private struct WidgetBody: View {
	let widget: DashboardWidget
	let format: MoneyFormat

	var body: some View {
		switch widget.view {
		case .netWorth(let points):
			VStack(alignment: .leading, spacing: Spacing.s1) {
				if let last = points.last {
					ReportAmount(amount: last.netWorth, format: format, weight: .bold, size: .title)
					if let first = points.first, points.count > 1 {
						Text("\(format.amount(last.netWorth - first.netWorth)) since \(ReportPeriod.monthName(first.month))")
							.font(.text(.caption))
							.foregroundStyle(.textSecondary)
					}
				}
				Chart(points, id: \.month) { point in
					AreaMark(x: .value("Month", ReportPeriod.shortMonth(point.month)), y: .value("Net worth", Double(point.netWorth) / 100))
						.foregroundStyle(Color.moneyIncomeFill.opacity(0.18))
					LineMark(x: .value("Month", ReportPeriod.shortMonth(point.month)), y: .value("Net worth", Double(point.netWorth) / 100))
						.foregroundStyle(Color.moneyIncomeFill)
				}
				.chartYAxis(.hidden)
				.chartYScale(domain: .automatic(includesZero: false))
			}
		case .cashFlow(let first, let last, let income, let expenses, let netProfit):
			VStack(alignment: .leading, spacing: Spacing.s2) {
				Text(ReportPeriod(first: first, last: last).title).font(.text(.caption)).foregroundStyle(.textSecondary)
				figure("Income", income, tone: .income)
				figure("Expenses", expenses)
				figure("Net Profit", netProfit, tone: netProfit < 0 ? .overspent : .neutral, bold: true)
			}
		case .spending(let month, let spent, let compareMonth, let compareSpent):
			VStack(alignment: .leading, spacing: Spacing.s1) {
				ReportAmount(amount: spent, format: format, weight: .bold, size: .title)
				Text("spent in \(ReportPeriod.monthName(month))").font(.text(.caption)).foregroundStyle(.textSecondary)
				Text("\(format.amount(compareSpent)) in \(ReportPeriod.monthName(compareMonth))").font(.text(.caption)).foregroundStyle(.textSecondary)
			}
		case .sankey(let report):
			SankeyCanvas(report: report, format: format, compact: true) { _ in }
		case .customReport(let report):
			CustomReportDetail(view: report, format: format, compact: true)
		case .markdown(let text, _):
			ScrollView {
				MarkdownText(text: text)
			}
		case .openInActual(let reason):
			OpenInActual(message: WidgetWords.placeholder(reason, kind: widget.kind))
		}
	}

	private func figure(_ title: String, _ amount: Int64, tone: VeritasCore.MoneyTone = .neutral, bold: Bool = false) -> some View {
		HStack {
			Text(title).font(.text(.body, weight: bold ? .semibold : .regular))
			Spacer()
			ReportAmount(amount: amount, tone: tone, format: format, weight: bold ? .bold : .medium)
		}
	}
}

/// A markdown card: headings and paragraphs, with inline emphasis and links.
struct MarkdownText: View {
	let text: String

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			ForEach(Array(Self.blocks(text).enumerated()), id: \.offset) { _, block in
				if block.hasPrefix("#") {
					Text(Self.inline(block.drop { $0 == "#" }.trimmingCharacters(in: .whitespaces)))
						.font(.text(.heading, weight: .semibold))
				} else {
					Text(Self.inline(block))
				}
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
	}

	static func blocks(_ text: String) -> [String] {
		text.components(separatedBy: "\n\n").flatMap { paragraph in
			paragraph.hasPrefix("#") ? paragraph.components(separatedBy: "\n") : [paragraph]
		}
		.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
		.filter { !$0.isEmpty }
	}

	static func inline(_ text: String) -> AttributedString {
		(try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(text)
	}
}
