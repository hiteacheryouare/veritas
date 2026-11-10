-- Veritas-only tables, version 1. They live in the same SQLite file as
-- Actual's tables, which Actual ignores. veritas-store creates any that are
-- missing when it adopts a budget, then records schema_version in veritas_meta.
-- No foreign keys (Actual's schema has none); the invariant audit checks references.
-- Amounts are minor units, like Actual. Dates are YYYYMMDD integers and months are
-- 'YYYY-MM' text, matching Actual's own encodings.

CREATE TABLE IF NOT EXISTS veritas_meta (
	key TEXT PRIMARY KEY,
	value TEXT NOT NULL
);

-- docs/finance/classification.md CLS-3. A category with no row here defaults
-- from Actual's is_income flag. archived_on is set by Archive (R-BUD-5), which
-- also sets categories.hidden, so reports can label archived vs merely hidden.
CREATE TABLE IF NOT EXISTS veritas_category_meta (
	category_id TEXT PRIMARY KEY,
	kind TEXT NOT NULL CHECK (kind IN ('income', 'expense', 'debt_principal', 'savings')),
	tax_bucket TEXT,
	archived_on INTEGER
);

-- docs/finance/statements.md ST-2 (kind) and docs/finance/debt.md (loan fields, R-DEBT-1).
CREATE TABLE IF NOT EXISTS veritas_account_meta (
	account_id TEXT PRIMARY KEY,
	kind TEXT CHECK (kind IN ('asset', 'liability')),
	loan_rate_bps INTEGER CHECK (loan_rate_bps >= 0),
	loan_payment INTEGER CHECK (loan_payment >= 0),
	loan_payment_day INTEGER CHECK (loan_payment_day BETWEEN 1 AND 31),
	lender TEXT
);

-- R-GOAL-*, docs/finance/debt.md DEBT-8. status only moves forward.
CREATE TABLE IF NOT EXISTS veritas_goal (
	id TEXT PRIMARY KEY,
	category_id TEXT NOT NULL UNIQUE,
	target_cents INTEGER NOT NULL CHECK (target_cents > 0),
	target_date INTEGER,
	icon TEXT,
	color TEXT,
	status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'reached', 'closed')),
	reached_month TEXT,
	closed_month TEXT,
	released_to_category_id TEXT,
	released_cents INTEGER,
	created_at INTEGER NOT NULL
);

-- docs/finance/classification.md CLS-5: reconciliation adjustments Veritas made.
CREATE TABLE IF NOT EXISTS veritas_transaction_meta (
	transaction_id TEXT PRIMARY KEY,
	adjustment INTEGER NOT NULL DEFAULT 0 CHECK (adjustment IN (0, 1))
);

-- docs/finance/tax-buckets.md TAX-1: which Actual tags mean which bucket.
CREATE TABLE IF NOT EXISTS veritas_tax_tag (
	tag TEXT PRIMARY KEY,
	bucket TEXT NOT NULL
);

-- R-FC-2. Scenarios only ever write these two tables.
CREATE TABLE IF NOT EXISTS veritas_scenario (
	id TEXT PRIMARY KEY,
	name TEXT NOT NULL,
	sort_order REAL NOT NULL DEFAULT 0,
	created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS veritas_scenario_event (
	id TEXT PRIMARY KEY,
	scenario_id TEXT NOT NULL,
	kind TEXT NOT NULL CHECK (kind IN ('one_time', 'recurring', 'category_percent', 'income_change', 'goal_contribution')),
	account_id TEXT,
	category_id TEXT,
	goal_id TEXT,
	amount INTEGER,
	percent_bp INTEGER,
	start_month TEXT NOT NULL,
	end_month TEXT,
	every_months INTEGER CHECK (every_months > 0)
);
