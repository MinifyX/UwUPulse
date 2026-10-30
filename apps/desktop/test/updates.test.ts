import assert from 'node:assert/strict';
import { test } from 'node:test';
import { notesFor } from '../src/lib/updates.ts';

test('notesFor: picks the language from JSON notes', () => {
  const notes = JSON.stringify({ de: 'Neu', en: 'New' });
  assert.equal(notesFor(notes, 'de'), 'Neu');
  assert.equal(notesFor(notes, 'en'), 'New');
});

test('notesFor: falls back to the other language, then the plain text', () => {
  assert.equal(notesFor(JSON.stringify({ de: 'Nur Deutsch' }), 'en'), 'Nur Deutsch');
  assert.equal(notesFor(JSON.stringify({ en: 'English only' }), 'de'), 'English only');
  assert.equal(notesFor('- Fixes\n- More fixes', 'en'), '- Fixes\n- More fixes');
  assert.equal(notesFor('"quoted"', 'de'), '"quoted"');
  assert.equal(notesFor(JSON.stringify({ other: 1 }), 'de'), '{"other":1}');
  assert.equal(notesFor(null, 'de'), '');
  assert.equal(notesFor('', 'en'), '');
});
