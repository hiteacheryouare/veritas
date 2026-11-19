import CoreGraphics
import Foundation
import VeritasCore

/// Where the Sankey card's nodes and ribbons go. The core sends the graph;
/// this only places it: each node in the column of its longest path from a
/// source, sinks in the last column, and each node as tall as the larger of
/// what flows in and out of it. Hidden spacer nodes keep their column but
/// aren't drawn, and links into or out of them (value -1) or cut by the
/// layer range (no target) draw nothing.
public struct SankeyCardLayout {
	public struct Node: Hashable {
		public let index: Int
		public let column: Int
		public let rect: CGRect
	}

	public struct Ribbon: Hashable {
		public let link: Int
		public let source: Int
		public let target: Int
		public let sourceX: CGFloat
		public let targetX: CGFloat
		/// The ribbon's top and bottom where it leaves the source and enters the target.
		public let sourceSpan: ClosedRange<CGFloat>
		public let targetSpan: ClosedRange<CGFloat>
	}

	public let nodes: [Node]
	public let ribbons: [Ribbon]
	public let columns: Int

	public init(nodes graphNodes: [SankeyCardNode], links: [SankeyCardLink], size: CGSize, nodeWidth: CGFloat = 8, gap: CGFloat = 6) {
		let count = graphNodes.count
		let edges = links.enumerated().compactMap { index, link -> (index: Int, source: Int, target: Int, weight: CGFloat)? in
			guard let target = link.target.map(Int.init), target < count, Int(link.source) < count else { return nil }
			return (index, Int(link.source), target, CGFloat(max(link.value, 0)))
		}
		var depth = Array(repeating: 0, count: count)
		for _ in 0..<count {
			var changed = false
			for edge in edges where depth[edge.target] < depth[edge.source] + 1 {
				depth[edge.target] = depth[edge.source] + 1
				changed = true
			}
			if !changed { break }
		}
		let last = depth.max() ?? 0
		let hasOutflow = Set(edges.map(\.source))
		for index in 0..<count where !hasOutflow.contains(index) && edges.contains(where: { $0.target == index }) {
			depth[index] = last
		}
		var weight = Array(repeating: CGFloat(0), count: count)
		for index in 0..<count {
			let inflow = edges.filter { $0.target == index }.reduce(0) { $0 + $1.weight }
			let outflow = edges.filter { $0.source == index }.reduce(0) { $0 + $1.weight }
			weight[index] = max(inflow, outflow)
		}
		let drawn = (0..<count).filter { graphNodes[$0].label != .hidden && weight[$0] > 0 }
		let byColumn = Dictionary(grouping: drawn) { depth[$0] }
		let scale = byColumn.values.map { members -> CGFloat in
			let total = members.reduce(0) { $0 + weight[$1] }
			let room = size.height - gap * CGFloat(max(members.count - 1, 0))
			return total > 0 ? max(room, 1) / total : .infinity
		}.min() ?? 1
		let unit = scale.isFinite ? scale : 1
		let step = last > 0 ? (size.width - nodeWidth) / CGFloat(last) : 0
		var rects: [Int: CGRect] = [:]
		for (column, members) in byColumn {
			var y: CGFloat = 0
			for index in members {
				let height = max(weight[index] * unit, 1)
				rects[index] = CGRect(x: CGFloat(column) * step, y: y, width: nodeWidth, height: height)
				y += height + gap
			}
		}
		nodes = drawn.compactMap { index in rects[index].map { Node(index: index, column: depth[index], rect: $0) } }
		columns = last + 1

		var leaving: [Int: CGFloat] = [:]
		var entering: [Int: CGFloat] = [:]
		var ribbons: [Ribbon] = []
		for edge in edges where edge.weight > 0 {
			guard let from = rects[edge.source], let to = rects[edge.target] else { continue }
			let thickness = edge.weight * unit
			let top = from.minY + leaving[edge.source, default: 0]
			let bottom = to.minY + entering[edge.target, default: 0]
			leaving[edge.source, default: 0] += thickness
			entering[edge.target, default: 0] += thickness
			ribbons.append(Ribbon(
				link: edge.index, source: edge.source, target: edge.target, sourceX: from.maxX, targetX: to.minX,
				sourceSpan: top...(top + thickness), targetSpan: bottom...(bottom + thickness)
			))
		}
		self.ribbons = ribbons
	}
}
