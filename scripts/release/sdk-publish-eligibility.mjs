import { readFileSync, appendFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

// A runtime-only release must never attempt publishing an older SDK,
// regardless of PUBLISH_NPM. A matching version still requires explicit opt-in.
export function sdkPublishEligibility({ runtimeVersion, sdkVersion, releaseTag, publishNpm }) {
  if (!/^\d+\.\d+\.\d+$/.test(runtimeVersion) || !/^\d+\.\d+\.\d+$/.test(sdkVersion)) {
    throw new Error('Expected stable Runtime and SDK SemVer for publication eligibility');
  }
  if (releaseTag !== `v${runtimeVersion}`) throw new Error('Release tag does not match Runtime version');
  const eligible = runtimeVersion === sdkVersion && publishNpm === 'true';
  return {
    publish: eligible,
    reason: runtimeVersion !== sdkVersion
      ? 'runtime-only: SDK package version differs from release version'
      : publishNpm !== 'true' ? 'npm publication not enabled' : 'matching SDK release explicitly enabled',
  };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
  const runtimeVersion = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8')).version;
  const sdkVersion = JSON.parse(readFileSync(path.join(root, 'packages/sdk/package.json'), 'utf8')).version;
  const result = sdkPublishEligibility({
    runtimeVersion, sdkVersion,
    releaseTag: process.env.RELEASE_TAG ?? '',
    publishNpm: process.env.PUBLISH_NPM ?? '',
  });
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `publish=${result.publish}\n`);
  process.stdout.write(`${JSON.stringify(result)}\n`);
}
