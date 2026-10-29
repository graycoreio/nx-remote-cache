/**
 * The plugin's options in nx.json. None of them are secrets. Each is overridden by its
 * environment variable (see ENV). The CI identities are only ever set through the environment
 * (TENANT_ID_ENV, WRITER_CLIENT_ID_ENV, READER_CLIENT_ID_ENV), where a workflow takes them from
 * its repository variables.
 */
export interface PluginOptions {
    storageAccount?: string;
    storageContainer?: string;
    /** Virtual directory in the container for cache entries. The server's default is `v1`. */
    storagePrefix?: string;
    /**
     * Which CI identity to ask for: `auto` (the writer only on a push to the default branch),
     * `read` or `write`. Azure decides what each identity may actually do.
     */
    access?: Access;
}

/** The environment variable that overrides each option. */
export const ENV = {
    storageAccount: 'NX_REMOTE_CACHE_STORAGE_ACCOUNT',
    storageContainer: 'NX_REMOTE_CACHE_STORAGE_CONTAINER',
    storagePrefix: 'NX_REMOTE_CACHE_STORAGE_PREFIX',
    access: 'NX_REMOTE_CACHE_ACCESS',
} as const satisfies Record<keyof PluginOptions, string>;

/** For GitHub Actions: the Entra tenant and the CI identities' client IDs. */
export const TENANT_ID_ENV = 'NX_REMOTE_CACHE_TENANT_ID';
export const WRITER_CLIENT_ID_ENV = 'NX_REMOTE_CACHE_WRITER_CLIENT_ID';
export const READER_CLIENT_ID_ENV = 'NX_REMOTE_CACHE_READER_CLIENT_ID';

export type Access = 'auto' | 'read' | 'write';

interface Storage {
    account: string;
    container: string;
    prefix?: string;
}

/** How the server reaches storage, and what Nx may do through it. */
export type Settings = Storage &
    (
        | {
              /** The developer's own `az login`; their role on the container decides what they can do. */
              mode: 'developer_tools';
          }
        | {
              /** A GitHub Actions job: its OIDC token, exchanged for one of the CI identities. */
              mode: 'github';
              tenantId: string;
              clientId: string;
              write: boolean;
          }
    );

export type Resolved = { settings: Settings } | { disabled: string };

export interface RunContext {
    eventName?: string;
    ref?: string;
    defaultBranch?: string;
}

/** Turns the options and the environment into settings, or the reason the cache stays off. */
export function resolveSettings(
    options: PluginOptions | undefined,
    env: NodeJS.ProcessEnv,
    readRunContext: () => RunContext,
): Resolved {
    if (['off', 'false', '0'].includes(env.NX_REMOTE_CACHE ?? '')) {
        return { disabled: `NX_REMOTE_CACHE=${env.NX_REMOTE_CACHE}` };
    }
    if (env.NX_SELF_HOSTED_REMOTE_CACHE_SERVER) {
        return { disabled: 'NX_SELF_HOSTED_REMOTE_CACHE_SERVER is already set; using that server' };
    }
    // An empty variable counts as unset, as GitHub expands a missing repository variable to ''.
    const option = <K extends keyof PluginOptions>(key: K): string | undefined =>
        env[ENV[key]] || options?.[key] || undefined;
    const account = option('storageAccount');
    const container = option('storageContainer');
    if (!account || !container) {
        return {
            disabled: `no storage configured (storageAccount and storageContainer, or ${ENV.storageAccount} and ${ENV.storageContainer})`,
        };
    }
    const storage: Storage = { account, container, prefix: option('storagePrefix') };

    if (env.GITHUB_ACTIONS !== 'true') {
        return { settings: { ...storage, mode: 'developer_tools' } };
    }
    if (!env.ACTIONS_ID_TOKEN_REQUEST_URL || !env.ACTIONS_ID_TOKEN_REQUEST_TOKEN) {
        // Also what a fork's pull request looks like: GitHub never gives those an OIDC token.
        return { disabled: 'this job has no GitHub OIDC token (it needs `permissions: id-token: write`)' };
    }
    const access = parseAccess(option('access') ?? 'auto');
    const write = wantsWrite(access, readRunContext());
    const tenantId = env[TENANT_ID_ENV];
    const clientIdEnv = write ? WRITER_CLIENT_ID_ENV : READER_CLIENT_ID_ENV;
    const clientId = env[clientIdEnv];
    if (!tenantId || !clientId) {
        return { disabled: `${TENANT_ID_ENV} and ${clientIdEnv} are needed in GitHub Actions` };
    }
    return { settings: { ...storage, mode: 'github', tenantId, clientId, write } };
}

function parseAccess(value: string): Access {
    if (value === 'auto' || value === 'read' || value === 'write') {
        return value;
    }
    throw new Error(`access must be auto, read or write, not ${JSON.stringify(value)}`);
}

/**
 * Whether to ask for the writer identity. `auto` asks for it only on a push to the default
 * branch. This only selects: the identities' federated credentials and roles decide what is
 * granted.
 */
export function wantsWrite(access: Access, ctx: RunContext): boolean {
    if (access !== 'auto') {
        return access === 'write';
    }
    return ctx.eventName === 'push' && !!ctx.defaultBranch && ctx.ref === `refs/heads/${ctx.defaultBranch}`;
}
