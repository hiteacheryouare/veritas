import VeritasCore

/// Tax bucket names as docs/finance/tax-buckets.md TAX-2 shows them.
public enum TaxBuckets {
	/// TAX-4: a negative bucket total stays negative, with this neutral note.
	public static let negativeNote = "Refunds or reversals exceed what was counted."

	/// The bucket's TAX-2 id, as the CSVs and `CategoryHistory.taxBucket` spell it.
	public static func id(_ bucket: TaxBucket) -> String {
		switch bucket {
		case .w2Income: "w2_income"
		case .interestIncome: "interest_income"
		case .businessIncome: "business_income"
		case .businessExpense: "business_expense"
		case .charitable: "charitable"
		case .medical: "medical"
		case .education: "education"
		case .hsa: "hsa"
		case .retirementContribution: "retirement_contribution"
		case .taxesWithheld: "taxes_withheld"
		}
	}

	public static func name(_ bucket: TaxBucket) -> String {
		name(id(bucket))
	}

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
