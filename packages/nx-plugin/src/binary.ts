/** The package holding the server binary for a platform, as Node reports it. */
export function platformPackage(platform: NodeJS.Platform, arch: string): string {
    if (platform === 'linux' && arch === 'x64') return '@graycore/nx-remote-cache-linux-x64-gnu';
    if (platform === 'linux' && arch === 'arm64') return '@graycore/nx-remote-cache-linux-arm64-gnu';
    if (platform === 'darwin') return '@graycore/nx-remote-cache-darwin-universal';
    throw new Error(`nx-remote-cache has no server binary for ${platform}/${arch} (supported: Linux x64 and arm64, macOS)`);
}

/**
 * The server binary: NX_REMOTE_CACHE_BINARY if set (e.g. one built from source), otherwise the
 * one npm installed for this platform as an optional dependency.
 *
 * Looked up next to the plugin first, then in the workspace: when the plugin is a linked
 * directory (`file:`, `npm link`), Node resolves from its real location, outside the workspace's
 * node_modules where the platform package is installed.
 */
export function resolveBinary(
    env: NodeJS.ProcessEnv,
    workspaceRoot?: string,
    resolve: (id: string, paths: string[]) => string = (id, paths) => require.resolve(id, { paths }),
    platform: NodeJS.Platform = process.platform,
    arch: string = process.arch,
): string {
    if (env.NX_REMOTE_CACHE_BINARY) {
        return env.NX_REMOTE_CACHE_BINARY;
    }
    const pkg = platformPackage(platform, arch);
    try {
        return resolve(`${pkg}/bin/nx-remote-cache`, [__dirname, ...(workspaceRoot ? [workspaceRoot] : [])]);
    } catch {
        throw new Error(
            `${pkg} is not installed: install the plugin from npm (not with --omit=optional), or set NX_REMOTE_CACHE_BINARY`,
        );
    }
}
