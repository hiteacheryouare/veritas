import PDFKit
import SwiftUI
import Testing

/// The words a view draws. SwiftUI renders it into a PDF, which keeps text as
/// text, and PDFKit reads the text back, so nothing loads Vision or CoreML.
@MainActor
enum DrawnText {
	/// Each run of text as drawn, in reading order. Runs on one baseline
	/// that touch are joined, so "DIV_BY_ZERO" stays one string.
	static func texts(in view: some View, size: CGSize) throws -> [String] {
		let renderer = ImageRenderer(content: view.frame(width: size.width, height: size.height).environment(\.colorScheme, .light))
		let data = NSMutableData()
		renderer.render { size, draw in
			var box = CGRect(origin: .zero, size: size)
			guard let consumer = CGDataConsumer(data: data), let context = CGContext(consumer: consumer, mediaBox: &box, nil) else { return }
			context.beginPDFPage(nil)
			draw(context)
			context.endPDFPage()
			context.closePDF()
		}
		let page = try #require(PDFDocument(data: data as Data)?.page(at: 0), "the view rendered to a PDF page")
		let runs = (page.selection(for: page.bounds(for: .mediaBox))?.selectionsByLine() ?? [])
			.compactMap { run in run.string.map { (text: $0, bounds: run.bounds(for: page)) } }
			.sorted { a, b in
				// PDF space puts the origin bottom left, so the top line has the largest y.
				let (lineA, lineB) = (a.bounds.minY.rounded(), b.bounds.minY.rounded())
				return lineA == lineB ? a.bounds.minX < b.bounds.minX : lineA > lineB
			}
		var texts: [(text: String, bounds: CGRect)] = []
		for run in runs {
			if let last = texts.last, last.bounds.minY.rounded() == run.bounds.minY.rounded(), run.bounds.minX - last.bounds.maxX < 2 {
				texts[texts.count - 1] = (last.text + run.text, last.bounds.union(run.bounds))
			} else {
				texts.append(run)
			}
		}
		try #require(!texts.isEmpty, "the view drew text; views AppKit draws, such as a scroll view, render nothing here")
		return texts.map(\.text)
	}
}
