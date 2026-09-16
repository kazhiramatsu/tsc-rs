import fs from 'node:fs';
const read = fs.readFileSync;
fs.readFileSync = (file, ...args) => String(file).endsWith('/fixtures/declaration-comment-ranges.json')
  ? Buffer.from('changed fixture bytes') : read(file, ...args);
