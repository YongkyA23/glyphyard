// Shared by the Photoshop/InDesign UXP plug-ins and the Illustrator CEP panel; build.ts copies it into each.
// Talks to the Typeshelf app's loopback bridge (crates/adobe-bridge). Loaded with a plain <script> tag, so the
// two functions below are globals.

/**
 * @typedef {{ postscriptName: string, family: string, style: string, missing: boolean }} DocFont
 * @typedef {{ activated: string[], missing: string[], alreadyActive: string[] }} DocResult
 * @typedef {"connected" | "not_paired" | "offline"} Status
 * @typedef {(method: string, url: string, headers: Record<string, string>, body: string | undefined,
 *   timeoutMs: number) => Promise<{ status: number, body: string }>} Transport
 * @typedef {{ status(): Promise<Status>, pair(): Promise<boolean>,
 *   sendDocument(document: string, action: "open" | "close", fonts: DocFont[]): Promise<any> }} Client
 * @typedef {{ id: string | number, name: string }} DocRef
 * @typedef {{
 *   listDocuments(): Promise<DocRef[]>,
 *   documentFonts(id: DocRef["id"]): Promise<DocFont[]>,
 *   activeDocumentId(): Promise<DocRef["id"] | null>,
 *   onChange(callback: () => void): void,
 *   restartNote?: string,
 * }} Host
 */

/**
 * @param {{ app: "photoshop" | "illustrator" | "indesign", appVersion: string, transport?: Transport,
 *   storage?: Storage }} options
 * @returns {Client}
 */
