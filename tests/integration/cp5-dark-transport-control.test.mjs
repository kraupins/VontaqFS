import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';

const css=readFileSync(new URL('../../apps/desktop/src/styles.css',import.meta.url),'utf8');
const ui=readFileSync(new URL('../../apps/desktop/src/main.tsx',import.meta.url),'utf8');

test('dark method radio control preserves checked, focus, disabled, and forced colors presentation',()=>{
  assert.match(css,/\.transport-method input\{[^}]*color-scheme:dark;/);
  assert.match(css,/\.transport-method input\{[^}]*appearance:none;/);
  assert.match(css,/\.transport-method input:checked/);
  assert.match(css,/\.transport-method input:focus-visible/);
  assert.match(css,/\.transport-method input:disabled/);
  assert.match(css,/@media\s*\(forced-colors:active\)\{\.transport-method input\{appearance:auto;forced-color-adjust:auto\}\}/);
});

test('transport remains an authorized PUT/POST fieldset with native radio and label semantics',()=>{
  assert.match(ui,/activeTransport\.authorization==='AUTHORIZED'&&activeTransport\.allowedMethods\.length===2/);
  assert.match(ui,/<fieldset disabled=\{transportApplying\} className="transport-methods">/);
  assert.match(ui,/<legend>\{tr\('Method for new transfers'/);
  assert.match(ui,/\(\['PUT','POST'\] as const\)\.map/);
  assert.match(ui,/<label key=\{method\} className="transport-method"><input type="radio" name="chunk-method" checked=\{activeTransport\.effectiveMethod===method\} onChange=\{\(\)=>void changeTransport\(method\)\}/);
});
