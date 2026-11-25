/// Contract v20 adds `EnvelopeRow.kind`. uniffi gives an enum field no
/// default, so this initializer without `kind` keeps Swift written before
/// v20 compiling, with `.expense`.
extension EnvelopeRow {
	public init(id: String, name: String, hidden: Bool, budgeted: Int64, spent: Int64, available: Int64, carryover: Bool, tone: MoneyTone, fill: Double, note: String?) {
		self.init(id: id, name: name, hidden: hidden, budgeted: budgeted, spent: spent, available: available, carryover: carryover, tone: tone, fill: fill, note: note, kind: .expense)
	}
}
