import CoreGraphics
import Foundation
import VeritasCore

/// A node or a flow, by its index in the graph.
public enum SankeyTarget: Hashable, Sendable {
	case node(Int)
	case flow(Int)
}

/// What lights up when a node or flow is hovered or clicked: the node's
/// flows in and out, or the flow and its two ends, followed on through any
/// node that has only one flow on that side. A category lights its group
/// and the group's flow from Income; a group lights its categories and its
/// flow from Income. Income gathers every source, so the path stops there.
public struct SankeyHighlight: Equatable, Sendable {
	public let nodes: Set<Int>
	public let flows: Set<Int>

	/// `links` are each flow's source and target by node index; a flow
	/// without a target draws nothing and never lights up.
	public init(_ target: SankeyTarget, links: [(source: Int, target: Int?)]) {
		let edges = links.enumerated().compactMap { index, link in link.target.map { (index: index, source: link.source, target: $0) } }
		var nodes = Set<Int>()
		var flows = Set<Int>()
		func upstream(from node: Int) {
			var at = node
			while true {
				let into = edges.filter { $0.target == at }
				guard into.count == 1, let edge = into.first, flows.insert(edge.index).inserted else { return }
				nodes.insert(edge.source)
				at = edge.source
			}
		}
		func downstream(from node: Int) {
			var at = node
			while true {
				let out = edges.filter { $0.source == at }
				guard out.count == 1, let edge = out.first, flows.insert(edge.index).inserted else { return }
				nodes.insert(edge.target)
				at = edge.target
			}
		}
		switch target {
		case .node(let node):
			nodes.insert(node)
			for edge in edges where edge.target == node {
				flows.insert(edge.index)
				nodes.insert(edge.source)
				upstream(from: edge.source)
			}
			for edge in edges where edge.source == node {
				flows.insert(edge.index)
				nodes.insert(edge.target)
				downstream(from: edge.target)
			}
		case .flow(let flow):
			if let edge = edges.first(where: { $0.index == flow }) {
				flows.insert(flow)
				nodes.formUnion([edge.source, edge.target])
				upstream(from: edge.source)
				downstream(from: edge.target)
			}
		}
		self.nodes = nodes
		self.flows = flows
	}

	public func contains(_ target: SankeyTarget) -> Bool {
		switch target {
		case .node(let node): nodes.contains(node)
		case .flow(let flow): flows.contains(flow)
		}
	}
}

/// A ribbon's outline: two cubic curves from the source's edge to the
/// target's, as d3-sankey draws a link.
public func sankeyRibbon(sourceX: CGFloat, source: ClosedRange<CGFloat>, targetX: CGFloat, target: ClosedRange<CGFloat>) -> CGPath {
	let path = CGMutablePath()
	let middle = (sourceX + targetX) / 2
	path.move(to: CGPoint(x: sourceX, y: source.lowerBound))
	path.addCurve(to: CGPoint(x: targetX, y: target.lowerBound), control1: CGPoint(x: middle, y: source.lowerBound), control2: CGPoint(x: middle, y: target.lowerBound))
	path.addLine(to: CGPoint(x: targetX, y: target.upperBound))
	path.addCurve(to: CGPoint(x: sourceX, y: source.upperBound), control1: CGPoint(x: middle, y: target.upperBound), control2: CGPoint(x: middle, y: source.upperBound))
	path.closeSubpath()
	return path
}

/// A node's hit area: a little wider than the node, and never shorter than
/// a pointer can comfortably land on.
func sankeyHitRect(_ rect: CGRect) -> CGRect {
	let height = max(rect.height, 10)
	return CGRect(x: rect.minX - 4, y: rect.midY - height / 2, width: rect.width + 8, height: height)
}

extension SankeyLayout.Ribbon {
	public var outline: CGPath {
		sankeyRibbon(sourceX: sourceX, source: source, targetX: targetX, target: target)
	}
}

extension SankeyLayout {
	public var links: [(source: Int, target: Int?)] {
		ribbons.sorted { $0.flow < $1.flow }.map { ($0.from, $0.to) }
	}

	/// The node or flow under `point`. Nodes win over the ribbons that touch them.
	public func target(at point: CGPoint) -> SankeyTarget? {
		if let node = nodes.first(where: { sankeyHitRect($0.rect).contains(point) }) {
			return .node(node.index)
		}
		return ribbons.last { $0.outline.contains(point) }.map { .flow($0.flow) }
	}
}

extension SankeyCardLayout.Ribbon {
	public var outline: CGPath {
		sankeyRibbon(sourceX: sourceX, source: sourceSpan, targetX: targetX, target: targetSpan)
	}
}

extension SankeyCardLayout {
	public static func links(_ links: [SankeyCardLink]) -> [(source: Int, target: Int?)] {
		links.map { link in (Int(link.source), link.value < 0 ? nil : link.target.map(Int.init)) }
	}

	public func target(at point: CGPoint) -> SankeyTarget? {
		if let node = nodes.first(where: { sankeyHitRect($0.rect).contains(point) }) {
			return .node(node.index)
		}
		return ribbons.last { $0.outline.contains(point) }.map { .flow($0.link) }
	}
}

/// What the hover readout shows for a node or flow: its words and amount.
public struct SankeyReadout: Equatable, Sendable {
	public let title: String
	public let amount: Int64
}

extension SankeyReport {
	public func readout(_ target: SankeyTarget) -> SankeyReadout? {
		switch target {
		case .node(let index):
			guard nodes.indices.contains(index) else { return nil }
			return SankeyReadout(title: ReportWords.node(nodes[index]), amount: nodes[index].value)
		case .flow(let index):
			guard flows.indices.contains(index) else { return nil }
			let flow = flows[index]
			return SankeyReadout(title: "\(ReportWords.node(nodes[Int(flow.from)])) → \(ReportWords.node(nodes[Int(flow.to)]))", amount: flow.value)
		}
	}

	/// The node whose transactions a click opens. A flow opens the end that
	/// isn't Income: a source's flow into Income opens the source, and every
	/// other flow opens the node it reaches, whose rows are the flow's rows.
	public func drillNode(_ target: SankeyTarget) -> SankeyNode? {
		switch target {
		case .node(let index):
			return nodes.indices.contains(index) ? nodes[index] : nil
		case .flow(let index):
			guard flows.indices.contains(index) else { return nil }
			let flow = flows[index]
			let from = nodes[Int(flow.from)]
			return from.column == 0 ? from : nodes[Int(flow.to)]
		}
	}
}

extension SankeyCardNode {
	/// The node's words, with the Other node's contents under it.
	public static func readout(_ target: SankeyTarget, nodes: [SankeyCardNode], links: [SankeyCardLink]) -> SankeyReadout? {
		switch target {
		case .node(let index):
			guard nodes.indices.contains(index), let words = CardWords.sankey(nodes[index].label) else { return nil }
			let into = links.filter { $0.target.map(Int.init) == index && $0.value > 0 }.reduce(Int64(0)) { $0 + $1.value }
			let out = links.filter { Int($0.source) == index && $0.value > 0 }.reduce(Int64(0)) { $0 + $1.value }
			return SankeyReadout(title: words, amount: max(into, out))
		case .flow(let index):
			guard links.indices.contains(index), let target = links[index].target.map(Int.init), nodes.indices.contains(target),
				let from = CardWords.sankey(nodes[Int(links[index].source)].label), let to = CardWords.sankey(nodes[target].label) else { return nil }
			return SankeyReadout(title: "\(from) → \(to)", amount: links[index].value)
		}
	}
}
