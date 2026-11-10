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

/// What the app does with each error, as contracts/errors.md lays it out.
public enum ErrorPresentation: Hashable, Sendable {
	case nothing
	case refresh
	case inline(String)
	case readOnly(ReadOnlyKind)
	case integrityReport(violations: UInt64)
	case alert(ErrorAlert)

	public init(_ error: any Error) {
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
			self = .alert(ErrorAlert(title: "Couldn't save that change. Nothing was lost.", offersRetry: true))
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
			return .alert(ErrorAlert(title: message))
		}
		return self
	}

	private static func bug(details: String) -> ErrorAlert {
		ErrorAlert(title: "Veritas hit a bug.", message: "Your budget wasn't changed.", details: details)
	}
}
