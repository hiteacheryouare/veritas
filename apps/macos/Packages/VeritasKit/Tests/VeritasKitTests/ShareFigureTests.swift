import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

struct ShareFigureTests {
	@Test func aShareWithNothingToDivideByIsBlank() {
		// Never "NaN%" or "∞%".
		#expect(CardFigure(summary: .nan, percentage: true) == .undefined)
		#expect(CardFigure(summary: .infinity, percentage: true) == .undefined)
		#expect(CardFigure(summary: 18.4, percentage: true) == .percent(18.4))
		#expect(CardFigure(summary: -12_345.6, percentage: false) == .money(-12_346))
	}
}
