import VeritasCore

/// What the Integrity report sheet shows.
public enum IntegrityReport: Hashable, Sendable {
	/// The open budget's background check: `nil` findings while it still runs.
	case budget(findings: [AuditFinding]?)
	/// An import SQLite found damaged, so nothing was imported.
	case refusedImport(problems: UInt64)
	/// Findings an export came back with. The export itself was written.
	case export(findings: [AuditFinding])
}
