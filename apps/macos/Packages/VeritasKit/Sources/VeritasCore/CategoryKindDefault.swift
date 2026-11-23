/// Contract v20 adds `EnvelopeRow.kind` with `#[uniffi(default)]`, which
/// the generated initializer spells `CategoryKind()`. An enum needs this to
/// answer it, so code written before v20 keeps compiling with `.expense`.
extension CategoryKind {
	public init() {
		self = .expense
	}
}
