import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import type { CanonicalValue } from './canonicalJson.ts';
import type { ActualSession } from './environment.ts';
import { loadSource, referenceDir } from './reference.ts';

export interface BillFactsRequest {
	// Schedule ids whose linked transactions to list.
	linked?: string[];
	// Dates or RecurConfigs to preview, as the schedule editor does.
	upcoming?: { config: unknown; count: number }[];
}

// What Actual itself says about bills: each schedule's linked transactions in
// its default transaction order (aql/schema/index.ts:278-285), split lines
// left out; every schedule as `v_schedules` reads it; and the dates
// `schedule/get-upcoming-dates` previews (schedules/app.ts:513-532), or
// "Invalid rule" where the editor shows that.
export const billFacts = async ({ api, internal }: ActualSession, request: BillFactsRequest) => {
	const linked: Record<string, CanonicalValue> = {};
	for (const id of request.linked ?? []) {
		const { data } = (await api.aqlQuery(
			api.q('transactions').filter({ schedule: id }).options({ splits: 'none' }).select(['id', 'date', 'amount', 'account'])
		)) as { data: CanonicalValue[] };
		linked[id] = data;
	}
	const upcoming: CanonicalValue[] = [];
	for (const { config, count } of request.upcoming ?? []) {
		try {
			upcoming.push(await internal.send('schedule/get-upcoming-dates', { config, count }));
		} catch {
			upcoming.push('Invalid rule');
		}
	}
	const { data: schedules } = (await api.aqlQuery(
		api.q('schedules').select(['id', 'name', '_amountOp', '_account', '_actions', 'next_date', 'completed'])
	)) as { data: CanonicalValue[] };
	return { linked, upcoming, schedules };
};

const strings = (text: string) => [...text.matchAll(/'([^']+)'/g)].map(match => match[1]);

const between = (text: string, start: string, end: string) => {
	const from = text.indexOf(start);
	const to = text.indexOf(end, from + start.length);
	if (from < 0 || to < 0) {
		throw new Error(`Actual's source no longer has ${JSON.stringify(start)} … ${JSON.stringify(end)}`);
	}
	return text.slice(from + start.length, to);
};

// The rule editor's choices: `getValidOps` for every field, run from Actual's
// own shared/rules.ts, and the lists RuleEditor.tsx and util/rule.ts hold,
// read from their text since those modules need React to load.
export const ruleChoices = async () => {
	const rules = await loadSource('packages/loot-core/src/shared/rules.ts');
	const validOps: Record<string, CanonicalValue> = {};
	for (const field of rules.FIELD_TYPES.keys()) {
		validOps[field] = rules.getValidOps(field);
	}
	const dir = referenceDir();
	const editor = readFileSync(join(dir, 'packages/desktop-client/src/components/rules/RuleEditor.tsx'), 'utf8');
	const util = readFileSync(join(dir, 'packages/desktop-client/src/util/rule.ts'), 'utf8');
	const conditions = between(editor, 'const conditionFields = [', 'type RuleEditorProps');
	const concat = conditions.slice(conditions.indexOf('.concat('));
	const allocation = between(util, 'export function getAllocationMethods', '\n}');
	return {
		fieldTypes: Object.fromEntries(rules.FIELD_TYPES),
		validOps,
		conditionFields: [
			...strings(conditions.slice(0, conditions.indexOf(']'))),
			...[...concat.matchAll(/\['([^']+)'/g)].map(match => match[1])
		],
		inflowOutflowDropIsBetween: editor.includes("ops = ops.filter(op => op !== 'isbetween')"),
		actionOps: strings(between(editor, "op === 'set' ? (", '/>').match(/ops=\{\[([^\]]*)\]/)?.[1] ?? ''),
		actionFields: strings(between(editor, 'function getActionFields() {', '].map')),
		parentOnlyFields: strings(between(editor, 'const parentOnlyFields = [', ']')),
		hidesNameFields: editor.includes("!s.includes('_name')"),
		allocationMethods: allocation
			.split('\n')
			.filter(line => !line.includes('hasFormulaMode') && /:\s*t\(/.test(line))
			.map(line => line.trim().split(':')[0].replaceAll("'", ''))
	};
};
