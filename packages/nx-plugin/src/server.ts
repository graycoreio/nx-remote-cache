import { spawn } from 'child_process';
import { createHash, randomBytes } from 'crypto';
import * as fs from 'fs';
import * as net from 'net';
import * as os from 'os';
import * as path from 'path';

import { Settings } from './settings';

export interface Server {
    pid: number;
    url: string;
    /** What Nx presents to the server. Made up per server; the server only knows its hash. */
    token: string;
    /** Identifies the binary and settings the server was started with. */
    key: string;
}

export interface Launched {
    pid: number;
    exited: () => boolean;
}

export type Launch = (binary: string, args: string[], o: { cwd: string; env: NodeJS.ProcessEnv; logFile: string }) => Launched;

export interface ServerDeps {
    stateDir: string;
    binary: string;
    /** The environment Nx runs in; the server gets only the parts in PASSED_ENV. */
    env: NodeJS.ProcessEnv;
    /** The job's GitHub OIDC token, for `github` settings. */
    oidcToken?: string;
    launch?: Launch;
    kill?: (pid: number) => void;
    fetchImpl?: typeof fetch;
    freePort?: () => Promise<number>;
    sleep?: (ms: number) => Promise<void>;
    timeoutMs?: number;
}

/**
 * What the server inherits: enough to run, to reach Azure (proxies, CA certificates) and, for
 * `developer_tools`, to run the Azure CLI. Not NX_CACHE__* overrides, which would change its
 * config, nor the job's other secrets.
 */
const PASSED_ENV = [
    'PATH', 'HOME', 'USER', 'TMPDIR', 'LANG',
    'AZURE_CONFIG_DIR',
    'HTTP_PROXY', 'HTTPS_PROXY', 'NO_PROXY', 'http_proxy', 'https_proxy', 'no_proxy',
    'SSL_CERT_FILE', 'SSL_CERT_DIR',
];

/**
 * Where the plugin keeps a workspace's server state: inside the workspace, so no other user or
 * workspace can plant or read it. `nx reset` removes only `.nx/cache` and `.nx/workspace-data`,
 * so it keeps this.
 */
export function stateDirFor(workspaceRoot: string): string {
    return path.join(workspaceRoot, '.nx', 'gc-remote-cache');
}

/** A TOML basic string. JSON string escaping is a subset of TOML's, so this is always valid. */
function tomlString(value: string): string {
    return JSON.stringify(value);
}

export function serverConfigToml(settings: Settings, o: { port: number; tokenSha256: string; tokenName: string }): string {
    const auth =
        settings.mode === 'github'
            ? `{ mode = "workload_identity", tenant_id = ${tomlString(settings.tenantId)}, client_id = ${tomlString(settings.clientId)} }`
            : '{ mode = "developer_tools" }';
    // A developer may write if their role allows it; storage refuses otherwise, and Nx skips the
    // upload. A CI job gets a read-only token with the reader identity, so its uploads are a clean
    // 403 rather than a storage error.
    const write = settings.mode === 'github' ? settings.write : true;
    return [
        '[server]',
        `listen = ${tomlString(`127.0.0.1:${o.port}`)}`,
        '[storage]',
        'type = "azure"',
        `account = ${tomlString(settings.account)}`,
        `container = ${tomlString(settings.container)}`,
        ...(settings.prefix !== undefined ? [`prefix = ${tomlString(settings.prefix)}`] : []),
        `auth = ${auth}`,
        '[[auth.providers]]',
        'type = "static"',
        `tokens = [{ name = ${tomlString(o.tokenName)}, sha256 = ${tomlString(o.tokenSha256)}, scope = ${tomlString(write ? 'read_write' : 'read')} }]`,
        '',
    ].join('\n');
}

/** The name entries written through this server record as their writer. */
export function tokenName(env: NodeJS.ProcessEnv): string {
    if (env.GITHUB_ACTIONS === 'true') {
        return `ci-${env.GITHUB_REPOSITORY ?? 'unknown'}-${env.GITHUB_RUN_ID ?? 'unknown'}`;
    }
    return `dev-${env.USER ?? os.userInfo().username}`;
}

function sha256Hex(value: string): string {
    return createHash('sha256').update(value).digest('hex');
}

/**
 * Returns a running server for these settings: the one started for an earlier Nx command if it
 * still answers and was started the same way, otherwise a new one. A server started with other
 * settings is stopped. Servers keep running after Nx exits, so later commands skip the startup.
 */
