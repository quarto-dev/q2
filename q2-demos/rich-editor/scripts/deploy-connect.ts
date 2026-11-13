/**
 * Publish `dist/` (the build) to Posit Connect as static content.
 *
 *   npm run deploy:connect
 *
 * Connect's address and API key come from rsconnect-python's own config,
 * saved once per machine (outside the repo) with
 *
 *   uvx --from rsconnect-python rsconnect add --name connect \
 *     --server https://connect.posit.it --api-key <key>
 *
 * The server's nickname is `connect` (`CONNECT_SERVER` overrides it). The
 * build updates the demo's existing content on connect.posit.it, whose id
 * is below (`CONNECT_APP_ID` overrides it; set it empty to publish new
 * content). The id isn't a secret: Connect decides who can update or open
 * it.
 */

import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const DEMO_APP_ID = 'a52850b5-b6b5-4655-86d9-98f55ef4590f'

const server = process.env.CONNECT_SERVER ?? 'connect'
const appId = process.env.CONNECT_APP_ID ?? DEMO_APP_ID
const dist = fileURLToPath(new URL('../dist/', import.meta.url))

const args = ['--from', 'rsconnect-python', 'rsconnect', 'deploy', 'html', '--name', server]
if (appId) args.push('--app-id', appId)
else args.push('--title', 'Rich editor prototype')
args.push(dist)

const { status, error } = spawnSync('uvx', args, { stdio: 'inherit' })
if (error) throw error
process.exit(status ?? 1)
