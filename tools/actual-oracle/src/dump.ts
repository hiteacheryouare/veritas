import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';

import { compareCodePoints, sqlReal } from './canonicalJson.ts';
import type { CanonicalValue } from './canonicalJson.ts';
import { withTempDir } from './environment.ts';
import type { ActualSession } from './environment.ts';
import { locateBudgetFiles, readZip } from './zip.ts';

export const DUMP_FORMAT = 'actual-canonical-dump';
export const DUMP_VERSION = 1;

const EXCLUDED_TABLES = new Set(['kvcache', 'kvcache_key']);

export const VOLATILE_METADATA_KEYS = [
	'cloudFileId',
	'encryptKeyId',
	'groupId',
	'id',
	'lastSyncedTimestamp',
	'lastUploaded',
	'userId'
];

const UPCOMING_SCHEDULE_DATES = 12;

const upcomingDates = async (api: any, schedule: any, today: string): Promise<CanonicalValue> => {
	if (schedule.completed) {
		return [];
	}
	try {
		return await api.getScheduleDates(schedule.date, { start: today, count: UPCOMING_SCHEDULE_DATES });
	} catch (error) {
		console.error(`Actual cannot expand the dates of schedule ${schedule.id}: ${(error as Error).message}`);
		return null;
	}
};

type CanonicalObject = { [key: string]: CanonicalValue };

interface ColumnInfo {
	name: string;
	pk: number;
}

const quoteIdentifier = (name: string) => `"${name.replaceAll('"', '""')}"`;

const toCanonicalCell = (value: unknown): CanonicalValue => {
	if (typeof value === 'number') {
		return sqlReal(value);
	}
	return value as CanonicalValue;
};

const normalizeMetadata = (bytes: Uint8Array): CanonicalObject => {
	const metadata = JSON.parse(Buffer.from(bytes).toString('utf8'));
	for (const key of VOLATILE_METADATA_KEYS) {
		delete metadata[key];
	}
	return metadata;
};

const dumpSchema = (db: DatabaseSync): CanonicalValue[] =>
	db
		.prepare(`SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite\\_%' ESCAPE '\\'`)
		.all()
		.map(row => ({ type: row.type, name: row.name, tableName: row.tbl_name, sql: row.sql }) as CanonicalObject)
		.sort((a, b) =>
			compareCodePoints(a.type as string, b.type as string) || compareCodePoints(a.name as string, b.name as string)
		);

const dumpTable = (db: DatabaseSync, table: string): CanonicalObject => {
	const columns = db.prepare(`PRAGMA table_info(${quoteIdentifier(table)})`).all() as unknown as ColumnInfo[];
	const primaryKey = columns.filter(column => column.pk > 0).sort((a, b) => a.pk - b.pk).map(column => column.name);
	const orderBy = (primaryKey.length > 0 ? primaryKey : columns.map(column => column.name))
		.map(name => `${quoteIdentifier(name)} COLLATE BINARY`)
		.join(', ');
	const statement = db.prepare(`SELECT * FROM ${quoteIdentifier(table)} ORDER BY ${orderBy}`);
	statement.setReadBigInts(true);
	const rows: CanonicalValue[] = [];
	for (const row of statement.iterate()) {
		const cells: CanonicalObject = {};
		for (const [key, value] of Object.entries(row)) {
			cells[key] = toCanonicalCell(value);
		}
		rows.push(cells);
	}
	return { primaryKey, rows };
};

export const rawDump = (zipBytes: Uint8Array): Promise<CanonicalObject> =>
	withTempDir('actual-oracle-raw-', async dir => {
		const files = locateBudgetFiles(readZip(zipBytes));
		const dbPath = join(dir, 'db.sqlite');
		writeFileSync(dbPath, files.db);
		const db = new DatabaseSync(dbPath, { readOnly: true });
		try {
			const tableNames = db
				.prepare(`SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'`)
				.all()
				.map(row => row.name as string)
				.filter(name => !EXCLUDED_TABLES.has(name));
			const tables: CanonicalObject = {};
			for (const name of tableNames) {
				tables[name] = dumpTable(db, name);
			}
			return {
				format: DUMP_FORMAT,
				version: DUMP_VERSION,
				metadata: normalizeMetadata(files.metadata),
				schema: dumpSchema(db),
				tables
			};
		} finally {
			db.close();
		}
	});

const ENTITY_KEYS = new Set(['id', 'name', 'is_income', 'hidden', 'group_id', 'categories']);

const valuesOf = (entity: Record<string, unknown>): CanonicalObject =>
	Object.fromEntries(Object.entries(entity).filter(([key]) => !ENTITY_KEYS.has(key))) as CanonicalObject;

const dumpMonth = (month: any): CanonicalObject => {
	const { month: _month, categoryGroups, ...totals } = month;
	const groups: CanonicalObject = {};
	const categories: CanonicalObject = {};
	for (const group of categoryGroups) {
		groups[group.id] = valuesOf(group);
		for (const category of group.categories ?? []) {
			categories[category.id] = valuesOf(category);
		}
	}
	return { ...totals, groups, categories };
};

export const budgetMonthsDump = async (api: any): Promise<CanonicalObject> => {
	const months: CanonicalObject = {};
	for (const month of await api.getBudgetMonths()) {
		months[month] = dumpMonth(await api.getBudgetMonth(month));
	}
	return months;
};

export const computedDump = async ({ api, internal, today }: ActualSession): Promise<CanonicalObject> => {
	const budgetType = (await internal.db.first("SELECT value FROM preferences WHERE id = 'budgetType'"))?.value ?? 'envelope';
	const months = await budgetMonthsDump(api);

	const accounts: CanonicalObject = {};
	for (const account of await api.getAccounts()) {
		accounts[account.id] = { balance: await api.getAccountBalance(account.id) };
	}

	const schedules: CanonicalObject = {};
	for (const schedule of await api.getSchedules()) {
		schedules[schedule.id] = {
			completed: Boolean(schedule.completed),
			nextDate: schedule.next_date ?? null,
			upcoming: await upcomingDates(api, schedule, today)
		};
	}

	return { today, budgetType, months, accounts, schedules };
};
