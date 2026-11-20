import AppKit
import VeritasDesign
import VeritasKit

/// The Tax summary as US Letter pages (R-REP-8, TAX-5): the statements' header
/// on every page, the bucket totals, the rows that need a decision, each
/// bucket's rows, TAX-8's known limits, and TAX-5's footer with page numbers.
@MainActor
enum TaxPDF {
	static let width: CGFloat = 612
	static let height: CGFloat = 792
	static let side: CGFloat = 54
	static let top: CGFloat = 48
	static let bottom: CGFloat = 64
	static var contentWidth: CGFloat { width - side * 2 }

	/// Date, payee, category, account; the amount takes what's left on the right.
	static let columns: [CGFloat] = [52, 124, 136, 100]

	static func data(_ document: TaxDocument) -> Data {
		let pages = paginate(blocks(document))
		let data = NSMutableData()
		var box = CGRect(x: 0, y: 0, width: width, height: height)
		let info: [CFString: Any] = [
			kCGPDFContextTitle: "\(document.entity) \(TaxDocument.title) \(document.year)",
			kCGPDFContextCreator: "Veritas"
		]
		guard let consumer = CGDataConsumer(data: data as CFMutableData),
			let context = CGContext(consumer: consumer, mediaBox: &box, info as CFDictionary)
		else { return Data() }
		for (index, blocks) in pages.enumerated() {
			context.beginPDFPage(nil)
			context.translateBy(x: 0, y: height)
			context.scaleBy(x: 1, y: -1)
			NSGraphicsContext.saveGraphicsState()
			NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
			var y = drawHeader(document)
			for block in blocks {
				block.draw(y)
				y += block.height
			}
			drawFooter(page: index + 1, of: pages.count)
			NSGraphicsContext.restoreGraphicsState()
			context.endPDFPage()
		}
		context.closePDF()
		return data as Data
	}

	struct Block {
		let height: CGFloat
		/// A heading moves to the next page with the block under it.
		let keepsWithNext: Bool
		let draw: (CGFloat) -> Void
	}

	static func paginate(_ blocks: [Block]) -> [[Block]] {
		let limit = height - bottom
		var pages: [[Block]] = [[]]
		var y = top + headerHeight
		var index = 0
		while index < blocks.count {
			var run = [blocks[index]]
			while run.last?.keepsWithNext == true, index + run.count < blocks.count {
				run.append(blocks[index + run.count])
			}
			let needed = run.reduce(0) { $0 + $1.height }
			if y + needed > limit, !(pages.last?.isEmpty ?? true) {
				pages.append([])
				y = top + headerHeight
			}
			for block in run {
				if y + block.height > limit, !(pages.last?.isEmpty ?? true) {
					pages.append([])
					y = top + headerHeight
				}
				pages[pages.count - 1].append(block)
				y += block.height
			}
			index += run.count
		}
		return pages
	}

	static func blocks(_ document: TaxDocument) -> [Block] {
		var blocks: [Block] = []
		if document.isEmpty {
			blocks.append(paragraph(document.emptyNote, color: secondary))
		} else {
			blocks.append(section("Summary"))
			blocks.append(summaryHeader())
			blocks += document.buckets.map(summaryRow)
		}
		if !document.conflicts.isEmpty {
			blocks.append(section("Needs a decision"))
			blocks.append(detailHeader())
			blocks += document.conflicts.map(detailRow)
		}
		for bucket in document.buckets {
			blocks.append(bucketHeading(bucket))
			blocks.append(detailHeader())
			blocks += bucket.rows.map(detailRow)
		}
		blocks.append(section(TaxDocument.limitsTitle))
		blocks += TaxDocument.limits.map { paragraph($0, color: ink, bullet: true) }
		return blocks
	}

	// MARK: Blocks

	private static func section(_ title: String) -> Block {
		let font = StatementPDF.font(14, .bold)
		return Block(height: 22 + lineHeight(font) + 6, keepsWithNext: true) { y in
			text(title, font: font, color: ink, in: CGRect(x: side, y: y + 22, width: contentWidth, height: lineHeight(font)))
		}
	}

	private static func bucketHeading(_ bucket: TaxDocument.Bucket) -> Block {
		let font = StatementPDF.font(12, .bold)
		return Block(height: 16 + lineHeight(font) + 4, keepsWithNext: true) { y in
			let top = y + 16
			text(bucket.name, font: font, color: ink, in: CGRect(x: side, y: top, width: contentWidth - 120, height: lineHeight(font)))
			trailing(bucket.total, font: font, color: ink, right: width - side, y: top)
		}
	}

	private static func summaryHeader() -> Block {
		let font = StatementPDF.font(9.5, .semibold)
		return Block(height: lineHeight(font) + 6, keepsWithNext: true) { y in
			text("Bucket", font: font, color: secondary, in: CGRect(x: side, y: y, width: 200, height: lineHeight(font)))
			trailing("Transactions", font: font, color: secondary, right: width - side - 130, y: y)
			trailing("Total", font: font, color: secondary, right: width - side, y: y)
			rule(y: y + lineHeight(font) + 3)
		}
	}

	private static func summaryRow(_ bucket: TaxDocument.Bucket) -> Block {
		let font = StatementPDF.font(11)
		let noteFont = StatementPDF.font(9.5)
		let noteHeight = bucket.note == nil ? 0 : lineHeight(noteFont) + 1
		return Block(height: lineHeight(font) + noteHeight + 6, keepsWithNext: false) { y in
			let top = y + 3
			text(bucket.name, font: font, color: ink, in: CGRect(x: side, y: top, width: 260, height: lineHeight(font)))
			trailing(bucket.count, font: font, color: ink, right: width - side - 130, y: top)
			trailing(bucket.total, font: font, color: ink, right: width - side, y: top)
			if let note = bucket.note {
				trailing(note, font: noteFont, color: secondary, right: width - side, y: top + lineHeight(font) + 1)
			}
		}
	}

