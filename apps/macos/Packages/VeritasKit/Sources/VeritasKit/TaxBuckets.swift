/// Tax bucket names as docs/finance/tax-buckets.md TAX-1 shows them.
public enum TaxBuckets {
	public static func name(_ id: String) -> String {
		switch id {
		case "w2_income": "W-2 income"
		case "interest_income": "Interest income"
		case "business_income": "Business income"
		case "business_expense": "Business expenses"
		case "charitable": "Charitable giving"
		case "medical": "Medical"
		case "education": "Education"
		case "hsa": "HSA contributions"
		case "retirement_contribution": "Retirement contributions withheld"
		case "taxes_withheld": "Taxes withheld from pay"
		default: id
		}
	}

	/// TAX-8: moving a category's history into one with a different bucket
	/// counts those rows under the target's bucket from then on. Informational;
	/// it never blocks the move.
	public static func mergeWarning(source: String, sourceBucket: String?, target: String, targetBucket: String?) -> String? {
		guard let sourceBucket, sourceBucket != targetBucket else { return nil }
		let targetHas = targetBucket.map(name) ?? "none"
		return "\(source) is in tax bucket \(name(sourceBucket)). \(target) has \(targetHas). Its rows will count under \(target)."
	}
}
