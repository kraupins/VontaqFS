import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const output = path.resolve(process.argv[2] || 'src-tauri/tauri.release.updater.conf.json');
const endpoint = String(process.env.VONTAQFS_UPDATE_ENDPOINT || '').trim();
const publicKey = String(process.env.VONTAQFS_UPDATER_PUBLIC_KEY || '').trim();

if (!/^https:\/\//i.test(endpoint)) {
  throw new Error('Production updater configuration requires VONTAQFS_UPDATE_ENDPOINT to be HTTPS.');
}
if (!publicKey) {
  throw new Error('Production updater configuration requires VONTAQFS_UPDATER_PUBLIC_KEY.');
}

const config = {
  bundle: {
    createUpdaterArtifacts: true,
  },
  plugins: {
    updater: {
      pubkey: publicKey,
      endpoints: [endpoint],
      windows: {
        installMode: 'passive',
      },
    },
  },
};

fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, `${JSON.stringify(config, null, 2)}\n`, 'utf8');
process.stdout.write(`${output}\n`);
