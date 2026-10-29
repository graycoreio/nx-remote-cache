/**
 * `nx-remote-cache status`: whether this workspace's remote cache works, checked the way Nx uses
 * it, by writing and reading an entry through the server.
 */
import { spawnSync } from 'child_process';
import * as fs from 'fs';
import * as path from 'path';
import * as pc from 'picocolors';

import { readState } from './server';
import { Settings } from './settings';

/** What the plugin's hook did last, so `status` can say why the cache is off. */
export interface LastRun {
    at: string;
    ok: boolean;
    message: string;
    settings?: Settings;
}

export function writeLastRun(stateDir: string, run: Omit<LastRun, 'at'>): void {
    try {
        fs.mkdirSync(stateDir, { recursive: true, mode: 0o700 });
        fs.writeFileSync(path.join(stateDir, 'last-run.json'), JSON.stringify({ at: new Date().toISOString(), ...run }));
    } catch {
        // Only for `status`; never worth failing Nx over.
    }
}

export function readLastRun(stateDir: string): LastRun | undefined {
    try {
        return JSON.parse(fs.readFileSync(path.join(stateDir, 'last-run.json'), 'utf8'));
    } catch {
        return undefined;
    }
}

export type Outcome = 'ok' | 'neutral' | 'warn' | 'fail';

export interface Check {
    outcome: Outcome;
    detail: string;
}

/**
 * The entry the check writes. Always the same, so checks don't fill the container: once it
 * exists, a write answers 409, which storage only gets to after authorizing the write.
 */
export const PROBE_HASH = 'nx-remote-cache-status-check';

/** Writes then reads the probe entry, as Nx would. */
export async function probe(
    url: string,
    token: string | undefined,
    settings: Settings | undefined,
    fetchImpl: typeof fetch = fetch,
): Promise<{ write: Check; read: Check }> {
    const entry = `${url}/v1/cache/${PROBE_HASH}`;
    const auth: Record<string, string> = token ? { authorization: `Bearer ${token}` } : {};
    const send = async (init: RequestInit) => {
        try {
            const res = await fetchImpl(entry, { ...init, headers: { ...auth, ...init.headers }, signal: AbortSignal.timeout(30_000) });
            return { status: res.status, text: (await res.text()).trim() };
        } catch (e) {
            return { status: 0, text: `no answer from ${url}: ${e instanceof Error ? e.message : e}` };
        }
    };

    const put = await send({ method: 'PUT', body: 'ok', headers: { 'content-type': 'application/octet-stream' } });
    let write: Check;
    if (put.status === 200 || put.status === 409) {
        write = { outcome: 'ok', detail: 'ok' };
    } else if (put.status === 403 && settings?.mode === 'github' && !settings.write) {
        write = { outcome: 'neutral', detail: 'read-only (reader identity)' };
    } else if (put.status === 403) {
        write = { outcome: 'warn', detail: `read-only: ${put.text}` };
    } else {
        write = { outcome: 'fail', detail: failure(put) };
    }

    const get = await send({ method: 'GET' });
    // 404 only when the write failed too: reading worked, there was just nothing to read.
    const read: Check = get.status === 200 || get.status === 404 ? { outcome: 'ok', detail: 'ok' } : { outcome: 'fail', detail: failure(get) };
    return { write, read };
}

function failure(res: { status: number; text: string }): string {
    if (res.status === 0) return res.text;
    if (res.status === 401) return 'the server rejected the access token';
    return `HTTP ${res.status}${res.text ? `: ${res.text}` : ''}`;
}

export interface StatusDeps {
    stateDir: string;
    env: NodeJS.ProcessEnv;
    fetchImpl?: typeof fetch;
    /** The signed-in Azure CLI account; undefined when signed out. */
    azAccount?: () => string | undefined;
}

/** Checks the cache and renders the result. `ok` is whether Nx can read from it. */
export async function status(deps: StatusDeps): Promise<{ ok: boolean; text: string }> {
    const { stateDir, env } = deps;
    const last = readLastRun(stateDir);
    const settings = last?.settings;
    // Inside an Nx task, where the hook just ran, Nx passes the server it set up; outside, the
    // server the last Nx command started.
    const running = readState(stateDir);
    const url = env.NX_SELF_HOSTED_REMOTE_CACHE_SERVER ?? running?.url;
    const token = env.NX_SELF_HOSTED_REMOTE_CACHE_SERVER ? env.NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN : running?.token;

    const rows: [string, string][] = [];
    // A running server keeps the token it got from `az login` until it expires, so it still
    // works for a while after `az logout`.
    let signedOut = false;
    if (settings) {
        rows.push(['storage', [settings.account, settings.container, settings.prefix ?? 'v1'].join('/')]);
        if (settings.mode === 'github') {
            rows.push(['identity', `GitHub OIDC → ${settings.write ? 'writer' : 'reader'} identity ${pc.dim(settings.clientId)}`]);
        } else {
            const who = (deps.azAccount ?? azAccount)();
            signedOut = !who;
            rows.push(['identity', who ? `az login · ${who}` : mark({ outcome: 'warn', detail: 'az login: signed out' })]);
        }
    }

    if (!url || (last && !last.ok && !env.NX_SELF_HOSTED_REMOTE_CACHE_SERVER)) {
        const reason = last && !last.ok ? last.message : 'no server is running; run any Nx command to start one';
        const [first, ...rest] = reason.split('\n');
        rows.push(['reason', first]);
        return { ok: false, text: render(`${pc.red('✖')} off`, rows, rest) };
    }

    rows.push(['server', url + (running && running.url === url ? pc.dim(` (pid ${running.pid})`) : '')]);
    const started = Date.now();
    const { read, write } = await probe(url, token, settings, deps.fetchImpl);
    const ms = Date.now() - started;
    rows.push(['read', mark(read)], ['write', mark(write)]);
    const ok = read.outcome === 'ok';
    if (!ok) {
        return { ok, text: render(`${pc.red('✖')} not working`, rows, []) };
    }
    if (signedOut) {
        const note = [
            "The server still uses the token from your last az login until it expires (up to about an hour).",
            'Run `nx @graycore/nx-remote-cache:stop` to stop it now; the next Nx command then runs without the cache.',
        ];
        return { ok, text: render(`${pc.yellow('!')} connected, but signed out ${pc.dim(`(${ms} ms)`)}`, rows, note) };
    }
    return { ok, text: render(`${pc.green('✔')} connected ${pc.dim(`(${ms} ms)`)}`, rows, []) };
}

function azAccount(): string | undefined {
    const az = spawnSync('az', ['account', 'show', '--query', 'user.name', '-o', 'tsv'], { encoding: 'utf8', timeout: 10_000 });
    return az.status === 0 ? az.stdout.trim() || undefined : undefined;
}

const MARKS: Record<Outcome, (s: string) => string> = {
    ok: (s) => `${pc.green('✔')} ${s}`,
    neutral: (s) => `${pc.dim('–')} ${s}`,
    warn: (s) => `${pc.yellow('!')} ${pc.yellow(s)}`,
    fail: (s) => `${pc.red('✖')} ${pc.red(s)}`,
};

function mark(check: Check): string {
    return MARKS[check.outcome](check.detail);
}

function render(headline: string, rows: [string, string][], extra: string[]): string {
    const width = Math.max(...rows.map(([label]) => label.length));
    return [
        `${pc.bold('nx-remote-cache')}  ${headline}`,
        ...rows.map(([label, value]) => `  ${pc.dim(label.padEnd(width))}  ${value}`),
        ...extra.map((line) => `  ${pc.dim(line)}`),
        '',
    ].join('\n');
}
