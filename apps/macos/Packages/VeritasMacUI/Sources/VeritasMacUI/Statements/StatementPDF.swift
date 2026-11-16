import AppKit
import CoreText
import VeritasDesign
import VeritasKit

/// A statement as US Letter pages: the paper's header on every page, rows
/// that never leave a heading alone at the foot of a page, and page numbers.
@MainActor
enum StatementPDF {
	static let pageNumberSpace: CGFloat = 28

	static func data(_ document: StatementDocument, format: MoneyFormat) -> Data {
		let pages = paginate(document)
		let data = NSMutableData()
		var box = CGRect(x: 0, y: 0, width: PaperMetrics.width, height: PaperMetrics.height)
		let info: [CFString: Any] = [
			kCGPDFContextTitle: "\(document.entity) \(document.title)",
			kCGPDFContextCreator: "Veritas"
		]
		guard let consumer = CGDataConsumer(data: data as CFMutableData),
			let context = CGContext(consumer: consumer, mediaBox: &box, info as CFDictionary)
		else { return Data() }
		let hasGroups = document.lines.contains { $0.style == .group }
		for (index, lines) in pages.enumerated() {
			context.beginPDFPage(nil)
			context.translateBy(x: 0, y: PaperMetrics.height)
			context.scaleBy(x: 1, y: -1)
			NSGraphicsContext.saveGraphicsState()
			NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
			var y = drawHeader(document)
			var previous: StatementLine.Style?
			for line in lines {
				y = draw(line, at: y, after: previous, format: format, hasGroups: hasGroups)
				previous = line.style
			}
			if pages.count > 1 {
				drawCentered("Page \(index + 1) of \(pages.count)", font: font(PaperMetrics.pageNumberSize), color: secondary, y: PaperMetrics.height - PaperMetrics.bottom + 12)
			}
			NSGraphicsContext.restoreGraphicsState()
			context.endPDFPage()
		}
		context.closePDF()
		return data as Data
	}

	/// The lines on each page. A section or group heading moves to the next
	/// page with the row under it.
	static func paginate(_ document: StatementDocument) -> [[StatementLine]] {
		let top = PaperMetrics.top + headerHeight
		let limit = PaperMetrics.height - PaperMetrics.bottom - pageNumberSpace
		var pages: [[StatementLine]] = [[]]
		var y = top
		for (index, line) in document.lines.enumerated() {
			let previous = pages.last?.last?.style
			var needed = height(line, after: previous)
			if line.style == .section || line.style == .group, index + 1 < document.lines.count {
				needed += height(document.lines[index + 1], after: line.style)
			}
			if y + needed > limit, !(pages.last?.isEmpty ?? true) {
				pages.append([])
				y = top
			}
			y += height(line, after: pages.last?.last?.style)
			pages[pages.count - 1].append(line)
		}
		return pages
	}

	private static var ink: NSColor {
		let paper = ColorToken.textPaper.light
		return NSColor(srgbRed: paper.red, green: paper.green, blue: paper.blue, alpha: paper.alpha)
	}
	private static var secondary: NSColor { ink.withAlphaComponent(PaperMetrics.secondaryOpacity) }

	private static var headerHeight: CGFloat {
		lineHeight(font(PaperMetrics.entitySize, .semibold)) + 6
			+ lineHeight(font(PaperMetrics.titleSize, .semibold)) + 2
			+ lineHeight(font(PaperMetrics.basisSize)) + PaperMetrics.headerGap
	}

	static func height(_ line: StatementLine, after previous: StatementLine.Style?) -> CGFloat {
		PaperMetrics.spaceAbove(line.style, after: previous) + lineHeight(rowFont(line.style)) + PaperMetrics.rowPadding * 2
	}

	private static func drawHeader(_ document: StatementDocument) -> CGFloat {
		var y = PaperMetrics.top
		let entity = NSMutableAttributedString(string: document.entity, attributes: [.font: font(PaperMetrics.entitySize, .semibold), .foregroundColor: ink])
		entity.append(NSAttributedString(string: " (\(StatementDocument.unaudited))", attributes: [.font: font(PaperMetrics.unauditedSize, italic: true), .foregroundColor: secondary]))
		drawCentered(entity, y: y)
		y += lineHeight(font(PaperMetrics.entitySize, .semibold)) + 6
		drawCentered(document.title, font: font(PaperMetrics.titleSize, .semibold), color: ink, y: y)
		y += lineHeight(font(PaperMetrics.titleSize, .semibold)) + 2
		drawCentered(document.basis, font: font(PaperMetrics.basisSize), color: secondary, y: y)
		return y + lineHeight(font(PaperMetrics.basisSize)) + PaperMetrics.headerGap
	}

