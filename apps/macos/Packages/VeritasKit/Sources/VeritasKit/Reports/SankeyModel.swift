import CoreGraphics
import Foundation
import Observation
import VeritasCore

/// Where each Sankey node and ribbon goes on a canvas of a given size. The
/// core decides the nodes, values and columns; this only places them.
public struct SankeyLayout: Sendable {
	public enum LabelSide: Sendable {
		case leading
		case trailing
		case above
	}

	public struct Node: Sendable {
		public let index: Int
		public let rect: CGRect
		public let labelSide: LabelSide
	}

	public struct Ribbon: Sendable {
		public let flow: Int
		public let from: Int
		public let to: Int
		public let source: ClosedRange<CGFloat>
		public let sourceX: CGFloat
		public let target: ClosedRange<CGFloat>
		public let targetX: CGFloat
	}

	public static let nodeWidth: CGFloat = 12
	/// The mockup's columns at 150, 360, 600 and 790 of 960 points.
	static let columnX: [CGFloat] = [0.156, 0.375, 0.625, 0.823]
	static let top: CGFloat = 28
	static let bottom: CGFloat = 8

	public let nodes: [Node]
	public let ribbons: [Ribbon]

	public init(report: SankeyReport, size: CGSize) {
		let columns = Dictionary(grouping: report.nodes.indices) { Int(report.nodes[$0].column) }
		let height = max(size.height - Self.top - Self.bottom, 1)
		let gap = { (column: Int) -> CGFloat in column == 3 ? 7 : 12 }
		let scale = columns.map { column, members -> CGFloat in
			let total = members.reduce(0.0) { $0 + Double(report.nodes[$1].value) }
			let room = height - gap(column) * CGFloat(members.count - 1)
			return total > 0 ? max(room, 1) / CGFloat(total) : .greatestFiniteMagnitude
		}.min() ?? 1

		var rects = [CGRect](repeating: .zero, count: report.nodes.count)
		for (column, members) in columns {
			var y = Self.top
			let x = size.width * Self.columnX[min(column, Self.columnX.count - 1)]
			for index in members.sorted() {
				let h = max(CGFloat(report.nodes[index].value) * scale, 2)
				rects[index] = CGRect(x: x, y: y, width: Self.nodeWidth, height: h)
				y += h + gap(column)
			}
		}
		nodes = report.nodes.indices.map { index in
			let node = report.nodes[index]
			let side: LabelSide = switch node.column {
			case 0: .leading
			case 1: .above
			case 2: node.key.kind == .categoryGroup ? .leading : .trailing
			default: .trailing
			}
			return Node(index: index, rect: rects[index], labelSide: side)
		}

		var outgoing = [CGFloat](repeating: 0, count: report.nodes.count)
		var incoming = [CGFloat](repeating: 0, count: report.nodes.count)
		ribbons = report.flows.indices.map { index in
			let flow = report.flows[index]
			let from = Int(flow.from), to = Int(flow.to)
			let thickness = CGFloat(flow.value) * scale
			let sourceTop = rects[from].minY + outgoing[from]
			let targetTop = rects[to].minY + incoming[to]
			outgoing[from] += thickness
			incoming[to] += thickness
			return Ribbon(
				flow: index, from: from, to: to,
				source: sourceTop...(sourceTop + thickness), sourceX: rects[from].maxX,
				target: targetTop...(targetTop + thickness), targetX: rects[to].minX
			)
		}
	}
}

/// The Sankey screen (R-REP-6, ST-8, design/mockups/sankey.html): where a
/// month's, quarter's, year's or custom range's income went.
@MainActor
@Observable
public final class SankeyModel {
	public private(set) var span: SankeySpan = .month
	public private(set) var period: ReportPeriod
	public private(set) var report: SankeyReport?

	@ObservationIgnored let workspace: WorkspaceModel
	@ObservationIgnored public let thisMonth: Month

	public init(workspace: WorkspaceModel, thisMonth: Month? = nil) {
		self.workspace = workspace
		let month = thisMonth ?? Month(date: WorkspaceModel.today()) ?? Month(year: 2026, month: 1)
		self.thisMonth = month
		period = ReportPeriod(first: month, last: month)
	}

	public var format: MoneyFormat { workspace.format }
	public var title: String { span.title(of: period) }
	public var canGoForward: Bool { period.last < thisMonth }

	public func load() async {
		let period = period
		guard let report = await workspace.report({ try await $0.sankey(first: period.first.firstDay, last: period.last.lastDay) }) else { return }
		guard period == self.period else { return }
		self.report = report
	}

	public func choose(_ span: SankeySpan) async {
		self.span = span
		period = span.period(containing: min(period.last, thisMonth), current: period)
		await load()
	}

	public func setCustom(first: Month, last: Month) async {
		span = .custom
		period = ReportPeriod(first: first, last: last)
		await load()
	}

	public func previous() async {
		period = period.shifted(by: -period.length)
		await load()
	}

	public func next() async {
		guard canGoForward else { return }
		period = period.shifted(by: period.length)
		await load()
	}

	/// What clicking a node opens, or `nil` for Shortfall, which has no rows.
	public func drilldown(_ node: SankeyNode) -> DrilldownModel? {
		guard let report, ReportWords.drillAction(for: node) != nil else { return nil }
		let caption = node.key.kind == .unallocated ? ReportWords.unallocatedCaption : nil
		return DrilldownModel(
			workspace: workspace,
			title: "\(ReportWords.node(node)), \(title)",
			caption: caption,
			drill: .sankeyNode(first: report.first, last: report.last, node: node.key)
		)
	}
}
