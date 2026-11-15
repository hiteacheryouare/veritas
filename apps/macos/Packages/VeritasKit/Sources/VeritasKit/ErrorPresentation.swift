import VeritasCore

public struct ErrorAlert: Hashable, Identifiable, Sendable {
	public let title: String
	public let message: String?
	public let offersRetry: Bool
	public let details: String?

	public var id: Self { self }

	public init(title: String, message: String? = nil, offersRetry: Bool = false, details: String? = nil) {
		self.title = title
		self.message = message
		self.offersRetry = offersRetry
		self.details = details
	}
}

/// What was happening when an error arrived, so a Storage failure names it.
public enum Operation: Hashable, Sendable {
	case change
	case open
	case creating
	case importing
	case export
}

/// What the app does with each error, as contracts/errors.md lays it out.
/// Alert titles have no closing period; messages are full sentences.
public enum ErrorPresentation: Hashable, Sendable {
	case nothing
	case refresh
	case inline(String)
	case readOnly(ReadOnlyKind)
	case integrityReport(violations: UInt64)
	case alert(ErrorAlert)

	public init(_ error: any Error, during operation: Operation = .change) {
		guard let error = error as? FfiError else {
			self = .alert(Self.bug(details: String(describing: error)))
			return
		}
		switch error {
		case .NotFound:
			self = .refresh
		case .Invalid(let message):
			self = .inline(message)
		case .ReadOnly(let reason):
			self = .readOnly(reason)
		case .UpgradeInActualFirst(let required):
			self = .alert(ErrorAlert(
				title: "Upgrade this budget in Actual first",
				message: "Open this budget in Actual \(required) or newer, then export it again."
			))
		case .AuditFailed(let violations):
			self = .integrityReport(violations: violations)
		case .NotAnActualBudget(let message):
			self = .alert(ErrorAlert(title: "That file isn't an Actual budget", message: message))
		case .Storage:
			self = .alert(Self.storage(during: operation))
		case .Io(let message):
			self = .alert(ErrorAlert(title: "Couldn't use a file or folder", message: message))
		case .Overflow:
			self = .inline("That amount is too large.")
		case .Cancelled:
			self = .nothing
		case .Internal(let message):
			self = .alert(Self.bug(details: message))
		}
	}

	/// Away from a field there is nowhere to show an inline message, so it becomes an alert.
	public var withoutField: ErrorPresentation {
		if case .inline(let message) = self {
			let title = message.hasSuffix(".") ? String(message.dropLast()) : message
			return .alert(ErrorAlert(title: title))
		}
		return self
	}

	private static func storage(during operation: Operation) -> ErrorAlert {
		switch operation {
		case .change:
			ErrorAlert(title: "Couldn't save that change", message: "Nothing was lost.", offersRetry: true)
		case .open:
			ErrorAlert(title: "Couldn't open this budget", message: "Nothing in it was changed.", offersRetry: true)
		case .creating:
			ErrorAlert(title: "Couldn't create the budget", message: "Nothing was saved.", offersRetry: true)
		case .importing:
			ErrorAlert(title: "Couldn't import that budget", message: "Your Actual export wasn't changed.", offersRetry: true)
		case .export:
			ErrorAlert(title: "Couldn't export this budget", message: "Nothing in it was changed.", offersRetry: true)
		}
	}

	private static func bug(details: String) -> ErrorAlert {
		ErrorAlert(title: "Veritas hit a bug", message: "Your budget wasn't changed.", details: details)
	}
}
