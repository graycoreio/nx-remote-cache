import { strict as assert } from 'assert';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { afterEach, beforeEach, describe, it } from 'node:test';

import { createNodes, preTasksExecution, STATUS_TARGET, STOP_TARGET } from './index';
import { Settings } from './settings';
import { probe, PROBE_HASH, readLastRun, status, writeLastRun } from './status';

const local: Settings = { mode: 'developer_tools', account: 'acct', container: 'nx-cache-v1' };
const reader: Settings = { mode: 'github', account: 'acct', container: 'nx-cache-v1', tenantId: 'tenant', clientId: 'reader', write: false };

/** A fake server answering PUT and GET on the probe entry with these statuses. */
function serverAnswering(put: number, get: number, text = '') {
    const requests: { method: string; url: string; auth: string | null }[] = [];
    const fetchImpl = (async (url: string, init: RequestInit) => {
        const method = init.method ?? 'GET';
        requests.push({ method, url, auth: new Headers(init.headers).get('authorization') });
        return new Response(method === 'PUT' ? text : null, { status: method === 'PUT' ? put : get });
    }) as typeof fetch;
    return { requests, fetchImpl };
}

describe('probe', () => {
    it('writes then reads the probe entry with the access token', async () => {
        const s = serverAnswering(200, 200);
        const result = await probe('http://127.0.0.1:1', 'tok', local, s.fetchImpl);
        assert.deepEqual(result, { write: { outcome: 'ok', detail: 'ok' }, read: { outcome: 'ok', detail: 'ok' } });
        assert.deepEqual(s.requests, [
            { method: 'PUT', url: `http://127.0.0.1:1/v1/cache/${PROBE_HASH}`, auth: 'Bearer tok' },
            { method: 'GET', url: `http://127.0.0.1:1/v1/cache/${PROBE_HASH}`, auth: 'Bearer tok' },
        ]);
    });

    it('counts an entry left by an earlier check as a working write', async () => {
        assert.equal((await probe('http://x', 'tok', local, serverAnswering(409, 200).fetchImpl)).write.outcome, 'ok');
    });

    it('expects a CI reader to be read-only, but warns a developer who is', async () => {
        assert.equal((await probe('http://x', 'tok', reader, serverAnswering(403, 404).fetchImpl)).write.outcome, 'neutral');
        const dev = await probe('http://x', 'tok', local, serverAnswering(403, 404, "this server's storage identity can't write to the cache").fetchImpl);
        assert.deepEqual(dev.write, { outcome: 'warn', detail: "read-only: this server's storage identity can't write to the cache" });
        assert.equal(dev.read.outcome, 'ok');
    });

    it('fails on a rejected token or no answer', async () => {
        const rejected = await probe('http://x', 'bad', local, serverAnswering(401, 401).fetchImpl);
        assert.deepEqual(rejected.read, { outcome: 'fail', detail: 'the server rejected the access token' });
        const down = await probe('http://x', 'tok', local, (async () => {
            throw new Error('ECONNREFUSED');
        }) as typeof fetch);
        assert.match(down.read.detail, /no answer from http:\/\/x: ECONNREFUSED/);
    });
});

