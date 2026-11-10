import assert from 'node:assert/strict';
import { test } from 'node:test';
import { claimedRequirements, evaluate, trustedSignoffs } from './mergeBar.ts';

const head = '3f2a9c1d4e5b6a7980c1d2e3f4a5b6c7d8e9f0a1';
const pr = (overrides = {}) => ({
	baseRef: 'dev',
	headSha: head,
	body: '> [!NOTE]\n> This content was created by an AI agent acting on behalf of Ryan.\n\nClaims: R-IO-1, R-IO-3\n',
	labels: [],
	...overrides
});
const by = (body: string, authorAssociation = 'OWNER') => ({ body, authorAssociation });

test('a fresh PR waits for the reviewer and for cowboy coverage of every claim', () => {
	assert.deepEqual(evaluate(pr(), []), {
		state: 'pending',
		description: 'Needs reviewer, cowboy (R-IO-1, R-IO-3) on 3f2a9c1'
	});
});

test('reviewer plus a cowboy covering every claimed id passes', () => {
	const signoffs = trustedSignoffs([
		by('Looks right against importBuffer.\n\nmerge-bar: reviewer approve 3f2a9c1'),
		by('Round-trip tests added.\nmerge-bar: cowboy approve 3f2a9c1d R-IO-1 R-IO-3')
	]);
	assert.equal(evaluate(pr(), signoffs).state, 'success');
});

test('a cowboy who leaves a claimed id uncovered keeps the bar pending', () => {
	const signoffs = trustedSignoffs([
		by('merge-bar: reviewer approve 3f2a9c1'),
		by('merge-bar: cowboy approve 3f2a9c1 R-IO-1')
	]);
	assert.equal(evaluate(pr(), signoffs).description, 'Needs cowboy (R-IO-3) on 3f2a9c1');
});

test('a new push invalidates sign-offs given on the previous commit', () => {
	const signoffs = trustedSignoffs([
		by('merge-bar: reviewer approve aaaaaaa'),
		by('merge-bar: cowboy approve aaaaaaa R-IO-1 R-IO-3')
	]);
	assert.equal(evaluate(pr(), signoffs).description, 'Needs reviewer, cowboy (R-IO-1, R-IO-3) on 3f2a9c1');
});

test('a later rejection on the same commit withdraws the approval', () => {
	const signoffs = trustedSignoffs([
		by('merge-bar: reviewer approve 3f2a9c1'),
		by('merge-bar: cowboy approve 3f2a9c1 R-IO-1 R-IO-3'),
		by('Found a regression.\nmerge-bar: reviewer reject 3f2a9c1')
	]);
	assert.equal(evaluate(pr(), signoffs).description, 'Needs reviewer on 3f2a9c1');
});

test('strangers on the public repo cannot sign off', () => {
	const signoffs = trustedSignoffs([
		by('merge-bar: reviewer approve 3f2a9c1', 'NONE'),
		by('merge-bar: reviewer approve 3f2a9c1', 'CONTRIBUTOR')
	]);
	assert.deepEqual(signoffs, []);
});

test('finance and contract labels require Fred and the Architect', () => {
	const docsOnly = pr({ body: 'Claims: none', labels: ['finance', 'contract'] });
	const reviewed = trustedSignoffs([by('merge-bar: reviewer approve 3f2a9c1')]);
	assert.equal(evaluate(docsOnly, reviewed).description, 'Needs fred, architect on 3f2a9c1');

	const all = trustedSignoffs([
		by('merge-bar: reviewer approve 3f2a9c1'),
		by('merge-bar: fred approve 3f2a9c1'),
		by('merge-bar: architect approve 3f2a9c1')
	]);
	assert.equal(evaluate(docsOnly, all).state, 'success');
});

test('a PR with no claims needs no cowboy', () => {
	const signoffs = trustedSignoffs([by('merge-bar: reviewer approve 3f2a9c1')]);
	assert.equal(evaluate(pr({ body: 'Docs only.\r\nClaims: none\r\n' }), signoffs).state, 'success');
});

test('gate PRs into main are left to Ryan', () => {
	assert.equal(evaluate(pr({ baseRef: 'main' }), []).state, 'success');
});

test('claims are read from the Claims line only, deduplicated', () => {
	const body = 'Fixes the R-BUD-9 typo.\nClaims: R-REL-2, R-IO-7, R-REL-2\nSee R-IO-1 later.';
	assert.deepEqual(claimedRequirements(body), ['R-IO-7', 'R-REL-2']);
});
