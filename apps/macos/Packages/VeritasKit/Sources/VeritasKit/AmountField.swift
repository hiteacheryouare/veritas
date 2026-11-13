import Foundation
import Observation
import VeritasCore

/// What every amount field asks the core: the text it opens with, the
/// single-operator rule per keystroke, and what the text evaluates to (R-BUD-3).
public protocol AmountReader: Sendable {
	func editText(_ amount: Int64, style: NumberStyle) -> String
	func fieldText(original: String, typed: String) -> String
	func evaluate(_ text: String, style: NumberStyle) async throws -> Int64
}

public struct CoreAmountReader: AmountReader {
	public init() {}

	public func editText(_ amount: Int64, style: NumberStyle) -> String {
		(try? amountEditText(amount: amount, style: style)) ?? ""
	}

	public func fieldText(original: String, typed: String) -> String {
		(try? amountFieldText(original: original, typed: typed)) ?? typed
	}

	public func evaluate(_ text: String, style: NumberStyle) async throws -> Int64 {
		try await Core.run { try evaluateAmount(text: text, style: style) }
	}
}

/// One editing session of an amount field: it opens on the current value as
/// text, previews an expression's result, and keeps the old value when the
/// text can't be read (D37).
@MainActor
@Observable
public final class AmountFieldModel {
	public private(set) var text: String
	/// What Return would set, while the text is an expression.
	public private(set) var preview: Int64?
	public private(set) var problem: String?
	public let original: Int64

	@ObservationIgnored private let originalText: String
	@ObservationIgnored private let reader: any AmountReader
	@ObservationIgnored private let style: NumberStyle
	@ObservationIgnored private var previewTask: Task<Void, Never>?

	public init(amount: Int64, style: NumberStyle, reader: any AmountReader) {
		self.original = amount
		self.style = style
		self.reader = reader
		originalText = reader.editText(amount, style: style)
		text = originalText
	}

	public func type(_ typed: String) {
		let shaped = reader.fieldText(original: originalText, typed: typed)
		guard shaped != text else { return }
		text = shaped
		problem = nil
		updatePreview()
	}

	/// The amount to save, or `nil` with `problem` set when the text can't be read.
	public func commit() async -> Int64? {
		do {
			let amount = try await reader.evaluate(text, style: style)
			problem = nil
			return amount
		} catch FfiError.Overflow {
			problem = "That amount is too large."
		} catch {
			problem = "Can't read \(text) as an amount. Fix it or press Esc."
		}
		return nil
	}

	private func updatePreview() {
		previewTask?.cancel()
		let text = text
		guard Self.isExpression(text) else {
			preview = nil
			return
		}
		previewTask = Task { [reader, style] in
			let value = try? await reader.evaluate(text, style: style)
			guard !Task.isCancelled else { return }
			preview = value
		}
	}

	/// An operator after the first character: `412.00+40`, not `-50`.
	static func isExpression(_ text: String) -> Bool {
		text.dropFirst().contains { "+-*/^()".contains($0) }
	}
}