describe('status', () => {
    let stateDir: string;
    beforeEach(() => {
        stateDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nx-remote-cache-test-'));
    });
    afterEach(() => fs.rmSync(stateDir, { recursive: true, force: true }));

    const env = { NX_SELF_HOSTED_REMOTE_CACHE_SERVER: 'http://127.0.0.1:1', NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN: 'tok' };
    const plain = (text: string) => text.replace(/\x1b\[[0-9;]*m/g, '');

    it('checks the server Nx passes to the task', async () => {
        writeLastRun(stateDir, { ok: true, message: 'connected', settings: local });
        const result = await status({ stateDir, env, fetchImpl: serverAnswering(200, 200).fetchImpl, azAccount: () => 'me@example.com' });
        assert.equal(result.ok, true);
        assert.match(plain(result.text), /connected/);
        assert.match(plain(result.text), /storage +acct\/nx-cache-v1\/v1/);
        assert.match(plain(result.text), /identity +az login · me@example\.com/);
    });

    it('warns when the server outlives an az logout', async () => {
        writeLastRun(stateDir, { ok: true, message: 'connected', settings: local });
        const result = await status({ stateDir, env, fetchImpl: serverAnswering(409, 200).fetchImpl, azAccount: () => undefined });
        assert.equal(result.ok, true);
        assert.match(plain(result.text), /connected, but signed out/);
        assert.match(plain(result.text), /identity +! az login: signed out/);
        assert.match(plain(result.text), /nx @graycore\/nx-remote-cache:stop/);
    });

    it("says why the cache is off, from the hook's last run", async () => {
        writeLastRun(stateDir, { ok: false, message: 'the server did not start; its log:\nplease run az login', settings: local });
        const result = await status({ stateDir, env: {}, fetchImpl: serverAnswering(200, 200).fetchImpl, azAccount: () => undefined });
        assert.equal(result.ok, false);
        assert.match(plain(result.text), /off\n[\s\S]*reason +the server did not start; its log:\n +please run az login/);
    });

    it('says so when no server was ever started', async () => {
        const result = await status({ stateDir, env: {} });
        assert.equal(result.ok, false);
        assert.match(plain(result.text), /no server is running/);
    });

    it('fails when Nx could not read through the server', async () => {
        writeLastRun(stateDir, { ok: true, message: 'connected', settings: local });
        const result = await status({ stateDir, env, fetchImpl: serverAnswering(500, 500).fetchImpl, azAccount: () => undefined });
        assert.equal(result.ok, false);
        assert.match(plain(result.text), /read +✖ HTTP 500/);
    });

    it('records the last run with when it happened', () => {
        writeLastRun(stateDir, { ok: false, message: 'off' });
        const last = readLastRun(stateDir);
        assert.equal(last?.message, 'off');
        assert.ok(!Number.isNaN(Date.parse(last!.at)));
    });
});

describe('createNodes', () => {
    it('adds an uncached status target to the root project, named as Nx names it', () => {
        const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nx-remote-cache-test-'));
        try {
            fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({ name: '@acme/source' }));
            const [[file, result]] = createNodes[1](['nx.json'], undefined, { workspaceRoot: root });
            assert.equal(file, 'nx.json');
            const project = result.projects['.'];
            assert.equal(project.name, '@acme/source');
            const { command, ...target } = project.targets[STATUS_TARGET];
            assert.deepEqual(target, {
                cache: false,
                metadata: { description: 'Checks that the remote cache works, or says why it is off.' },
            });
            // Runs this package's CLI by its path from the workspace root, where Nx runs commands.
            const [, cli] = command.match(/^node "(.+)" status$/)!;
            assert.equal(path.resolve(root, cli), path.join(__dirname, 'cli.js'));
            assert.equal(project.targets[STOP_TARGET].command, `node "${cli}" stop`);
            assert.equal(project.targets[STOP_TARGET].cache, false);
        } finally {
            fs.rmSync(root, { recursive: true, force: true });
        }
    });
});

describe('preTasksExecution', () => {
    it("doesn't start a server for the stop target", async () => {
        const root = fs.mkdtempSync(path.join(os.tmpdir(), 'nx-remote-cache-test-'));
        try {
            await preTasksExecution({ storageAccount: 'acct', storageContainer: 'c' }, { workspaceRoot: root, argv: ['nx', STOP_TARGET] });
            // It would have recorded the attempt, successful or not.
            assert.equal(fs.existsSync(path.join(root, '.nx', 'gc-remote-cache', 'last-run.json')), false);
        } finally {
            fs.rmSync(root, { recursive: true, force: true });
        }
    });
});
