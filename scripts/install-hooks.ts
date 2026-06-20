import { chmodSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const gitDir = join(process.cwd(), '.git');
if (!existsSync(gitDir)) {
	console.log('[hooks] not a git repo, skip');
	process.exit(0);
}

const hooksDir = join(gitDir, 'hooks');
mkdirSync(hooksDir, { recursive: true });

const prePushPath = join(hooksDir, 'pre-push');
const prePushContent = `#!/bin/sh
# Auto-installed by \`bun run prepare\`. Edit scripts/install-hooks.ts to change.
echo "[pre-push] running tests..."
bun run test
if [ $? -ne 0 ]; then
  echo ""
  echo "[pre-push] tests FAILED — push aborted."
  echo "[pre-push] fix failing tests, or bypass with: git push --no-verify"
  exit 1
fi
echo "[pre-push] tests passed — pushing..."
`;

writeFileSync(prePushPath, prePushContent);
chmodSync(prePushPath, 0o755);
console.log(`[hooks] pre-push installed -> ${prePushPath}`);