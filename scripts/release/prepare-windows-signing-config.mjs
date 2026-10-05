import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const mode = String(process.env.WINDOWS_SIGNING_MODE || '').trim().toLowerCase();
const output = path.resolve(process.argv[2] || 'src-tauri/tauri.release.windows.conf.json');
let windows;
if (mode === 'pfx' || mode === 'pfx-self-signed') {
  const thumbprint = String(process.env.WINDOWS_CERTIFICATE_THUMBPRINT || '').replace(/\s/g, '');
  const timestampUrl = String(process.env.WINDOWS_TIMESTAMP_URL || '').trim();
  if (!/^[A-Fa-f0-9]{40,128}$/.test(thumbprint)) throw new Error('Windows PFX signing requires WINDOWS_CERTIFICATE_THUMBPRINT from the imported certificate.');
  if (mode === 'pfx' && !/^https?:\/\//i.test(timestampUrl)) throw new Error('Trusted Windows PFX signing requires WINDOWS_TIMESTAMP_URL.');
  windows = { certificateThumbprint: thumbprint, digestAlgorithm: 'sha256' };
  if (timestampUrl) windows.timestampUrl = timestampUrl;
} else if (mode === 'azure-artifact-signing') {
  const endpoint = String(process.env.WINDOWS_SIGNING_ENDPOINT || '').trim();
  const account = String(process.env.WINDOWS_SIGNING_ACCOUNT || '').trim();
  const profile = String(process.env.WINDOWS_SIGNING_CERT_PROFILE || '').trim();
  if (!/^https:\/\//i.test(endpoint) || !account || !profile) throw new Error('Azure Artifact Signing requires endpoint, account and certificate profile repository variables.');
  const quote = (value) => `"${value.replaceAll('"', '\\"')}"`;
  windows = {
    signCommand: `artifact-signing-cli -e ${quote(endpoint)} -a ${quote(account)} -c ${quote(profile)} -d VontaqFS %1`,
  };
} else {
  throw new Error(`Unsupported WINDOWS_SIGNING_MODE ${JSON.stringify(mode)}. Use pfx, pfx-self-signed or azure-artifact-signing.`);
}
fs.writeFileSync(output, `${JSON.stringify({ bundle: { windows } }, null, 2)}\n`, 'utf8');
process.stdout.write(`${output}\n`);
