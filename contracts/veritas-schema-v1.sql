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

-- docs/finance/classification.md CLS-3. A category with no row, or a NULL kind,
-- defaults from Actual's is_income flag. Bucket ids are TAX-2's. archived_on is set by Archive (R-BUD-5), which
-- also sets categories.hidden, so reports can label archived vs merely hidden.
CREATE TABLE IF NOT EXISTS veritas_category_meta (
	category_id TEXT PRIMARY KEY,
	kind TEXT CHECK (kind IN ('income', 'expense', 'debt_principal', 'savings')),
	tax_bucket TEXT CHECK (tax_bucket IN ('w2_income', 'interest_income', 'business_income', 'business_expense', 'charitable', 'medical', 'education', 'hsa', 'retirement_contribution', 'taxes_withheld')),
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

-- R-GOAL-*, docs/finance/debt.md DEBT-8. status moves forward, except an explicit Reopen (DEBT-8.2).
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
	created_at INTEGER NOT NULL,
	-- docs/finance/debt.md GOAL-6 (D72): what the last dismiss of "new activity" saw.
	activity_seen_through TEXT,
	activity_seen_total INTEGER,
	close_month_dismissed_available INTEGER
);

-- docs/finance/classification.md CLS-5: reconciliation adjustments Veritas made.
CREATE TABLE IF NOT EXISTS veritas_transaction_meta (
	transaction_id TEXT PRIMARY KEY,
	adjustment INTEGER NOT NULL DEFAULT 0 CHECK (adjustment IN (0, 1)),
	needs_actual TEXT
);

-- D76, docs/finance/debt.md DEBT-3: which expense line of a loan payment the
-- wizard wrote as interest, escrow or fees. payment is the split's id, line
-- the expense child's; part is interest, escrow or fees, and a reader skips any
-- other value. Principal isn't stored: it is the transfer line.
CREATE TABLE IF NOT EXISTS veritas_loan_payment_part (
	id TEXT PRIMARY KEY,
	payment TEXT NOT NULL,
	line TEXT NOT NULL,
	part TEXT NOT NULL,
	tombstone INTEGER NOT NULL DEFAULT 0 CHECK (tombstone IN (0, 1))
);

-- R-BILL-3: Pause and End are both Actual's Complete (completed = 1). paused only
-- picks the label the bill screen shows for a completed bill; it never changes what posts.
CREATE TABLE IF NOT EXISTS veritas_schedule_meta (
	schedule_id TEXT PRIMARY KEY,
	paused INTEGER NOT NULL DEFAULT 0 CHECK (paused IN (0, 1)),
	display_name TEXT,
	icon TEXT
);

-- docs/finance/migration.md MIG-3.4: an Actual dashboard widget Veritas shows
-- as a native card while the fingerprint still matches (MIG-3.6, MIG-3.8).
CREATE TABLE IF NOT EXISTS veritas_widget_override (
	widget_id TEXT PRIMARY KEY,
	native_type TEXT NOT NULL,
	native_meta TEXT,
	fingerprint TEXT NOT NULL,
	created_at TEXT
);

-- MIG-7.1: what each upgraded item's Revert restores.
CREATE TABLE IF NOT EXISTS veritas_migration_applied (
	item_key TEXT PRIMARY KEY,
	prior TEXT,
	applied_at TEXT
);

-- docs/finance/tax-buckets.md TAX-1: which Actual tags mean which bucket.
CREATE TABLE IF NOT EXISTS veritas_tax_tag (
	tag TEXT PRIMARY KEY,
	bucket TEXT NOT NULL CHECK (bucket IN ('w2_income', 'interest_income', 'business_income', 'business_expense', 'charitable', 'medical', 'education', 'hsa', 'retirement_contribution', 'taxes_withheld', 'none'))
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
	on_date INTEGER,
	start_month TEXT NOT NULL,
	end_month TEXT,
	every_months INTEGER CHECK (every_months > 0),
	every_weeks INTEGER CHECK (every_weeks > 0),
	CHECK (every_months IS NULL OR every_weeks IS NULL)
);
