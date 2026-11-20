/**
 * Renderer shim: recreates the globals the Electron preload used to define, on
 * top of the shell HTTP API served by the bridge (`/__dsh_tauri__/`).
 *
 * Loaded into the application document by the bridge's proxy, before the client
 * bundle runs. Keep it dependency-free and side-effect-light: the client treats
 * every global here as optional, so a throwing shim degrades the app instead of
 * loading it.
 */
(() => {
  const BASE = '/__dsh_tauri__';

  const call = (path, body) =>
    fetch(BASE + path, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body)
    }).then((response) => (response.ok ? response.json() : Promise.reject(new Error(`shell ${path}: ${response.status}`))));

  const pickDirectory = async () => {
    const result = await call('/pick-directory', {});
    return result.path ?? undefined;
  };

  window.__DSH_DIRECTORY_PICKER__ = { pick: pickDirectory };

  window.__DSH_TAURI__ = {
    pickDirectory,
    openExternal: (url) => call('/open-external', { url }),
    notify: (title, body) => call('/notify', { title, body }),
    readClipboard: () => call('/clipboard', {}).then((result) => result.text ?? ''),
    writeClipboard: (text) => call('/clipboard', { text }),
    preferences: () => call('/preferences'),
    setPreferences: (patch) => call('/preferences', { patch })
  };

  /**
   * Desktop boot handshake. Resolving this switches the client into desktop mode
   * (`__DSH_TRANSPORT__.ownsHost`), which is what enables the native-only slots.
   * Index injections are applied by the Host when it serves the document, so the
   * list handed back here is empty on purpose.
   */
  window.dshDesktopBoot = {
    async ready() {
      const info = await call('/info');
      return {
        injections: [],
        streamBaseUrl: globalThis.location.origin,
        documentId: 'main',
        profile: info.profile ?? null
      };
    },
    async failed(message) {
      await call('/report', { kind: 'boot-failed', message: String(message) }).catch(() => undefined);
    }
  };

  /**
   * Absolute paths for dropped files. Electron answered this synchronously from
   * `webUtils.getPathForFile`; WebKit exposes no path at all, so staged copies
   * are keyed by the identity a `File` does expose (name, size, mtime).
   */
  const staged = new Map();
  const fileKey = (file) => `${file.name}:${file.size}:${file.lastModified}`;

  window.__DSH_HOST_PATHS__ = {
    pathFor(file) {
      return staged.get(fileKey(file));
    }
  };

  const stageDrop = async (files) => {
    for (const file of files) {
      const buffer = await file.arrayBuffer();
      const bytes = new Uint8Array(buffer);
      let binary = '';
      for (let index = 0; index < bytes.length; index += 1) binary += String.fromCharCode(bytes[index]);
      const result = await call('/stage-file', { name: file.name, dataBase64: btoa(binary) });
      if (typeof result?.path === 'string') staged.set(fileKey(file), result.path);
    }
  };

  /**
   * Intercept real drops, stage the payload, then re-dispatch an equivalent drop
   * so the client's own handler runs with paths available. Staging failures are
   * swallowed on purpose: the client falls back to uploading file contents.
   */
  window.addEventListener(
    'drop',
    (event) => {
      const files = Array.from(event.dataTransfer?.files ?? []);
      if (files.length === 0 || typeof DataTransfer !== 'function') return;
      event.preventDefault();
      event.stopImmediatePropagation();
      const target = event.target;
      const init = { bubbles: true, cancelable: true, dataTransfer: new DataTransfer() };
      (async () => {
        try {
          await stageDrop(files);
          for (const file of files) init.dataTransfer.items.add(file);
        } catch {
          return;
        }
        target?.dispatchEvent(new DragEvent('drop', init));
      })();
    },
    true
  );

  /** Permission plumbing for the settings screen; the Host still owns policy. */
  window.desktopNext = {
    renderer: {
      profile: () => call('/info').then((info) => info.profile ?? null),
      report: (payload) => call('/report', { kind: 'renderer', payload }),
      diagnostic: (payload) => call('/report', { kind: 'diagnostic', payload })
    },
    state: () => call('/preferences'),
    permissions: {
      query: () => call('/permissions'),
      request: () => call('/permissions')
    },
    onOpenSettings: () => () => {}
  };
})();
