/** The package holding the server binary for a platform, as Node reports it. */
export function platformPackage(platform: NodeJS.Platform, arch: string): string {
    if (platform === 'linux' && arch === 'x64') return '@graycoreio/nx-remote-cache-linux-x64-gnu';
    if (platform === 'linux' && arch === 'arm64') return '@graycoreio/nx-remote-cache-linux-arm64-gnu';
    if (platform === 'darwin') return '@graycoreio/nx-remote-cache-darwin-universal';
    throw new Error(`nx-remote-cache has no server binary for ${platform}/${arch} (supported: Linux x64 and arm64, macOS)`);
}

/**
 * The server binary: NX_REMOTE_CACHE_BINARY if set (e.g. one built from source), otherwise the
 * one npm installed for this platform as an optional dependency.
 */
export function resolveBinary(
    env: NodeJS.ProcessEnv,
    resolve: (id: string) => string = require.resolve,
    platform: NodeJS.Platform = process.platform,
    arch: string = process.arch,
): string {
    if (env.NX_REMOTE_CACHE_BINARY) {
        return env.NX_REMOTE_CACHE_BINARY;
    }
    const pkg = platformPackage(platform, arch);
    try {
        return resolve(`${pkg}/bin/nx-remote-cache`);
    } catch {
        throw new Error(`${pkg} is not installed (npm skips optional dependencies with --omit=optional)`);
    }
}
