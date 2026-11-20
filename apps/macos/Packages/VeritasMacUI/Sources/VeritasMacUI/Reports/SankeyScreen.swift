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

/// The Sankey drawn calm, with nothing lit. Hovering anywhere on a node or
/// ribbon lights its path and shows its amount; clicking opens its
/// transactions (a ribbon opens the node it carries money for), and
/// Shortfall explains itself instead.
struct SankeyCanvas: View {
	let report: SankeyReport
	let format: MoneyFormat
	var compact = false
	let open: (SankeyNode) -> Void
	@State private var hovered: SankeyTarget?
	@State private var pointer: CGPoint = .zero
	@State private var explaining: Int?
	@FocusState private var focused: Int?

	/// The pointer wins; without it, the node with keyboard focus is lit.
	private var shown: SankeyTarget? { hovered ?? focused.map(SankeyTarget.node) }

	var body: some View {
		GeometryReader { proxy in
			let layout = SankeyLayout(report: report, size: proxy.size)
			let lit = shown.map { SankeyHighlight($0, links: layout.links) }
			ZStack(alignment: .topLeading) {
				Canvas { context, _ in
					for ribbon in layout.ribbons {
						let opacity = lit.map { $0.flows.contains(ribbon.flow) ? 0.55 : 0.12 } ?? 0.28
						context.fill(Path(ribbon.outline), with: .color(color(report.nodes[ribbon.to]).opacity(opacity)))
					}
					for node in layout.nodes {
						let shape = Path(roundedRect: node.rect, cornerRadius: 3)
						let isLit = lit?.nodes.contains(node.index) ?? false
						let dimmed = lit != nil && !isLit
						context.fill(shape, with: .color(isLit && shown == .node(node.index) ? .textPrimary : color(report.nodes[node.index]).opacity(dimmed ? 0.45 : 1)))
					}
				}
				ForEach(layout.nodes, id: \.index) { placed in
					let node = report.nodes[placed.index]
					if !compact || placed.rect.height >= 9 || node.column < 3 {
						label(node, placed)
					}
					nodeElement(node, placed)
				}
				ForEach(layout.ribbons, id: \.flow) { ribbon in
					flowElement(ribbon)
				}
				if let shown, let readout = report.readout(shown) {
					let anchor = hovered == nil ? focusAnchor(layout) : pointer
					SankeyReadoutView(readout: readout, format: format)
						.position(x: min(max(anchor.x, 120), proxy.size.width - 120), y: max(anchor.y - 36, 20))
						.allowsHitTesting(false)
				}
				if let explaining, layout.nodes.indices.contains(explaining) {
					let rect = layout.nodes[explaining].rect
					Color.clear
						.frame(width: rect.width, height: rect.height)
						.position(x: rect.midX, y: rect.midY)
						.popover(isPresented: Binding(get: { self.explaining != nil }, set: { if !$0 { self.explaining = nil } })) { explanation(report.nodes[explaining]) }
						.allowsHitTesting(false)
				}
			}
			.contentShape(.rect)
			.onContinuousHover { phase in
				switch phase {
				case .active(let location):
					pointer = location
					hovered = layout.target(at: location)
				case .ended:
					hovered = nil
				}
			}
			.onTapGesture(coordinateSpace: .local) { location in
				if let target = layout.target(at: location) { activate(target) }
			}
		}
		.accessibilityElement(children: .contain)
		.accessibilityLabel("Where the income went")
		.accessibilityIdentifier("sankey.canvas")
	}

	private func activate(_ target: SankeyTarget) {
		guard let node = report.drillNode(target) else { return }
		if node.key.kind == .shortfall {
			explaining = report.nodes.firstIndex(of: node)
		} else if ReportWords.drillAction(for: node) != nil {
			open(node)
		}
	}

	private func focusAnchor(_ layout: SankeyLayout) -> CGPoint {
		guard let focused, let node = layout.nodes.first(where: { $0.index == focused }) else { return .zero }
		return CGPoint(x: node.rect.midX, y: node.rect.minY)
	}

	/// The node's button: Tab reaches it with keyboard navigation on, and
	/// Space or a click opens it. Hover stays with the canvas underneath.
	private func nodeElement(_ node: SankeyNode, _ placed: SankeyLayout.Node) -> some View {
		let height = max(placed.rect.height, 12)
		return Button {
			activate(.node(placed.index))
		} label: {
			Color.clear
				.frame(width: placed.rect.width + 10, height: height)
				.contentShape(.rect)
		}
		.buttonStyle(.plain)
		.focused($focused, equals: placed.index)
		.position(x: placed.rect.midX, y: placed.rect.midY)
		.help(ReportWords.drillAction(for: node) ?? ReportWords.shortfallCaption)
		.accessibilityLabel("\(ReportWords.node(node)), \(format.amount(node.value, on: .register))")
		.accessibilityHint(ReportWords.drillAction(for: node) ?? ReportWords.shortfallCaption)
		.accessibilityIdentifier("sankey.node.\(Self.identifier(node.key))")
	}

	/// A handle in the middle of a ribbon, where its two curves are furthest from the nodes.
	private func flowElement(_ ribbon: SankeyLayout.Ribbon) -> some View {
		let thickness = max(ribbon.source.upperBound - ribbon.source.lowerBound, 4)
		let middle = CGPoint(x: (ribbon.sourceX + ribbon.targetX) / 2, y: (ribbon.source.lowerBound + ribbon.source.upperBound + ribbon.target.lowerBound + ribbon.target.upperBound) / 4)
		let from = report.nodes[ribbon.from], to = report.nodes[ribbon.to]
		return Color.clear
			.frame(width: 12, height: thickness)
			.position(middle)
			.allowsHitTesting(false)
			.accessibilityElement()
			.accessibilityAddTraits(.isButton)
			.accessibilityLabel("\(ReportWords.node(from)) → \(ReportWords.node(to)), \(format.amount(report.flows[ribbon.flow].value, on: .register))")
			.accessibilityAction { activate(.flow(ribbon.flow)) }
			.accessibilityIdentifier("sankey.flow.\(Self.identifier(from.key)).\(Self.identifier(to.key))")
	}

	private func explanation(_ node: SankeyNode) -> some View {
		VStack(alignment: .leading, spacing: 4) {
			Text("Shortfall \(format.amount(node.value, on: .register))").font(.text(.body, weight: .semibold))
			Text(ReportWords.shortfallCaption).foregroundStyle(.textSecondary)
		}
		.padding(Spacing.s3)
	}

	@ViewBuilder private func label(_ node: SankeyNode, _ placed: SankeyLayout.Node) -> some View {
		let name = ReportWords.node(node)
		let value = format.amount(node.value, on: .register)
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
			.accessibilityHidden(true)
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
			.accessibilityHidden(true)
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

	static func identifier(_ key: SankeyNodeKey) -> String {
		"\(key.kind)" + (key.id.map { ".\($0)" } ?? "")
	}
}