function typeshelfClient({ app, appVersion, transport, storage }) {
  const PORTS = [27716, 27717, 27718, 27719, 27720];
  const store = storage || window.localStorage;
  const send = transport || fetchTransport;
  const tokenKey = `typeshelf.${app}.token`;
  /** @type {number | null} */
  let port = null;

  /** @type {Transport} */
  function fetchTransport(method, url, headers, body, timeoutMs) {
    const request = fetch(url, { method, headers, body }).then(async (res) => ({
      status: res.status,
      body: await res.text(),
    }));
    // ponytail: Promise.race instead of AbortController, which not every UXP version has.
    const timeout = new Promise((_, reject) => setTimeout(() => reject(new Error("timeout")), timeoutMs));
    return /** @type {Promise<{ status: number, body: string }>} */ (Promise.race([request, timeout]));
  }

  function clientId() {
    let id = store.getItem(`typeshelf.${app}.clientId`);
    if (!id) {
      id = `${app}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
      store.setItem(`typeshelf.${app}.clientId`, id);
    }
    return id;
  }

  /**
   * @param {number} p
   * @param {string} method
   * @param {string} path
   * @param {unknown} [body]
   */
  async function request(p, method, path, body, timeoutMs = 3000) {
    /** @type {Record<string, string>} */
    const headers = { "X-Typeshelf-Client": `${app}-plugin` };
    const token = store.getItem(tokenKey);
    if (token) headers.Authorization = `Bearer ${token}`;
    if (body !== undefined) headers["Content-Type"] = "application/json";
    const url = `http://127.0.0.1:${p}${path}`;
    const res = await send(method, url, headers, body === undefined ? undefined : JSON.stringify(body), timeoutMs);
    let json = null;
    try {
      json = JSON.parse(res.body);
    } catch (_) {}
    return { status: res.status, json };
  }

  async function discover() {
    for (const p of PORTS) {
      try {
        const res = await request(p, "GET", "/v1/ping", undefined, 1000);
        if (res.status === 200 && res.json && res.json.app === "typeshelf") return (port = p);
      } catch (_) {}
    }
    throw new Error("offline");
  }

  /**
   * @param {string} method
   * @param {string} path
   * @param {unknown} [body]
   * @param {number} [timeoutMs]
   */
  async function call(method, path, body, timeoutMs) {
    const p = port || (await discover());
    try {
      return await request(p, method, path, body, timeoutMs);
    } catch (e) {
      port = null; // Typeshelf quit or moved to another port; rediscover next time.
      throw e;
    }
  }

  return {
    async status() {
      try {
        const res = await call("GET", "/v1/ping");
        if (res.json && res.json.paired) return "connected";
        store.removeItem(tokenKey); // revoked or never paired
        return "not_paired";
      } catch (_) {
        return "offline";
      }
    },

    async pair() {
      // The app shows a prompt and the bridge waits up to 60 s for the user.
      const res = await call("POST", "/v1/pair", { app, appVersion, clientId: clientId() }, 65000);
      if (res.status !== 200 || !res.json || !res.json.token) return false;
      store.setItem(tokenKey, res.json.token);
      return true;
    },

    async sendDocument(document, action, fonts) {
      const res = await call("POST", "/v1/document", { app, document, action, fonts });
      if (res.status === 401) {
        store.removeItem(tokenKey);
        throw new Error("not_paired");
      }
      if (res.status !== 200) throw new Error(`Typeshelf answered ${res.status}`);
      return res.json;
    },
  };
}

/**
 * Wires the panel (shared index.html) and keeps the bridge told about open/closed documents.
 * @param {Client} client
 * @param {Host} host
 */
function typeshelfPanel(client, host) {
  const statusEl = /** @type {HTMLElement} */ (document.getElementById("status"));
  const pairBtn = /** @type {HTMLButtonElement} */ (document.getElementById("pair"));
  const checkBtn = /** @type {HTMLButtonElement} */ (document.getElementById("check"));
  const logEl = /** @type {HTMLElement} */ (document.getElementById("log"));
  const LABELS = { connected: "Connected to Typeshelf", not_paired: "Not paired", offline: "Typeshelf isn't running" };
  /** @type {Status} */
  let state = "offline";
  /** Documents the bridge has been told about: id -> name sent. */
  const tracked = new Map();
  /** @type {Promise<unknown>} */
  let queue = Promise.resolve();
  /** @type {ReturnType<typeof setTimeout> | undefined} */
  let debounce;

  /** @param {string} message */
  function log(message) {
    const li = document.createElement("li");
    li.textContent = `${new Date().toLocaleTimeString()} ${message}`;
    logEl.insertBefore(li, logEl.firstChild);
    while (logEl.childNodes.length > 10) logEl.removeChild(/** @type {ChildNode} */ (logEl.lastChild));
  }

  async function refresh() {
    state = await client.status();
    // A restarted app has forgotten our documents; resend them all once connected again.
    if (state !== "connected") tracked.clear();
    statusEl.textContent = LABELS[state];
    pairBtn.style.display = state === "not_paired" ? "" : "none";
    checkBtn.disabled = state !== "connected";
    return state;
  }

  /** Runs `task` after the previous one; errors are logged, never thrown. @param {() => Promise<unknown>} task */
  function enqueue(task) {
    queue = queue.then(task).catch(async (e) => {
      log(`Error: ${e && e.message ? e.message : e}`);
      await refresh().catch(() => {});
    });
    return queue;
  }

  /** @param {DocRef} doc */
  async function open(doc) {
    const fonts = await host.documentFonts(doc.id);
    if (!fonts.length) {
      tracked.set(doc.id, doc.name);
      return;
    }
    /** @type {DocResult} */
    const r = await client.sendDocument(doc.name, "open", fonts);
    tracked.set(doc.id, doc.name);
    const parts = [];
    // restartNote: the app (InDesign) only sees fonts turned on while it runs after a restart.
    if (r.activated.length) parts.push(`turned on ${r.activated.join(", ")}${host.restartNote ? ` (${host.restartNote})` : ""}`);
    if (r.missing.length) parts.push(`not in library: ${r.missing.join(", ")}`);
    if (!parts.length) parts.push("all fonts already on");
    log(`${doc.name}: ${parts.join("; ")}`);
  }

  async function sync() {
    if (state !== "connected" && (await refresh()) !== "connected") return;
    const docs = await host.listDocuments();
    const ids = new Set(docs.map((d) => d.id));
    for (const [id, name] of [...tracked]) {
      if (ids.has(id)) continue;
      tracked.delete(id);
      await client.sendDocument(name, "close", []);
    }
    for (const doc of docs) if (!tracked.has(doc.id)) await open(doc);
  }

  const scheduleSync = () => {
    clearTimeout(debounce);
    // Hosts fire "open" before the document is fully readable; wait a moment and coalesce bursts.
    debounce = setTimeout(() => enqueue(sync), 500);
  };

  pairBtn.addEventListener("click", () =>
    enqueue(async () => {
      pairBtn.disabled = true;
      statusEl.textContent = "Allow it in Typeshelf…";
      try {
        log((await client.pair()) ? "Paired with Typeshelf" : "Pairing was declined");
      } finally {
        pairBtn.disabled = false;
      }
      await refresh();
      await sync();
    }),
  );

  checkBtn.addEventListener("click", () =>
    enqueue(async () => {
      const id = await host.activeDocumentId();
      const doc = (await host.listDocuments()).find((d) => d.id === id);
      if (doc) await open(doc);
      else log("No document is open");
    }),
  );

  host.onChange(scheduleSync);
  enqueue(sync); // documents that were already open
  // Pick up a Typeshelf that starts after the Adobe app.
  setInterval(() => {
    if (state !== "connected") enqueue(sync);
  }, 15000);
}