export async function ensureServer(settings: Settings, deps: ServerDeps): Promise<Server> {
    const { stateDir, binary, env, launch = launchDetached, kill = killServer, fetchImpl = fetch } = deps;
    fs.mkdirSync(stateDir, { recursive: true, mode: 0o700 });
    const tokenFile = path.join(stateDir, 'github-oidc-token');
    if (settings.mode === 'github') {
        // Rewritten for every Nx command, so a reused server exchanges a fresh token when its
        // Azure token expires.
        writePrivate(tokenFile, deps.oidcToken ?? '');
    }

    const key = sha256Hex(JSON.stringify({ binary, settings }));
    const previous = readState(stateDir);
    if (previous && (await healthy(previous.url, fetchImpl))) {
        if (previous.key === key) {
            return previous;
        }
        kill(previous.pid);
    }

    const token = randomBytes(32).toString('hex');
    const port = await (deps.freePort ?? findFreePort)();
    const serverEnv: NodeJS.ProcessEnv = {};
    for (const name of PASSED_ENV) {
        if (env[name] !== undefined) serverEnv[name] = env[name];
    }
    // Its log is a file, not a terminal.
    serverEnv.NO_COLOR = '1';
    serverEnv.NX_CACHE_CONFIG_TOML = serverConfigToml(settings, { port, tokenSha256: sha256Hex(token), tokenName: tokenName(env) });
    if (settings.mode === 'github') {
        serverEnv.AZURE_FEDERATED_TOKEN_FILE = tokenFile;
    }

    const logFile = path.join(stateDir, 'server.log');
    // The state dir as cwd, so a config.toml in the workspace can't leak into the config.
    const launched = launch(binary, ['serve'], { cwd: stateDir, env: serverEnv, logFile });
    const server: Server = { pid: launched.pid, url: `http://127.0.0.1:${port}`, token, key };

    const sleep = deps.sleep ?? ((ms: number) => new Promise((r) => setTimeout(r, ms)));
    const deadline = Date.now() + (deps.timeoutMs ?? 30_000);
    while (Date.now() < deadline && !launched.exited()) {
        // The server checks storage before it listens, so answering at all means it's usable.
        if (await healthy(server.url, fetchImpl)) {
            writePrivate(path.join(stateDir, 'state.json'), JSON.stringify(server));
            return server;
        }
        await sleep(250);
    }
    // It exits by itself when it can't use storage; on a timeout, stop it.
    if (!launched.exited()) kill(launched.pid);
    throw new ServerNotReady(readLog(logFile));
}

/** Stops the workspace's server, if one is running. Returns whether there was one. */
export async function stopServer(stateDir: string, kill = killServer, fetchImpl: typeof fetch = fetch): Promise<boolean> {
    const state = readState(stateDir);
    fs.rmSync(path.join(stateDir, 'state.json'), { force: true });
    // Only signal the pid while its server answers: otherwise it may be another process by now.
    if (state && (await healthy(state.url, fetchImpl))) {
        kill(state.pid);
        return true;
    }
    return false;
}

function readState(stateDir: string): Server | undefined {
    try {
        return JSON.parse(fs.readFileSync(path.join(stateDir, 'state.json'), 'utf8'));
    } catch {
        return undefined;
    }
}

function writePrivate(file: string, content: string): void {
    fs.writeFileSync(file, content, { mode: 0o600 });
    // writeFileSync only applies the mode when it creates the file.
    fs.chmodSync(file, 0o600);
}

async function healthy(url: string, fetchImpl: typeof fetch): Promise<boolean> {
    try {
        return (await fetchImpl(`${url}/health`, { signal: AbortSignal.timeout(2_000) })).ok;
    } catch {
        return false;
    }
}

/** Starts the server in its own process group, detached from Nx, logging to a file. */
const launchDetached: Launch = (binary, args, o) => {
    const log = fs.openSync(o.logFile, 'w', 0o600);
    try {
        const child = spawn(binary, args, { cwd: o.cwd, env: o.env, detached: true, stdio: ['ignore', log, log] });
        let exited = false;
        child.once('exit', () => (exited = true));
        child.once('error', () => (exited = true));
        child.unref();
        return { pid: child.pid ?? -1, exited: () => exited };
    } finally {
        fs.closeSync(log);
    }
};

function killServer(pid: number): void {
    // 0 and negative pids signal whole process groups.
    if (!(pid > 0)) return;
    try {
        process.kill(pid, 'SIGTERM');
    } catch {
        // already gone
    }
}

function findFreePort(): Promise<number> {
    return new Promise((resolve, reject) => {
        const probe = net.createServer();
        probe.once('error', reject);
        probe.listen(0, '127.0.0.1', () => {
            const { port } = probe.address() as net.AddressInfo;
            probe.close(() => resolve(port));
        });
    });
}

export function readLog(logFile: string): string {
    try {
        return fs.readFileSync(logFile, 'utf8');
    } catch (e) {
        return `(could not read ${logFile}: ${e instanceof Error ? e.message : e})`;
    }
}

export class ServerNotReady extends Error {
    constructor(readonly log: string) {
        super(`the nx-remote-cache server did not start; its log:\n${log.trimEnd().split('\n').slice(-20).join('\n')}`);
    }
}
