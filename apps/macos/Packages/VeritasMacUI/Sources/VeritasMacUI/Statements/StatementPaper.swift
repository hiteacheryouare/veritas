import SwiftUI
import VeritasDesign
import VeritasKit

/// The statement as printed (design/mockups/statement.html). The screen and
/// the PDF share these measurements, so the preview is the page.
enum PaperMetrics {
	static let width: CGFloat = 612
	static let height: CGFloat = 792
	static let top: CGFloat = 48
	static let side: CGFloat = 72
	static let bottom: CGFloat = 56
	static let amountWidth: CGFloat = 120
	static let headerGap: CGFloat = 24
	static let rowPadding: CGFloat = 4

	static let entitySize: CGFloat = 22
	static let unauditedSize: CGFloat = 16
	static let titleSize: CGFloat = 16
	static let basisSize: CGFloat = 13
	static let rowSize: CGFloat = 13.5
	static let sectionSize: CGFloat = 14
	static let grandSize: CGFloat = 15
	static let pageNumberSize: CGFloat = 10

	static let secondaryOpacity = 0.74
	static let ruleOpacity = 0.5

	static func size(_ style: StatementLine.Style) -> CGFloat {
		switch style {
		case .section: sectionSize
		case .grand: grandSize
		default: rowSize
		}
	}

	static func isBold(_ style: StatementLine.Style) -> Bool {
		[.section, .total, .grand].contains(style)
	}

	/// A grand total straight under its heading sits with it.
	static func spaceAbove(_ style: StatementLine.Style, after previous: StatementLine.Style?) -> CGFloat {
		switch style {
		case .section: 18
		case .group: 8
		case .grand: previous == .section || previous == .group ? 0 : 20
		default: 0
		}
	}

	/// Lines sit under a group heading when the statement has groups, else just in from the left.
	static func indent(_ style: StatementLine.Style, hasGroups: Bool) -> CGFloat {
		switch style {
		case .group, .subtotal: 14
		case .line: hasGroups ? 28 : 14
		default: 0
		}
	}
}

struct StatementPaper: View {
	let document: StatementDocument
	let format: MoneyFormat

	private var ink: Color { .textPaper }

	var body: some View {
		let hasGroups = document.lines.contains { $0.style == .group }
		VStack(spacing: 0) {
			header
				.padding(.bottom, PaperMetrics.headerGap)
			ForEach(Array(document.lines.enumerated()), id: \.element.id) { index, line in
				row(line, after: index > 0 ? document.lines[index - 1].style : nil, hasGroups: hasGroups)
			}
		}
		.padding(.top, PaperMetrics.top)
		.padding(.horizontal, PaperMetrics.side)
		.padding(.bottom, PaperMetrics.bottom)
		.frame(width: PaperMetrics.width)
		.foregroundStyle(ink)
		.background {
			RoundedRectangle(cornerRadius: 4)
				.fill(.surfacePaper)
				.shadow(color: .black.opacity(0.14), radius: 14, y: 6)
		}
		.environment(\.colorScheme, .light)
		.containerIdentifier("statements.paper")
	}

	private var header: some View {
		VStack(spacing: 0) {
			let entity = Text(document.entity).font(Self.font(PaperMetrics.entitySize, .semibold))
			let unaudited = Text("(\(StatementDocument.unaudited))").font(Self.font(PaperMetrics.unauditedSize).italic()).foregroundStyle(ink.opacity(PaperMetrics.secondaryOpacity))
			Text("\(entity) \(unaudited)")
				.accessibilityIdentifier("statements.entity")
			Text(document.title)
				.font(Self.font(PaperMetrics.titleSize, .semibold))
				.padding(.top, 6)
				.padding(.bottom, 2)
				.accessibilityIdentifier("statements.title")
			Text(document.basis)
				.font(Self.font(PaperMetrics.basisSize))
				.foregroundStyle(ink.opacity(PaperMetrics.secondaryOpacity))
				.accessibilityIdentifier("statements.basis")
		}
		.multilineTextAlignment(.center)
	}

	private func row(_ line: StatementLine, after previous: StatementLine.Style?, hasGroups: Bool) -> some View {
		let font = Self.font(PaperMetrics.size(line.style), PaperMetrics.isBold(line.style) ? .bold : .regular)
		return HStack(alignment: .firstTextBaseline, spacing: Spacing.s3) {
			Text(line.title)
				.font(line.style == .group ? font.italic() : font)
				.padding(.leading, PaperMetrics.indent(line.style, hasGroups: hasGroups))
				.frame(maxWidth: .infinity, alignment: .leading)
			Text(line.amount.map { format.statement($0, on: .register) } ?? "")
				.font(font.monospacedDigit())
				.lineLimit(1)
				.frame(width: PaperMetrics.amountWidth, alignment: .trailing)
				.overlay(alignment: .top) { rule(above: line.style).offset(y: -PaperMetrics.rowPadding) }
				.overlay(alignment: .bottom) { if line.style == .grand { doubleRule } }
		}
		.padding(.vertical, PaperMetrics.rowPadding)
		.padding(.top, PaperMetrics.spaceAbove(line.style, after: previous))
		.accessibilityElement(children: .combine)
		.accessibilityIdentifier("statements.line.\(line.id)")
	}

	@ViewBuilder private func rule(above style: StatementLine.Style) -> some View {
		switch style {
		case .subtotal: Rectangle().fill(ink.opacity(PaperMetrics.ruleOpacity)).frame(height: 1)
		case .total: Rectangle().fill(ink).frame(height: 1)
		default: EmptyView()
		}
	}

	private var doubleRule: some View {
		VStack(spacing: 1.5) {
			Rectangle().frame(height: 0.75)
			Rectangle().frame(height: 0.75)
		}
		.foregroundStyle(ink)
		.offset(y: PaperMetrics.rowPadding + 2)
	}

	static func font(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
		.system(size: size, weight: weight, design: .serif)
	}
}
