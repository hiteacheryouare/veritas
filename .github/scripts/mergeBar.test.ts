import assert from 'node:assert/strict';
import { test } from 'node:test';
import { claimedRequirements, evaluate, reviewedCommits, trustedSignoffs } from './mergeBar.ts';

const head = '3f2a9c1d4e5b6a7980c1d2e3f4a5b6c7d8e9f0a1';
const pr = (overrides = {}) => ({
	baseRef: 'dev',
	headRef: 'ws/p0-oracle',
	headSha: head,
	reviewedShas: [head],
	fromFork: false,
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

test('a contract PR the Architect wrote needs only the reviewer (D42)', () => {
	const reviewed = trustedSignoffs([by('merge-bar: reviewer approve 3f2a9c1')]);
	const ownContract = pr({ body: 'Claims: none', labels: ['contract', 'architect-authored'] });
	assert.equal(evaluate(ownContract, reviewed).state, 'success');
	assert.equal(evaluate(ownContract, []).description, 'Needs reviewer on 3f2a9c1');
	assert.equal(evaluate({ ...ownContract, labels: ['contract'] }, reviewed).description, 'Needs architect on 3f2a9c1');
});

test('a PR with no claims needs no cowboy', () => {
	const signoffs = trustedSignoffs([by('merge-bar: reviewer approve 3f2a9c1')]);
	assert.equal(evaluate(pr({ body: 'Docs only.\r\nClaims: none\r\n' }), signoffs).state, 'success');
});

test('a clean catch-up merge from dev keeps the sign-offs; anything else drops them', () => {
	const reviewed = { sha: 'aaaaaaa1', parents: ['0000000'] };
	const syncDev = { sha: 'bbbbbbb2', parents: ['aaaaaaa1', 'devtip01'] };
	const newWork = { sha: 'ccccccc3', parents: ['bbbbbbb2'] };
	const editedMerge = { sha: 'ddddddd4', parents: ['aaaaaaa1', 'devtip01'] };
	const commits = [reviewed, syncDev, newWork, editedMerge];
	const clean = (commit: { sha: string }) => commit.sha === 'bbbbbbb2';

	assert.deepEqual(reviewedCommits('bbbbbbb2', commits, clean), ['bbbbbbb2', 'aaaaaaa1']);
	assert.deepEqual(reviewedCommits('ccccccc3', commits, clean), ['ccccccc3']);
	assert.deepEqual(reviewedCommits('ddddddd4', commits, clean), ['ddddddd4']);
	assert.deepEqual(reviewedCommits('notlisted', commits, clean), ['notlisted']);

	const signoffs = trustedSignoffs([
		by('merge-bar: reviewer approve aaaaaaa'),
		by('merge-bar: cowboy approve aaaaaaa R-IO-1 R-IO-3')
	]);
	assert.equal(evaluate(pr({ headSha: 'bbbbbbb2', reviewedShas: ['bbbbbbb2', 'aaaaaaa1'] }), signoffs).state, 'success');
	assert.equal(evaluate(pr({ headSha: 'ddddddd4', reviewedShas: ['ddddddd4'] }), signoffs).state, 'pending');
});

test('a sign-off on an earlier catch-up merge survives a second catch-up merge', () => {
	const work = { sha: 'aaaaaaa1', parents: ['0000000'] };
	const firstSync = { sha: 'bbbbbbb2', parents: ['aaaaaaa1', 'devtip01'] };
	const secondSync = { sha: 'ccccccc3', parents: ['bbbbbbb2', 'devtip02'] };
	const chain = reviewedCommits('ccccccc3', [work, firstSync, secondSync], () => true);
	assert.deepEqual(chain, ['ccccccc3', 'bbbbbbb2', 'aaaaaaa1']);
	const signoffs = trustedSignoffs([by('merge-bar: reviewer approve bbbbbbb'), by('merge-bar: cowboy approve aaaaaaa R-IO-1 R-IO-3')]);
	assert.equal(evaluate(pr({ headSha: 'ccccccc3', reviewedShas: chain }), signoffs).state, 'success');
});

test('a fork branch named dev cannot pass as a gate PR', () => {
	assert.equal(evaluate(pr({ baseRef: 'main', headRef: 'dev', fromFork: true }), []).state, 'pending');
});

test('gate PRs from dev into main are left to Ryan, and nothing else may target main', () => {
	assert.equal(evaluate(pr({ baseRef: 'main', headRef: 'dev' }), []).state, 'success');
	assert.deepEqual(evaluate(pr({ baseRef: 'main' }), []), { state: 'pending', description: 'Only dev merges into main' });
});

test('the untouched PR template claims nothing', async () => {
	const { readFile } = await import('node:fs/promises');
	const template = await readFile(new URL('../PULL_REQUEST_TEMPLATE.md', import.meta.url), 'utf8');
	assert.deepEqual(claimedRequirements(template), []);
	assert.deepEqual(trustedSignoffs([by(template)]), []);
});

test('a sign-off quoted in a code block or HTML comment does not count', () => {
	const quoted = '```\nmerge-bar: reviewer approve 3f2a9c1\n```\n<!-- merge-bar: reviewer approve 3f2a9c1 -->';
	assert.deepEqual(trustedSignoffs([by(quoted)]), []);
});

test('claims are read from the Claims line only, deduplicated', () => {
	const body = 'Fixes the R-BUD-9 typo.\nClaims: R-REL-2, R-IO-7, R-REL-2\nSee R-IO-1 later.';
	assert.deepEqual(claimedRequirements(body), ['R-IO-7', 'R-REL-2']);
});
