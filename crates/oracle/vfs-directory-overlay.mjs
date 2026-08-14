import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";

function compareDirectoryNames(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function normalizedAbsolutePath(fileName, currentDirectory) {
  return ts.getNormalizedAbsolutePath(fileName, currentDirectory);
}

/**
 * Index the directories implied by an iterable of virtual file names.
 *
 * TypeScript's compiler test harness creates every parent directory before it
 * writes a virtual document. This index exposes the same immediate-child
 * directory view without consulting the machine running the oracle.
 */
export function createVirtualDirectoryIndex(
  fileNames,
  currentDirectory,
  useCaseSensitiveFileNames = true,
) {
  const canonicalize = ts.createGetCanonicalFileName(
    useCaseSensitiveFileNames,
  );
  const entries = new Map();

  function entry(directory) {
    const normalized = normalizedAbsolutePath(directory, currentDirectory);
    const key = canonicalize(normalized);
    let value = entries.get(key);
    if (value === undefined) {
      value = { path: normalized, children: new Map() };
      entries.set(key, value);
    }
    return value;
  }

  for (const fileName of fileNames) {
    const absolute = normalizedAbsolutePath(fileName, currentDirectory);
    let directory = ts.getDirectoryPath(absolute);
    entry(directory);
    while (true) {
      const parent = ts.getDirectoryPath(directory);
      if (parent === directory) break;
      const childName = ts.getBaseFileName(directory);
      entry(parent).children.set(canonicalize(childName), childName);
      directory = parent;
    }
  }

  return Object.freeze({
    has(directory) {
      const normalized = normalizedAbsolutePath(directory, currentDirectory);
      return entries.has(canonicalize(normalized));
    },
    getDirectories(directory) {
      const normalized = normalizedAbsolutePath(directory, currentDirectory);
      const value = entries.get(canonicalize(normalized));
      if (value === undefined) return undefined;
      return [...value.children.values()].sort(compareDirectoryNames);
    },
  });
}

/**
 * Overlay virtual directory queries on a compiler host without leaking
 * physical children into a virtual directory.
 *
 * `getDirectories` intentionally returns immediate child basenames, matching
 * TypeScript's `Harness.Fakes.System`. A virtual directory shadows the
 * fallback even when it contains no child directories; falling back merely
 * because the result is empty would make oracle output depend on the host FS.
 */
export function createHermeticDirectoryOverlay(
  fileNames,
  {
    currentDirectory,
    useCaseSensitiveFileNames = true,
    fallbackHost,
  },
) {
  const index = createVirtualDirectoryIndex(
    fileNames,
    currentDirectory,
    useCaseSensitiveFileNames,
  );
  const fallbackDirectoryExists = fallbackHost?.directoryExists?.bind(fallbackHost);
  const fallbackGetDirectories = fallbackHost?.getDirectories?.bind(fallbackHost);

  return Object.freeze({
    directoryExists(directory) {
      if (index.has(directory)) return true;
      const normalized = normalizedAbsolutePath(directory, currentDirectory);
      return fallbackDirectoryExists?.(normalized) ?? false;
    },
    getDirectories(directory) {
      const virtual = index.getDirectories(directory);
      if (virtual !== undefined) return virtual;
      const normalized = normalizedAbsolutePath(directory, currentDirectory);
      return fallbackGetDirectories?.(normalized) ?? [];
    },
  });
}
