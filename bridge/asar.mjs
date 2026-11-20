/**
 * Minimal asar reader.
 *
 * Only what the shell needs at runtime: read one entry without loading the
 * whole archive (the packaged app is ~280 MB) and read the desktop version, so
 * logs and error messages can say which upstream build we are driving.
 */
import { closeSync, openSync, readFileSync, readSync, statSync } from 'node:fs';
import { join } from 'node:path';

/** Read exactly `length` bytes at `position`, or fewer at end of file. */
function readAt(fd, position, length) {
  const buffer = Buffer.alloc(length);
  const read = readSync(fd, buffer, 0, length, position);
  return buffer.subarray(0, read);
}

/** Parse the archive index and return a reader for its entries. */
function openAsar(path) {
  const fd = openSync(path, 'r');
  const headerSize = readAt(fd, 12, 4).readUInt32LE(0);
  const header = JSON.parse(readAt(fd, 16, headerSize).toString('utf8'));
  // The index is a pickled string: its payload starts at 16 and is padded to a
  // 4-byte boundary before the entry data begins.
  const base = (16 + headerSize + 3) & ~3;

  const entries = new Map();
  const walk = (node, prefix) => {
    for (const [name, child] of Object.entries(node.files ?? {})) {
      const entry = `${prefix}/${name}`;
      if (child.files) walk(child, entry);
      else if (child.offset !== undefined) entries.set(entry, { offset: Number(child.offset), size: Number(child.size) });
    }
  };
  walk(header, '');

  return {
    has: (entry) => entries.has(entry),
    read(entry) {
      const found = entries.get(entry);
      if (!found) return null;
      return readAt(fd, base + found.offset, found.size);
    },
    close: () => closeSync(fd)
  };
}

/**
 * Desktop version of the installation behind `root`, or null when unknown.
 *
 * `root` is either an unpacked app directory or `resources/app.asar`.
 */
export function readDesktopVersion(root) {
  try {
    if (!statSync(root).isFile()) {
      return JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version ?? null;
    }
  } catch {
    return null;
  }
  const archive = openAsar(root);
  try {
    const bytes = archive.read('/package.json');
    if (bytes === null) return null;
    return JSON.parse(bytes.toString('utf8')).version ?? null;
  } catch {
    return null;
  } finally {
    archive.close();
  }
}