	private static func draw(_ line: StatementLine, at top: CGFloat, after previous: StatementLine.Style?, format: MoneyFormat, hasGroups: Bool) -> CGFloat {
		let rowFont = rowFont(line.style)
		let left = PaperMetrics.side
		let right = PaperMetrics.width - PaperMetrics.side
		let rowTop = top + PaperMetrics.spaceAbove(line.style, after: previous)
		let textTop = rowTop + PaperMetrics.rowPadding
		let titleFont = line.style == .group ? font(PaperMetrics.rowSize, italic: true) : rowFont
		let amountLeft = right - PaperMetrics.amountWidth
		let titleWidth = amountLeft - left - PaperMetrics.indent(line.style, hasGroups: hasGroups) - 12
		NSAttributedString(string: line.title, attributes: [.font: titleFont, .foregroundColor: ink])
			.draw(with: CGRect(x: left + PaperMetrics.indent(line.style, hasGroups: hasGroups), y: textTop, width: titleWidth, height: lineHeight(titleFont)), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
		if let amount = line.amount {
			let text = NSAttributedString(string: format.statement(amount, on: .register), attributes: [.font: rowFont, .foregroundColor: ink])
			text.draw(at: CGPoint(x: right - text.size().width, y: textTop))
		}
		switch line.style {
		case .subtotal:
			rule(from: amountLeft, to: right, y: rowTop + 0.5, width: 1, color: ink.withAlphaComponent(PaperMetrics.ruleOpacity))
		case .total:
			rule(from: amountLeft, to: right, y: rowTop + 0.5, width: 1, color: ink)
		case .grand:
			let bottom = textTop + lineHeight(rowFont) + PaperMetrics.rowPadding
			rule(from: amountLeft, to: right, y: bottom, width: 0.75, color: ink)
			rule(from: amountLeft, to: right, y: bottom + 2.25, width: 0.75, color: ink)
		default:
			break
		}
		return rowTop + lineHeight(rowFont) + PaperMetrics.rowPadding * 2
	}

	private static func rule(from x1: CGFloat, to x2: CGFloat, y: CGFloat, width: CGFloat, color: NSColor) {
		let path = NSBezierPath()
		path.move(to: CGPoint(x: x1, y: y))
		path.line(to: CGPoint(x: x2, y: y))
		path.lineWidth = width
		color.setStroke()
		path.stroke()
	}

	private static func drawCentered(_ text: String, font: NSFont, color: NSColor, y: CGFloat) {
		drawCentered(NSAttributedString(string: text, attributes: [.font: font, .foregroundColor: color]), y: y)
	}

	private static func drawCentered(_ text: NSAttributedString, y: CGFloat) {
		let width = min(text.size().width, PaperMetrics.width - PaperMetrics.side * 2)
		text.draw(with: CGRect(x: (PaperMetrics.width - width) / 2, y: y, width: width, height: text.size().height), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
	}

	private static func rowFont(_ style: StatementLine.Style) -> NSFont {
		font(PaperMetrics.size(style), PaperMetrics.isBold(style) ? .bold : .regular)
	}

	private static func lineHeight(_ font: NSFont) -> CGFloat {
		ceil(font.ascender - font.descender + font.leading)
	}

	/// New York, with tabular figures so amounts line up.
	static func font(_ size: CGFloat, _ weight: NSFont.Weight = .regular, italic: Bool = false) -> NSFont {
		var descriptor = NSFont.systemFont(ofSize: size, weight: weight).fontDescriptor
		descriptor = descriptor.withDesign(.serif) ?? descriptor
		if italic { descriptor = descriptor.withSymbolicTraits(.italic) }
		descriptor = descriptor.addingAttributes([.featureSettings: [[
			NSFontDescriptor.FeatureKey.typeIdentifier: kNumberSpacingType,
			NSFontDescriptor.FeatureKey.selectorIdentifier: kMonospacedNumbersSelector
		]]])
		return NSFont(descriptor: descriptor, size: size) ?? .systemFont(ofSize: size, weight: weight)
	}
}
