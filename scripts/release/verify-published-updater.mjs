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
const tag = args.tag || process.env.RELEASE_TAG || process.env.GITHUB_REF_NAME;
const version = args.version;
const releaseId = args['release-id'] || process.env.RELEASE_ID || null;
const attempts = Number(args.attempts || 36);
const delayMs = Number(args['delay-ms'] || 5000);
const requestTimeoutMs = Number(args['request-timeout-ms'] || 15000);
if (!repository || !tag || !version) throw new Error('Published updater verification requires --repo, --tag and --version.');
if (!Number.isInteger(attempts) || attempts < 1) throw new Error('--attempts must be a positive integer.');
if (!Number.isFinite(delayMs) || delayMs < 0) throw new Error('--delay-ms must be a non-negative number.');
if (!Number.isFinite(requestTimeoutMs) || requestTimeoutMs <= 0) throw new Error('--request-timeout-ms must be a positive number.');

const encodedTag = encodeURIComponent(tag);
const tagManifestUrl = `https://github.com/${repository}/releases/download/${encodedTag}/latest.json`;
const latestManifestUrl = `https://github.com/${repository}/releases/latest/download/latest.json`;
const githubApiBase = `https://api.github.com/repos/${repository}`;
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
const log = (message) => process.stderr.write(`[verify-published-updater] ${message}\n`);

function requestSignal() {
  return typeof AbortSignal?.timeout === 'function' ? AbortSignal.timeout(requestTimeoutMs) : undefined;
}

function githubApiHeaders() {
  const headers = {
    Accept: 'application/vnd.github+json',
    'X-GitHub-Api-Version': '2022-11-28',
    'Cache-Control': 'no-cache',
  };
  const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN;
  if (token) headers.Authorization = `Bearer ${token}`;
  return headers;
}

async function retry(label, operation, count = attempts) {
  let lastError;
  for (let attempt = 1; attempt <= count; attempt += 1) {
    try {
      const value = await operation(attempt);
      if (attempt > 1) log(`${label} converged on attempt ${attempt}/${count}.`);
      return value;
    } catch (error) {
      lastError = error instanceof Error ? error : new Error(String(error));
      log(`${label} attempt ${attempt}/${count} failed: ${lastError.message}`);
      if (attempt < count) await sleep(delayMs);
    }
  }
  throw new Error(`${label} did not converge after ${count} attempts. Last error: ${lastError?.message ?? 'unknown error'}`, { cause: lastError });
}

async function fetchJson(url, label, headers = {}) {
  const response = await fetch(url, {
    redirect: 'follow',
    cache: 'no-store',
    headers: { 'Cache-Control': 'no-cache', ...headers },
    signal: requestSignal(),
  });
  if (!response.ok) throw new Error(`${label} returned HTTP ${response.status} ${response.statusText}`);
  try {
    return await response.json();
  } catch (error) {
    throw new Error(`${label} did not return valid JSON`, { cause: error });
  }
}

function validateRelease(release, label) {
  if (release?.tag_name !== tag) throw new Error(`${label} tag ${release?.tag_name ?? '(missing)'} does not match ${tag}`);
  if (release?.draft !== false) throw new Error(`${label} is still draft=${String(release?.draft)}`);
  if (release?.prerelease !== false) throw new Error(`${label} has prerelease=${String(release?.prerelease)}`);
  return release;
}

function validateManifest(manifest, label) {
  if (manifest?.version !== version) throw new Error(`${label} version ${manifest?.version ?? '(missing)'} does not match ${version}`);
  for (const platform of requiredPlatforms) {
    const entry = manifest.platforms?.[platform];
    if (!entry?.url || !entry?.signature) throw new Error(`${label} is missing ${platform} URL/signature.`);
  }
  return manifest;
}

async function verifyReleaseIdentity() {
  if (releaseId) {
    return retry(`release id ${releaseId}`, async () => {
      const release = await fetchJson(`${githubApiBase}/releases/${encodeURIComponent(releaseId)}`, `release id ${releaseId}`, githubApiHeaders());
      return validateRelease(release, `release id ${releaseId}`);
    }, Math.min(attempts, 12));
  }
  return retry(`release ${tag}`, async () => {
    const release = await fetchJson(`${githubApiBase}/releases/tags/${encodedTag}`, `release ${tag}`, githubApiHeaders());
    return validateRelease(release, `release ${tag}`);
  }, Math.min(attempts, 12));
}

async function verifyLatestReleaseIdentity() {
  return retry('GitHub latest-release API', async () => {
    const release = await fetchJson(`${githubApiBase}/releases/latest`, 'GitHub latest-release API', githubApiHeaders());
    if (release?.tag_name !== tag) throw new Error(`latest release is ${release?.tag_name ?? '(missing)'}, expected ${tag}`);
    return validateRelease(release, 'GitHub latest release');
  });
}

async function fetchManifest(url, label) {
  return retry(label, async () => validateManifest(await fetchJson(url, label), label));
}

async function verifyAsset(url, platform) {
  return retry(`${platform} updater asset`, async () => {
    const response = await fetch(url, {
      redirect: 'follow',
      cache: 'no-store',
      headers: {
        Range: 'bytes=0-0',
        'Cache-Control': 'no-cache',
      },
      signal: requestSignal(),
    });
    if (!(response.status === 200 || response.status === 206)) {
      await response.body?.cancel();
      throw new Error(`HTTP ${response.status} ${response.statusText}: ${url}`);
    }
    await response.body?.cancel();
    return true;
  }, Math.min(attempts, 18));
}

const release = await verifyReleaseIdentity();
log(`release identity confirmed: ${release.tag_name} (${release.html_url || `id ${release.id}`}).`);

// First verify the immutable tag-specific manifest. This distinguishes an actual
// missing/broken release asset from temporary propagation of GitHub's /latest alias.
const tagManifest = await fetchManifest(tagManifestUrl, `tag-specific latest.json for ${tag}`);
for (const platform of requiredPlatforms) {
  await verifyAsset(tagManifest.platforms[platform].url, platform);
}

// The desktop updater uses releases/latest/download/latest.json, so the release is
// not considered healthy until both GitHub's latest-release API and the public
// /latest download alias converge to this tag/version. These paths may lag publish
// by tens of seconds, hence bounded retries rather than an immediate destructive failure.
await verifyLatestReleaseIdentity();
const latestManifest = await fetchManifest(latestManifestUrl, 'public latest.json alias');

process.stdout.write(`${JSON.stringify({
  ok: true,
  repository,
  releaseId: release.id,
  tag,
  version,
  tagManifestUrl,
  latestManifestUrl,
  latestManifestVersion: latestManifest.version,
  platforms: requiredPlatforms,
}, null, 2)}\n`);
