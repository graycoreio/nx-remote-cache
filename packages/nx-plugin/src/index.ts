/**
 * Nx plugin: before Nx runs tasks, makes sure an nx-remote-cache server is running on 127.0.0.1
 * for this workspace and points Nx's self-hosted cache client at it.
 *
 *   // nx.json
 *   "plugins": [{ "plugin": "@graycoreio/nx-remote-cache", "options": { "storageAccount": "…", … } }]
 *
 * If the server can't start (no `az login`, no role on the container, no OIDC token), Nx runs
 * without the remote cache instead of failing: an unreachable cache would fail the whole run.
 */
import { resolveBinary } from './binary';
import { fetchOidcToken, readRunContext } from './github';
import { ensureServer, stateDirFor } from './server';
import { PluginOptions, resolveSettings } from './settings';

export type { PluginOptions } from './settings';

/** The subset of Nx's PreTasksExecutionContext the plugin uses. */
interface PreTasksExecutionContext {
    readonly workspaceRoot: string;
}

// Read once, when Nx loads the plugin. With the daemon, the hook runs inside it and the URL set
// by a previous run stays in its environment, so a later check would mistake it for a server the
// user configured.
const userServer = process.env.NX_SELF_HOSTED_REMOTE_CACHE_SERVER;

export async function preTasksExecution(options: PluginOptions | undefined, context: PreTasksExecutionContext): Promise<void> {
    const env: NodeJS.ProcessEnv = { ...process.env, NX_SELF_HOSTED_REMOTE_CACHE_SERVER: userServer };
    try {
        const resolved = resolveSettings(options, env, () => readRunContext(env));
        if ('disabled' in resolved) {
            // Worth seeing in CI, where it means a job runs without the cache; locally it is
            // usually deliberate (not configured, or switched off).
            log(`remote cache off: ${resolved.disabled}`, env.GITHUB_ACTIONS === 'true' ? 'warn' : 'info');
            return;
        }
        const { settings } = resolved;
        const server = await ensureServer(settings, {
            stateDir: stateDirFor(context.workspaceRoot),
            binary: resolveBinary(env),
            env,
            oidcToken: settings.mode === 'github' ? await fetchOidcToken(env) : undefined,
        });
        // Nx copies what the hook sets in process.env into the environment of the command.
        process.env.NX_SELF_HOSTED_REMOTE_CACHE_SERVER = server.url;
        process.env.NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN = server.token;
    } catch (e) {
        log(`remote cache off: ${e instanceof Error ? e.message : e}`, 'warn');
    }
}

function log(message: string, level: 'info' | 'warn' = 'info'): void {
    if (level === 'warn' || process.env.NX_VERBOSE_LOGGING === 'true') {
        console[level](`[nx-remote-cache] ${message}`);
    }
}
