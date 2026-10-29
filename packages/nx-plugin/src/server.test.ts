import { strict as assert } from 'assert';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { afterEach, beforeEach, describe, it } from 'node:test';

import { ensureServer, Launch, serverConfigToml, ServerDeps, ServerNotReady, stopServer } from './server';
import { Settings } from './settings';

const local: Settings = { mode: 'developer_tools', account: 'acct', container: 'nx-cache-v1' };
const reader: Settings = { mode: 'github', account: 'acct', container: 'nx-cache-v1', prefix: 'v1', tenantId: 'tenant', clientId: 'reader', write: false };

describe('serverConfigToml', () => {
    it('uses workload identity and a read-only token for a CI reader', () => {
        assert.equal(
            serverConfigToml(reader, { port: 40123, tokenSha256: 'abc', tokenName: 'ci' }),
            [
                '[server]',
                'listen = "127.0.0.1:40123"',
                '[storage]',
                'type = "azure"',
                'account = "acct"',
                'container = "nx-cache-v1"',
                'prefix = "v1"',
                'auth = { mode = "workload_identity", tenant_id = "tenant", client_id = "reader" }',
                '[[auth.providers]]',
                'type = "static"',
                'tokens = [{ name = "ci", sha256 = "abc", scope = "read" }]',
                '',
            ].join('\n'),
        );
    });

    it("uses the developer's az login, leaving what they may write to their role", () => {
        const toml = serverConfigToml(local, { port: 1, tokenSha256: 'abc', tokenName: 'dev-me' });
        assert.match(toml, /auth = \{ mode = "developer_tools" \}/);
        assert.match(toml, /scope = "read_write"/);
        assert.doesNotMatch(toml, /prefix/);
    });

    it('escapes values', () => {
        assert.match(serverConfigToml({ ...local, container: 'a"b\nc' }, { port: 1, tokenSha256: 'x', tokenName: 'n' }), /container = "a\\"b\\nc"/);
    });
});

describe('ensureServer', () => {
    let stateDir: string;
    beforeEach(() => {
        stateDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nx-remote-cache-test-'));
    });
    afterEach(() => fs.rmSync(stateDir, { recursive: true, force: true }));

    /** A fake world: servers answer /health on the ports they were started on, until killed. */
    function world() {
        const live = new Map<number, string>(); // pid → url
        const started: { binary: string; cwd: string; env: NodeJS.ProcessEnv }[] = [];
        const killed: number[] = [];
        let nextPid = 100;
        let nextPort = 40000;
        const launch: Launch = (binary, _args, o) => {
            const pid = nextPid++;
            const port = Number(/127\.0\.0\.1:(\d+)/.exec(o.env.NX_CACHE_CONFIG_TOML!)![1]);
            live.set(pid, `http://127.0.0.1:${port}`);
            started.push({ binary, cwd: o.cwd, env: o.env });
            return { pid, exited: () => !live.has(pid) };
        };
        const kill = (pid: number) => {
            killed.push(pid);
            live.delete(pid);
        };
        const fetchImpl = (async (url: string) => {
            const up = [...live.values()].some((u) => url === `${u}/health`);
            if (!up) throw new Error('ECONNREFUSED');
            return new Response('OK');
        }) as unknown as typeof fetch;
        const deps = (o: Partial<ServerDeps> = {}): ServerDeps => ({
            stateDir,
            binary: '/bin/nx-remote-cache',
            env: { PATH: '/usr/bin', NX_CACHE__STORAGE__CONTAINER: 'elsewhere', ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'secret' },
            launch,
            kill,
            fetchImpl,
            freePort: async () => nextPort++,
            sleep: async () => undefined,
            ...o,
        });
        return { live, started, killed, deps, launch, kill, fetchImpl };
    }

    it('starts the binary with its config and only the environment it needs', async () => {
        const w = world();
        const server = await ensureServer(local, w.deps());
        assert.equal(server.url, 'http://127.0.0.1:40000');
        assert.match(server.token, /^[0-9a-f]{64}$/);
        const [run] = w.started;
        assert.equal(run.binary, '/bin/nx-remote-cache');
        assert.equal(run.cwd, stateDir);
        assert.deepEqual(Object.keys(run.env).sort(), ['NO_COLOR', 'NX_CACHE_CONFIG_TOML', 'PATH']);
        // The server only sees the token's hash.
        assert.doesNotMatch(run.env.NX_CACHE_CONFIG_TOML!, new RegExp(server.token));
        assert.equal(fs.statSync(path.join(stateDir, 'state.json')).mode & 0o777, 0o600);
    });

    it('reuses a running server started the same way', async () => {
        const w = world();
        const first = await ensureServer(local, w.deps());
        const second = await ensureServer(local, w.deps());
        assert.deepEqual(second, first);
        assert.equal(w.started.length, 1);
    });

    it('replaces a server started with other settings, or one that is gone', async () => {
        const w = world();
        const first = await ensureServer(local, w.deps());
        const other = await ensureServer({ ...local, container: 'other' }, w.deps());
        assert.deepEqual(w.killed, [first.pid]);
        assert.notEqual(other.url, first.url);

        w.live.clear();
        await ensureServer({ ...local, container: 'other' }, w.deps());
        // A pid whose server no longer answers may belong to another process by now.
        assert.deepEqual(w.killed, [first.pid]);
        assert.equal(w.started.length, 3);
    });

    it('gives a CI server a fresh OIDC token file on every run', async () => {
        const w = world();
        await ensureServer(reader, w.deps({ oidcToken: 'jwt-1' }));
        await ensureServer(reader, w.deps({ oidcToken: 'jwt-2' }));
        const tokenFile = w.started[0].env.AZURE_FEDERATED_TOKEN_FILE!;
        assert.equal(tokenFile, path.join(stateDir, 'github-oidc-token'));
        assert.equal(fs.readFileSync(tokenFile, 'utf8'), 'jwt-2');
        assert.equal(fs.statSync(tokenFile).mode & 0o777, 0o600);
        assert.equal(w.started.length, 1);
    });

    it('fails with the server log when the server exits', async () => {
        const w = world();
        const exits: Launch = (binary, args, o) => {
            fs.writeFileSync(o.logFile, 'ERROR storage is not usable: lacks role\n');
            return { pid: 7, exited: () => true };
        };
        await assert.rejects(ensureServer(local, w.deps({ launch: exits })), (e: Error) => e instanceof ServerNotReady && /lacks role/.test(e.message));
        assert.deepEqual(w.killed, []);
    });

    it('stops a server that never becomes ready', async () => {
        const w = world();
        const silent: Launch = () => ({ pid: 9, exited: () => false });
        await assert.rejects(ensureServer(local, w.deps({ launch: silent, timeoutMs: 5 })), ServerNotReady);
        assert.deepEqual(w.killed, [9]);
    });

    it('stopServer stops the running server and forgets it', async () => {
        const w = world();
        const server = await ensureServer(local, w.deps());
        assert.equal(await stopServer(stateDir, w.kill, w.fetchImpl), true);
        assert.deepEqual(w.killed, [server.pid]);
        assert.equal(await stopServer(stateDir, w.kill, w.fetchImpl), false);
    });
});
