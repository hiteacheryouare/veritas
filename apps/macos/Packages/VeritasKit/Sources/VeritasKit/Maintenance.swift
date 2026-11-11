import Foundation
import VeritasCore

/// One budget folder, for the Open Budget picker.
public struct BudgetListingItem: Hashable, Identifiable, Sendable {
	public let id: String
	public let name: String?
	public let folder: URL
	public let lastOpened: Date?

	public init(id: String, name: String?, folder: URL, lastOpened: Date?) {
		self.id = id
		self.name = name
		self.folder = folder
		self.lastOpened = lastOpened
	}
}

public struct BackupStatus: Hashable, Sendable {
	public let folder: URL?
	public let lastAt: Date?
	public let lastFile: String?
	public let lastProblem: String?

	public init(folder: URL?, lastAt: Date?, lastFile: String?, lastProblem: String?) {
		self.folder = folder
		self.lastAt = lastAt
		self.lastFile = lastFile
		self.lastProblem = lastProblem
	}

	public static let none = BackupStatus(folder: nil, lastAt: nil, lastFile: nil, lastProblem: nil)
}

/// How often the app writes a backup zip. The app schedules it; the core never does (R-REL-4).
public enum BackupSchedule: String, CaseIterable, Hashable, Sendable {
	case off
	case daily
	case weekly

	public var title: String {
		switch self {
		case .off: "Off"
		case .daily: "Daily"
		case .weekly: "Weekly"
		}
	}

	public func isDue(lastBackup: Date?, now: Date) -> Bool {
		let interval: TimeInterval
		switch self {
		case .off: return false
		case .daily: interval = 24 * 60 * 60
		case .weekly: interval = 7 * 24 * 60 * 60
		}
		guard let lastBackup else { return true }
		return now.timeIntervalSince(lastBackup) >= interval
	}
}

/// Per-budget settings the Mac keeps rather than the budget file: the backup
/// folder's security-scoped bookmark and the schedule.
public protocol LocalBudgetSettings: Sendable {
	/// Resolves the saved folder and starts its sandbox access, or `nil`.
	func backupFolder(for budget: URL) -> URL?
	func rememberBackupFolder(_ folder: URL?, for budget: URL) throws
	func backupSchedule(for budget: URL) -> BackupSchedule
	func setBackupSchedule(_ schedule: BackupSchedule, for budget: URL)
}

public extension AuditCheck {
	var title: String {
		switch self {
		case .damage: "The file is damaged"
		case .unrepresentable: "Rows Veritas can't show"
		case .unpairedTransfer: "Transfer with no other side"
		case .strayTransferLink: "Transfer linked to a missing transaction"
		case .transferAmountMismatch: "Transfer sides don't match"
		case .splitSumMismatch: "Split parts don't add up"
		case .emptySplit: "Split with no parts"
		case .childOfNonParent: "Split part without its split"
		case .missingReference: "Points to a row that doesn't exist"
		case .deletedReference: "Points to a deleted row"
		case .mappingChain: "Merged item that points to another merge"
		case .mappingCycle: "Merged items that point to each other"
		case .duplicateBudgetCell: "Budget amount stored twice"
		case .unknownTaxBucket: "Unknown tax bucket"
		}
	}
}

public extension SnapshotKind {
	var title: String {
		switch self {
		case .open: "Opened"
		case .import: "Imported"
		case .export: "Before export"
		case .versionChange: "Before a version change"
		case .bulkEdit: "Before a bulk edit"
		case .ruleApply: "Before applying rules"
		case .hourly: "Hourly"
		case .restore: "Before a restore"
		}
	}
}

/// The core's moments are UTC `YYYY-MM-DDTHH:MM:SSZ`.
enum CoreTime {
	static func date(_ text: String?) -> Date? {
		guard let text else { return nil }
		return try? Date(text, strategy: .iso8601)
	}
}
