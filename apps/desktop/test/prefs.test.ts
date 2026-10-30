import assert from 'node:assert/strict';
import { test } from 'node:test';
import { defaultChannel, defaults, parsePrefs } from '../src/lib/prefs.ts';

test('defaultChannel: a pre-release starts on beta', () => {
  assert.equal(defaultChannel('0.1.0'), 'stable');
  assert.equal(defaultChannel('0.2.0-beta.1'), 'beta');
  assert.equal(defaultChannel(''), 'stable');
});

test('parsePrefs: nothing stored gives the defaults', () => {
  assert.deepEqual(parsePrefs(null, '0.1.0'), defaults('0.1.0'));
  assert.deepEqual(parsePrefs('not json', '0.1.0'), defaults('0.1.0'));
  assert.deepEqual(parsePrefs('[1,2]', '0.1.0'), defaults('0.1.0'));
  assert.equal(parsePrefs(null, '1.0.0-rc.1').updateChannel, 'beta');
  assert.equal(defaults().font, 'uwu');
  assert.equal(defaults().autoUpdate, true);
});

test('parsePrefs: keeps valid values and drops unknown ones', () => {
  const prefs = parsePrefs(
    JSON.stringify({
      language: 'en',
      theme: 'dark',
      contrast: 'high',
      motion: 'off',
      font: 'rubik',
      textSize: 'largest',
      updateChannel: 'beta',
      autoUpdate: false,
    }),
  );
  assert.deepEqual(prefs, {
    language: 'en',
    theme: 'dark',
    contrast: 'high',
    motion: 'off',
    font: 'rubik',
    textSize: 'largest',
    updateChannel: 'beta',
    autoUpdate: false,
  });

  const odd = parsePrefs(
    JSON.stringify({ theme: 'neon', font: 'comic', textSize: 3, autoUpdate: 'yes', extra: 1 }),
  );
  assert.equal(odd.theme, 'system');
  assert.equal(odd.font, 'uwu');
  assert.equal(odd.textSize, 'system');
  assert.equal(odd.autoUpdate, true);
  assert.equal('extra' in odd, false);
});
