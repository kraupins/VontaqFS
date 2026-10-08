import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
let ts;
try {
  ts = require('typescript');
} catch (error) {
  throw new Error(
    'TypeScript is required to build the Figma example. Run npm install in the example/project before npm run build.',
    { cause: error },
  );
}

const here = path.dirname(fileURLToPath(import.meta.url));
const distDir = path.join(here, 'dist');
const sdkRoot = process.env.VONTAQ_FS_SDK_ROOT
  ? path.resolve(process.env.VONTAQ_FS_SDK_ROOT)
  : path.resolve(here, '../../packages/sdk');
const sdkUsesDist = fs.existsSync(path.join(sdkRoot, 'dist', 'index.js'));

fs.rmSync(distDir, { recursive: true, force: true });
fs.mkdirSync(distDir, { recursive: true });

const aliases = new Map([
  ['@vontaq/fs', sdkUsesDist ? path.join(sdkRoot, 'dist', 'index.js') : path.join(sdkRoot, 'src', 'index.ts')],
  ['@vontaq/fs/figma', sdkUsesDist ? path.join(sdkRoot, 'dist', 'figma.js') : path.join(sdkRoot, 'src', 'figma.ts')],
]);

function resolveModule(specifier, fromFile) {
  if (aliases.has(specifier)) return aliases.get(specifier);
  if (!specifier.startsWith('.')) throw new Error(`Unsupported external module in Figma fixture: ${specifier}`);
  const base = path.resolve(path.dirname(fromFile), specifier);
  const candidates = [base];
  if (base.endsWith('.js')) candidates.push(base.slice(0, -3) + '.ts');
  if (base.endsWith('.mjs')) candidates.push(base.slice(0, -4) + '.ts');
  candidates.push(`${base}.ts`, `${base}.js`);
  for (const candidate of candidates) if (fs.existsSync(candidate)) return candidate;
  throw new Error(`Cannot resolve ${specifier} from ${fromFile}`);
}

function bundle(entry) {
  const modules = [];
  const idByFile = new Map();
  function add(file) {
    file = path.resolve(file);
    if (idByFile.has(file)) return idByFile.get(file);
    const id = modules.length;
    idByFile.set(file, id);
    modules.push(null);
    const source = fs.readFileSync(file, 'utf8');
    const output = ts.transpileModule(source, {
      fileName: file,
      compilerOptions: {
        target: ts.ScriptTarget.ES2020,
        module: ts.ModuleKind.CommonJS,
        esModuleInterop: true,
        sourceMap: false,
        removeComments: true,
      },
    }).outputText;
    const deps = {};
    for (const match of output.matchAll(/require\(["']([^"']+)["']\)/g)) {
      const specifier = match[1];
      deps[specifier] = add(resolveModule(specifier, file));
    }
    modules[id] = { file, code: output, deps };
    return id;
  }
  const entryId = add(entry);
  const serializedModules = modules.map((module, id) => {
    const rel = path.relative(here, module.file).replaceAll('\\', '/');
    return `${JSON.stringify(id)}: function(module, exports, require) {\n${module.code}\n}`;
  }).join(',\n');
  const deps = Object.fromEntries(modules.map((module, id) => [id, module.deps]));
  return `(() => {\n"use strict";\nconst __modules = {\n${serializedModules}\n};\nconst __deps = ${JSON.stringify(deps)};\nconst __cache = {};\nfunction __require(id) {\n  if (__cache[id]) return __cache[id].exports;\n  const module = { exports: {} };\n  __cache[id] = module;\n  const localRequire = specifier => {\n    const dep = __deps[id][specifier];\n    if (dep === undefined) throw new Error("Missing bundled module: " + specifier);\n    return __require(dep);\n  };\n  __modules[id](module, module.exports, localRequire);\n  return module.exports;\n}\n__require(${entryId});\n})();\n`;
}

const uiBundle = bundle(path.join(here, 'src', 'ui.ts'));
const uiHtml = `<!doctype html><html><head><meta charset="utf-8"></head><body><script>${uiBundle.replaceAll('</script>', '<\\/script>')}</script></body></html>\n`;
fs.writeFileSync(path.join(distDir, 'ui.html'), uiHtml);
fs.writeFileSync(path.join(distDir, 'code.js'), bundle(path.join(here, 'src', 'code.ts')));

for (const [name, contents] of [
  ['dist/code.js', fs.readFileSync(path.join(distDir, 'code.js'), 'utf8')],
  ['dist/ui.html', uiHtml],
]) {
  if (contents.includes('import(')) throw new Error(`${name} contains scanner-dangerous import(`);
}
console.log('Built Figma main + UI fixture.');
