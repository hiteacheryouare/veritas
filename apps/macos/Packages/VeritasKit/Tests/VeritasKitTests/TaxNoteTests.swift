import Testing
import VeritasCore
@testable import VeritasKit

/// TAX-4's note follows the core's `negative` flag (contract v16), not a sign test in Swift.
struct TaxNoteTests {
	@Test func theNoteShowsWhenTheCoreFlagsTheTotal() {
		let refunded = TaxBucketTotal(bucket: .medical, inflow: false, total: -1_500, count: 2, negative: true)
		#expect(TaxBuckets.note(for: refunded) == "Refunds or reversals exceed what was counted.")
		let counted = TaxBucketTotal(bucket: .medical, inflow: false, total: 6_800, count: 2, negative: false)
		#expect(TaxBuckets.note(for: counted) == nil)
	}

	@Test func swiftDoesNotReadTheSignItself() {
		let unflagged = TaxBucketTotal(bucket: .w2Income, inflow: true, total: -100, count: 1, negative: false)
		#expect(TaxBuckets.note(for: unflagged) == nil, "only the core decides when the note is due")
	}
}
