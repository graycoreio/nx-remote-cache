import * as fs from 'fs';

import { RunContext } from './settings';

/** The audience Entra expects on a GitHub OIDC token exchanged for a managed identity. */
export const AZURE_TOKEN_EXCHANGE_AUDIENCE = 'api://AzureADTokenExchange';

/** The parts of the run that decide which identity to ask for. */
export function readRunContext(
    env: NodeJS.ProcessEnv,
    readFile: (path: string) => string = (path) => fs.readFileSync(path, 'utf8'),
): RunContext {
    let defaultBranch: string | undefined;
    if (env.GITHUB_EVENT_PATH) {
        try {
            defaultBranch = JSON.parse(readFile(env.GITHUB_EVENT_PATH))?.repository?.default_branch;
        } catch {
            // No usable event payload: `auto` then never selects the writer.
        }
    }
    return { eventName: env.GITHUB_EVENT_NAME, ref: env.GITHUB_REF, defaultBranch };
}

/** Requests the job's GitHub OIDC token for the Entra token exchange. */
export async function fetchOidcToken(env: NodeJS.ProcessEnv, fetchImpl: typeof fetch = fetch): Promise<string> {
    const url = new URL(env.ACTIONS_ID_TOKEN_REQUEST_URL!);
    url.searchParams.set('audience', AZURE_TOKEN_EXCHANGE_AUDIENCE);
    const response = await fetchImpl(url, {
        headers: { Authorization: `bearer ${env.ACTIONS_ID_TOKEN_REQUEST_TOKEN}` },
    });
    if (!response.ok) {
        throw new Error(`could not get a GitHub OIDC token: HTTP ${response.status}`);
    }
    const { value } = (await response.json()) as { value?: string };
    if (!value) {
        throw new Error('could not get a GitHub OIDC token: empty response');
    }
    return value;
}
