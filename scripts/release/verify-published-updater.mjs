import process from 'node:process';

function parseArgs(argv) {
  const result = {};
  for (let i = 2; i < argv.length; i += 2) {
    const key = argv[i];
    const value = argv[i + 1];
    if (!key?.startsWith('--') || value == null) throw new Error(`Invalid argument list near ${key ?? '(end)'}`);
    result[key.slice(2)] = value;
  }
  return result;
}

const args = parseArgs(process.argv);
const repository = args.repo || process.env.GITHUB_REPOSITORY;
const tag = args.tag || process.env.GITHUB_REF_NAME;
const version = args.version;
const attempts = Number(args.attempts || 10);
const delayMs = Number(args['delay-ms'] || 3000);
if (!repository || !tag || !version) throw new Error('Published updater verification requires --repo, --tag and --version.');

const manifestUrl = `https://github.com/${repository}/releases/latest/download/latest.json`;
const requiredPlatforms = [
  'windows-x86_64-nsis',
  'windows-x86_64-msi',
  'windows-aarch64-nsis',
  'windows-aarch64-msi',
  'windows-x86_64',
  'windows-aarch64',
  'darwin-x86_64',
  'darwin-aarch64',
];

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function fetchManifest() {
  let lastError;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      const response = await fetch(manifestUrl, { redirect: 'follow', cache: 'no-store' });
      if (!response.ok) throw new Error(`latest.json returned HTTP ${response.status}`);
      const manifest = await response.json();
      if (manifest.version !== version) throw new Error(`latest.json version ${manifest.version} does not match ${version}`);
      return manifest;
    } catch (error) {
      lastError = error;
      if (attempt < attempts) await sleep(delayMs);
    }
  }
  throw lastError;
}

async function verifyAsset(url, platform) {
  const response = await fetch(url, {
    redirect: 'follow',
    cache: 'no-store',
    headers: { Range: 'bytes=0-0' },
  });
  if (!(response.status === 200 || response.status === 206)) {
    throw new Error(`${platform} updater URL returned HTTP ${response.status}: ${url}`);
  }
  await response.body?.cancel();
}

const manifest = await fetchManifest();
for (const platform of requiredPlatforms) {
  const entry = manifest.platforms?.[platform];
  if (!entry?.url || !entry?.signature) throw new Error(`latest.json is missing ${platform} URL/signature.`);
  await verifyAsset(entry.url, platform);
}

process.stdout.write(`${JSON.stringify({ ok: true, tag, version, manifestUrl, platforms: requiredPlatforms }, null, 2)}\n`);
