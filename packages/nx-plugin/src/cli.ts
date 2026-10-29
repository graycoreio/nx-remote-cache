#!/usr/bin/env node
/**
 *   nx-remote-cache stop      stops the server the plugin started for this workspace
 *   nx-remote-cache <args…>   runs the server binary, e.g. `nx-remote-cache generate-token`
 */
import { spawnSync } from 'child_process';
import * as fs from 'fs';
import * as path from 'path';

import { resolveBinary } from './binary';
import { stateDirFor, stopServer } from './server';

/** The nearest directory with an nx.json, as Nx itself finds the workspace. */
function findWorkspaceRoot(dir: string): string {
    for (let d = dir; ; d = path.dirname(d)) {
        if (fs.existsSync(path.join(d, 'nx.json'))) return d;
        if (path.dirname(d) === d) throw new Error(`no nx.json in ${dir} or above`);
    }
}

async function main(args: string[]): Promise<number> {
    if (args[0] === 'stop') {
        const stopped = await stopServer(stateDirFor(findWorkspaceRoot(process.cwd())));
        console.log(stopped ? 'stopped the nx-remote-cache server' : 'no nx-remote-cache server is running');
        return 0;
    }
    const result = spawnSync(resolveBinary(process.env), args, { stdio: 'inherit' });
    if (result.error) throw result.error;
    return result.status ?? 1;
}

main(process.argv.slice(2)).then(
    (code) => process.exit(code),
    (e) => {
        console.error(e instanceof Error ? e.message : e);
        process.exit(1);
    },
);
