/**
 * Nx plugin: before Nx runs tasks, makes sure an nx-remote-cache server is running on 127.0.0.1
 * for this workspace and points Nx's self-hosted cache client at it.
 *
 *   // nx.json
 *   "plugins": [{ "plugin": "@graycoreio/nx-remote-cache", "options": { "storageAccount": "…", … } }]
 *
 * If the server can't start (no `az login`, no role on the container, no OIDC token), Nx runs
 * without the remote cache instead of failing: an unreachable cache would fail the whole run.
 *
 * It also adds a target named after the package to the root project: `nx @graycoreio/nx-remote-cache`
 * checks that the cache works and says why when it doesn't.
 */
import * as fs from 'fs';
import * as path from 'path';
import * as pc from 'picocolors';
import { resolveBinary } from './binary';
import { fetchOidcToken, readRunContext } from './github';
import { ensureServer, stateDirFor } from './server';
import { PluginOptions, resolveSettings, Settings } from './settings';
import { writeLastRun } from './status';

export type { PluginOptions } from './settings';

/** The subset of Nx's PreTasksExecutionContext the plugin uses. */
interface PreTasksExecutionContext {
    readonly workspaceRoot: string;
}

/** The subset of Nx's CreateNodesContext the plugin uses. */
interface CreateNodesContext {
    readonly workspaceRoot: string;
}

export const STATUS_TARGET = '@graycoreio/nx-remote-cache';

// The hook has no output of its own with the Nx daemon (it goes to the daemon's log), so the
// status lives in a target: running it runs the hook, and Nx shows a task's output.
export const createNodes = [
    'nx.json',
    (files: readonly string[], _options: PluginOptions | undefined, context: CreateNodesContext) =>
        files.map((file) => [
            file,
            {
                projects: {
                    '.': {
                        name: rootProjectName(context.workspaceRoot),
                        targets: {
                            [STATUS_TARGET]: {
                                // This package's own CLI, not the `nx-remote-cache` bin: a package
                                // copied into node_modules has no link in node_modules/.bin.
                                command: `node ${JSON.stringify(path.relative(context.workspaceRoot, path.join(__dirname, 'cli.js')))} status`,
                                cache: false,
                                metadata: { description: 'Checks that the remote cache works, or says why it is off.' },
                            },
                        },
                    },
                },
            },
        ] as const),
] as const;

// The same function under the name Nx 20 to 22 look for first. Without it, Nx 20 calls
// `createNodes` with one file at a time, its old signature.
export const createNodesV2 = createNodes;

/** Nx's own name for a root project without project.json: the root package.json's name. */
function rootProjectName(workspaceRoot: string): string {
    try {
        const name = JSON.parse(fs.readFileSync(path.join(workspaceRoot, 'package.json'), 'utf8')).name;
        if (typeof name === 'string' && name) return name;
    } catch {
        // no package.json
    }
    return path.basename(workspaceRoot);
}

// Read once, when Nx loads the plugin. With the daemon, the hook runs inside it and the URL set
// by a previous run stays in its environment, so a later check would mistake it for a server the
// user configured.
const userServer = process.env.NX_SELF_HOSTED_REMOTE_CACHE_SERVER;

export async function preTasksExecution(options: PluginOptions | undefined, context: PreTasksExecutionContext): Promise<void> {
    const env: NodeJS.ProcessEnv = { ...process.env, NX_SELF_HOSTED_REMOTE_CACHE_SERVER: userServer };
    const stateDir = stateDirFor(context.workspaceRoot);
    let settings: Settings | undefined;
    try {
        const resolved = resolveSettings(options, env, () => readRunContext(env));
        if ('disabled' in resolved) {
            writeLastRun(stateDir, { ok: false, message: resolved.disabled });
            // Worth seeing in CI, where it means a job runs without the cache; locally it is
            // usually deliberate (not configured, or switched off).
            log(`remote cache off: ${resolved.disabled}`, env.GITHUB_ACTIONS === 'true' ? 'warn' : 'info');
            return;
        }
        settings = resolved.settings;
        const server = await ensureServer(settings, {
            stateDir,
            binary: resolveBinary(env),
            env,
            oidcToken: settings.mode === 'github' ? await fetchOidcToken(env) : undefined,
        });
        // Nx copies what the hook sets in process.env into the environment of the command.
        process.env.NX_SELF_HOSTED_REMOTE_CACHE_SERVER = server.url;
        process.env.NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN = server.token;
        writeLastRun(stateDir, { ok: true, message: `connected via ${server.url}`, settings });
        log(`remote cache on: ${settings.account}/${settings.container} via ${server.url}`, 'success');
    } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        writeLastRun(stateDir, { ok: false, message, settings });
        log(`remote cache off: ${message}`, 'warn');
    }
}

function log(message: string, level: 'info' | 'success' | 'warn' = 'info'): void {
    if (level === 'warn') {
        console.warn(pc.yellow(`[nx-remote-cache] ${message}`));
    } else if (level === 'success') {
        console.info(pc.blue(`[nx-remote-cache] ${message}`));
    } else if (process.env.NX_VERBOSE_LOGGING === 'true') {
        console.info(`[nx-remote-cache] ${message}`);
    }
}
