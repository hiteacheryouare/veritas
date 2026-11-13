import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Where the income went (R-REP-6, ST-8, design/mockups/sankey.html). Each
/// node opens its transactions; Shortfall explains itself instead.
struct SankeyScreen: View {
	@Bindable var workspace: WorkspaceModel
	@State private var model: SankeyModel
	@State private var drilldown: DrilldownModel?
	@State private var isChoosingRange = false
	@FocusState private var hasKeyFocus: Bool

	init(workspace: WorkspaceModel, model: SankeyModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? SankeyModel(workspace: workspace))
	}

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: Spacing.s3) {
				if let report = model.report {
					if report.nodes.isEmpty {
						Text("No income or spending in \(model.title)")
							.foregroundStyle(.textSecondary)
							.frame(maxWidth: .infinity, minHeight: 240)
					} else {
						SankeyCanvas(report: report, format: model.format) { node in
							drilldown = model.drilldown(node)
						}
						.frame(height: 600)
						.padding(.vertical, Spacing.s2)
						.padding(.horizontal, Spacing.s4)
						.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
						.overlay { RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped) }
					}
				} else {
					ProgressView().frame(maxWidth: .infinity, minHeight: 240)
				}
			}
			.padding(.top, Spacing.s2)
			.padding(.horizontal, Spacing.s5)
			.padding(.bottom, Spacing.s6)
		}
		.background(.surfaceWindow)
		.focusable()
		.focusEffectDisabled()
		.focused($hasKeyFocus)
		.defaultFocus($hasKeyFocus, true)
		.focusedSceneValue(\.budgetMonths, BudgetMonthActions(
			canGoBack: true,
			canGoForward: model.canGoForward,
			previous: { Task { await model.previous() } },
			next: { Task { await model.next() } }
		))
		.navigationTitle(model.title)
		.toolbar {
			ToolbarItem(placement: .navigation) {
				PeriodStepper(canGoForward: model.canGoForward, previous: { Task { await model.previous() } }, next: { Task { await model.next() } }, id: "sankey")
			}
			ToolbarItem(placement: .primaryAction) {
				SpanPicker(selection: model.span, choices: SankeySpan.allCases, title: \.title, id: "sankey.range") { span in
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
		.containerIdentifier("screen.sankey")
	}
}

struct SankeyCanvas: View {
	let report: SankeyReport
	let format: MoneyFormat
	var compact = false
	let open: (SankeyNode) -> Void
	@State private var hovered: Int?

	var body: some View {
		GeometryReader { proxy in
			let layout = SankeyLayout(report: report, size: proxy.size)
			ZStack(alignment: .topLeading) {
				Canvas { context, _ in
					for ribbon in layout.ribbons {
						let touches = hovered == ribbon.from || hovered == ribbon.to
						context.fill(Self.path(ribbon), with: .color(color(report.nodes[ribbon.to]).opacity(touches ? 0.55 : 0.28)))
					}
					for node in layout.nodes {
						let shape = Path(roundedRect: node.rect, cornerRadius: 3)
						context.fill(shape, with: .color(hovered == node.index ? .textPrimary : color(report.nodes[node.index])))
					}
				}
				ForEach(layout.nodes, id: \.index) { placed in
					let node = report.nodes[placed.index]
					if !compact || placed.rect.height >= 9 || node.column < 3 {
						label(node, placed)
					}
					SankeyNodeButton(node: node, rect: placed.rect, format: format, open: open) { isHovered in
						hovered = isHovered ? placed.index : (hovered == placed.index ? nil : hovered)
					}
				}
			}
		}
		.accessibilityElement(children: .contain)
		.accessibilityLabel("Where the income went")
		.accessibilityIdentifier("sankey.canvas")
	}

	@ViewBuilder private func label(_ node: SankeyNode, _ placed: SankeyLayout.Node) -> some View {
		let name = ReportWords.node(node)
		let value = format.amount(node.value)
		let small = compact || node.column == 3
		let width: CGFloat = 200
		switch placed.labelSide {
		case .above:
			VStack(spacing: 0) {
				Text(name).font(.text(.body, weight: .semibold))
				Text(value).font(.figures(.caption)).foregroundStyle(.textSecondary)
			}
			.frame(width: width)
			.position(x: placed.rect.midX, y: placed.rect.minY - 16)
			.allowsHitTesting(false)
		case .leading, .trailing:
			let leading = placed.labelSide == .leading
			Group {
				if small {
					HStack(spacing: 6) {
						Text(name).font(.text(.caption, weight: .medium))
						Text(value).font(.figures(.caption)).foregroundStyle(.textSecondary)
					}
				} else {
					VStack(alignment: leading ? .trailing : .leading, spacing: 0) {
						Text(name).font(.text(.body, weight: .medium))
						Text(value).font(.figures(.caption)).foregroundStyle(.textSecondary)
					}
				}
			}
			.lineLimit(1)
			.frame(width: width, alignment: leading ? .trailing : .leading)
			.position(x: leading ? placed.rect.minX - 10 - width / 2 : placed.rect.maxX + 10 + width / 2, y: placed.rect.midY)
			.allowsHitTesting(false)
		}
	}

	func color(_ node: SankeyNode) -> Color {
		switch node.key.kind {
		case .incomeCategory, .income, .refundsAndWithdrawals, .unreviewedTransfersIn, .shortfall: .moneyIncomeFill
		case .savings: .chartSavings
		case .debtPaydown: .chartDebtPaydown
		case .unallocated: .chartUnallocated
		default: .chartExpense
		}
	}

	static func path(_ ribbon: SankeyLayout.Ribbon) -> Path {
		var path = Path()
		let middle = (ribbon.sourceX + ribbon.targetX) / 2
		path.move(to: CGPoint(x: ribbon.sourceX, y: ribbon.source.lowerBound))
		path.addCurve(to: CGPoint(x: ribbon.targetX, y: ribbon.target.lowerBound), control1: CGPoint(x: middle, y: ribbon.source.lowerBound), control2: CGPoint(x: middle, y: ribbon.target.lowerBound))
		path.addLine(to: CGPoint(x: ribbon.targetX, y: ribbon.target.upperBound))
		path.addCurve(to: CGPoint(x: ribbon.sourceX, y: ribbon.source.upperBound), control1: CGPoint(x: middle, y: ribbon.target.upperBound), control2: CGPoint(x: middle, y: ribbon.source.upperBound))
		path.closeSubpath()
		return path
	}

	static func identifier(_ key: SankeyNodeKey) -> String {
		"\(key.kind)" + (key.id.map { ".\($0)" } ?? "")
	}
}

/// A node's click target, a little larger than the node so thin ones can be hit.
private struct SankeyNodeButton: View {
	let node: SankeyNode
	let rect: CGRect
	let format: MoneyFormat
	let open: (SankeyNode) -> Void
	let hover: (Bool) -> Void
	@State private var isExplaining = false

	var body: some View {
		let height = max(rect.height, 12)
		Button(action: activate) {
			Color.clear
				.frame(width: rect.width + 10, height: height)
				.contentShape(.rect)
		}
		.buttonStyle(.plain)
		.offset(x: rect.minX - 5, y: rect.minY - (height - rect.height) / 2)
		.onHover(perform: hover)
		.help(ReportWords.drillAction(for: node) ?? ReportWords.shortfallCaption)
		.popover(isPresented: $isExplaining) { explanation }
		.accessibilityLabel("\(ReportWords.node(node)), \(format.amount(node.value))")
		.accessibilityHint(ReportWords.drillAction(for: node) ?? ReportWords.shortfallCaption)
		.accessibilityIdentifier("sankey.node.\(SankeyCanvas.identifier(node.key))")
	}

	private var explanation: some View {
		VStack(alignment: .leading, spacing: 4) {
			Text("Shortfall \(format.amount(node.value))").font(.text(.body, weight: .semibold))
			Text(ReportWords.shortfallCaption).foregroundStyle(.textSecondary)
		}
		.padding(Spacing.s3)
	}

	private func activate() {
		if node.key.kind == .shortfall {
			isExplaining = true
		} else if ReportWords.drillAction(for: node) != nil {
			open(node)
		}
	}
}