	private static func detailHeader() -> Block {
		let font = StatementPDF.font(8.5, .semibold)
		return Block(height: lineHeight(font) + 5, keepsWithNext: true) { y in
			var x = side
			for (title, columnWidth) in zip(["Date", "Payee", "Category", "Account"], columns) {
				text(title, font: font, color: secondary, in: CGRect(x: x, y: y, width: columnWidth, height: lineHeight(font)))
				x += columnWidth
			}
			trailing("Amount", font: font, color: secondary, right: width - side, y: y)
			rule(y: y + lineHeight(font) + 2)
		}
	}

	private static func detailRow(_ row: TaxDocument.Row) -> Block {
		let font = StatementPDF.font(9.5)
		let noteFont = StatementPDF.font(8.5, italic: true)
		let noteHeight = row.note == nil ? 0 : lineHeight(noteFont)
		return Block(height: lineHeight(font) + noteHeight + 4, keepsWithNext: false) { y in
			let top = y + 2
			var x = side
			for (value, columnWidth) in zip([row.date, row.payee, row.category, row.account], columns) {
				text(value, font: font, color: ink, in: CGRect(x: x, y: top, width: columnWidth - 6, height: lineHeight(font)))
				x += columnWidth
			}
			trailing(row.amount, font: font, color: ink, right: width - side, y: top)
			if let note = row.note {
				text(note, font: noteFont, color: secondary, in: CGRect(x: side + columns[0], y: top + lineHeight(font), width: contentWidth - columns[0] - 90, height: lineHeight(noteFont)))
			}
		}
	}

	private static func paragraph(_ value: String, color: NSColor, bullet: Bool = false) -> Block {
		let font = StatementPDF.font(10)
		let indent: CGFloat = bullet ? 12 : 0
		let string = NSAttributedString(string: value, attributes: [.font: font, .foregroundColor: color])
		let textHeight = ceil(string.boundingRect(with: CGSize(width: contentWidth - indent, height: .greatestFiniteMagnitude), options: .usesLineFragmentOrigin).height)
		return Block(height: textHeight + 6, keepsWithNext: false) { y in
			if bullet {
				text("\u{2022}", font: font, color: color, in: CGRect(x: side, y: y + 2, width: indent, height: lineHeight(font)))
			}
			string.draw(with: CGRect(x: side + indent, y: y + 2, width: contentWidth - indent, height: textHeight + 2), options: .usesLineFragmentOrigin)
		}
	}

	// MARK: Header and footer

	private static var headerHeight: CGFloat {
		lineHeight(StatementPDF.font(18, .semibold)) + 4 + lineHeight(StatementPDF.font(14, .semibold)) + 2 + lineHeight(StatementPDF.font(11)) + 8
	}

	private static func drawHeader(_ document: TaxDocument) -> CGFloat {
		var y = top
		let entityFont = StatementPDF.font(18, .semibold)
		centered(document.entity, font: entityFont, color: ink, y: y)
		y += lineHeight(entityFont) + 4
		let titleFont = StatementPDF.font(14, .semibold)
		centered(TaxDocument.title, font: titleFont, color: ink, y: y)
		y += lineHeight(titleFont) + 2
		let basisFont = StatementPDF.font(11)
		centered(document.basis, font: basisFont, color: secondary, y: y)
		return y + lineHeight(basisFont) + 8
	}

	private static func drawFooter(page: Int, of count: Int) {
		let font = StatementPDF.font(9)
		let y = height - bottom + 20
		centered(TaxDocument.footer, font: font, color: secondary, y: y)
		if count > 1 {
			centered("Page \(page) of \(count)", font: font, color: secondary, y: y + lineHeight(font) + 2)
		}
	}

	// MARK: Drawing

	private static var ink: NSColor {
		let paper = ColorToken.textPaper.light
		return NSColor(srgbRed: paper.red, green: paper.green, blue: paper.blue, alpha: paper.alpha)
	}

	private static var secondary: NSColor { ink.withAlphaComponent(PaperMetrics.secondaryOpacity) }

	private static func text(_ value: String, font: NSFont, color: NSColor, in rect: CGRect) {
		NSAttributedString(string: value, attributes: [.font: font, .foregroundColor: color])
			.draw(with: rect, options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
	}

	private static func trailing(_ value: String, font: NSFont, color: NSColor, right: CGFloat, y: CGFloat) {
		let string = NSAttributedString(string: value, attributes: [.font: font, .foregroundColor: color])
		string.draw(at: CGPoint(x: right - string.size().width, y: y))
	}

	private static func centered(_ value: String, font: NSFont, color: NSColor, y: CGFloat) {
		let string = NSAttributedString(string: value, attributes: [.font: font, .foregroundColor: color])
		let textWidth = min(string.size().width, contentWidth)
		string.draw(with: CGRect(x: (width - textWidth) / 2, y: y, width: textWidth, height: string.size().height), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
	}

	private static func rule(y: CGFloat) {
		let path = NSBezierPath()
		path.move(to: CGPoint(x: side, y: y))
		path.line(to: CGPoint(x: width - side, y: y))
		path.lineWidth = 0.5
		ink.withAlphaComponent(PaperMetrics.ruleOpacity).setStroke()
		path.stroke()
	}

	private static func lineHeight(_ font: NSFont) -> CGFloat {
		ceil(font.ascender - font.descender + font.leading)
	}
}
