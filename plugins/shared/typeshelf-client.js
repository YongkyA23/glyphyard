// Shared by the Photoshop/InDesign UXP plug-ins and the Illustrator CEP panel; build.ts copies it into each.
// Talks to the Glyphyard app's loopback bridge (crates/adobe-bridge). Loaded with a plain <script> tag, so the
// two functions below are globals.

/**
 * @typedef {{ postscriptName: string, family: string, style: string, missing: boolean }} DocFont
 * @typedef {{ activated: string[], missing: string[], alreadyActive: string[] }} DocResult
 * @typedef {"connected" | "not_paired" | "offline"} Status
 * @typedef {(method: string, url: string, headers: Record<string, string>, body: string | undefined,
 *   timeoutMs: number) => Promise<{ status: number, body: string }>} Transport
 * @typedef {{ status(): Promise<Status>, documentsOnBridge(): number | null, pair(): Promise<boolean>,
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
  /** This app's documents the bridge had open at the last connected ping; null if it didn't say. */
  /** @type {number | null} */
  let documents = null;

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
      port = null; // Glyphyard quit or moved to another port; rediscover next time.
      throw e;
    }
  }

  return {
    async status() {
      documents = null;
      try {
        const res = await call("GET", "/v1/ping");
        if (res.json && res.json.paired) {
          if (typeof res.json.documents === "number") documents = res.json.documents;
          return "connected";
        }
        store.removeItem(tokenKey); // revoked or never paired
        return "not_paired";
      } catch (_) {
        return "offline";
      }
    },

    documentsOnBridge() {
      return documents;
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
      if (res.status !== 200) throw new Error(`Glyphyard answered ${res.status}`);
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
  const LABELS = { connected: "Connected to Glyphyard", not_paired: "Not paired", offline: "Glyphyard isn't running" };
  /** @type {Status} */
  let state = "offline";
  /** Documents the bridge has been told about: id -> name sent. */
  const tracked = new Map();
  /** Ids of tracked documents whose fonts were sent with "open" (the bridge only knows these). */
  const reported = new Set();
  /** Set when Glyphyard may have forgotten our documents; the next sync re-sends every open document. */
  let reReport = false;
  /** The bridge closes an app's documents when it hasn't heard from it for 60 s and the app has quit. */
  const HEARTBEAT_MS = 15000;
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
    // A restarted Glyphyard has forgotten our documents, and one that didn't hear from us for 60 s while it saw
    // no process of our app closed them: resend them all. Seen as losing the connection, or as a connected bridge
    // with fewer documents than we sent (it keys them by name, so count distinct names). A document event in
    // between re-adds only one, hence "fewer", not "none". `tracked` is kept so closed documents get their "close".
    const onBridge = client.documentsOnBridge();
    const sent = new Set([...reported].map((id) => tracked.get(id))).size;
    if (state !== "connected" || (onBridge !== null && onBridge < sent)) reReport = true;
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
      reported.delete(doc.id); // nothing sent, so the bridge may rightly have none of it
      return;
    }
    /** @type {DocResult} */
    const r = await client.sendDocument(doc.name, "open", fonts);
    tracked.set(doc.id, doc.name);
    reported.add(doc.id);
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
      await client.sendDocument(name, "close", []);
      tracked.delete(id); // only once sent, so a failed close is retried by the next sync
      reported.delete(id);
    }
    // Re-opening a document the bridge still knows only adds to it, so re-sending is safe.
    for (const doc of docs) if (reReport || !tracked.has(doc.id)) await open(doc);
    reReport = false;
  }

  const scheduleSync = () => {
    clearTimeout(debounce);
    // Hosts fire "open" before the document is fully readable; wait a moment and coalesce bursts.
    debounce = setTimeout(() => enqueue(sync), 500);
  };

  pairBtn.addEventListener("click", () =>
    enqueue(async () => {
      pairBtn.disabled = true;
      statusEl.textContent = "Allow it in Glyphyard…";
      try {
        log((await client.pair()) ? "Paired with Glyphyard" : "Pairing was declined");
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
  async function heartbeat() {
    // Not connected: pick up a Glyphyard that starts after the Adobe app.
    if (state !== "connected") return sync();
    // Connected: an authenticated ping, which also tells us whether the bridge forgot our documents.
    if ((await refresh()) === "connected" && reReport) await sync();
  }
  setInterval(() => enqueue(heartbeat), HEARTBEAT_MS);
}
