// CEP panel for Illustrator, Photoshop and InDesign: on document open/switch/close, send the document's
// fonts to Typeshelf. (Photoshop/InDesign use it when Creative Cloud's UXP installer isn't available.)
// Uses window.__adobe_cep__ directly (what CSInterface.js wraps) so no Adobe library has to be vendored.
(() => {
  const cep = window.__adobe_cep__;
  const http = require("http");

  /** @param {string} script @returns {Promise<string>} */
  function evalScript(script) {
    return new Promise((resolve) => cep.evalScript(script, resolve));
  }

  /** @param {string} script */
  async function evalJson(script) {
    const out = await evalScript(script);
    try {
      return JSON.parse(out);
    } catch (_) {
      throw new Error(`Adobe script failed: ${out}`);
    }
  }

  /** Node's http instead of fetch: CEF would send a CORS preflight, which the bridge refuses. @type {Transport} */
  function nodeTransport(method, url, headers, body, timeoutMs) {
    return new Promise((resolve, reject) => {
      const req = http.request(url, { method, headers, timeout: timeoutMs }, (/** @type {any} */ res) => {
        let data = "";
        res.setEncoding("utf8");
        res.on("data", (/** @type {string} */ chunk) => (data += chunk));
        res.on("end", () => resolve({ status: res.statusCode || 0, body: data }));
      });
      req.on("timeout", () => req.destroy(new Error("timeout")));
      req.on("error", reject);
      if (body) req.write(body);
      req.end();
    });
  }

  /** @type {any} */
  let env = {};
  try {
    env = JSON.parse(cep.getHostEnvironment());
    const c = env.appSkinInfo.panelBackgroundColor.color;
    const light = c.red + c.green + c.blue > 3 * 128;
    const root = document.documentElement.style;
    root.setProperty("--bg", `rgb(${c.red}, ${c.green}, ${c.blue})`);
    root.setProperty("--fg", light ? "#1d1d1d" : "#e8e8e8");
    root.setProperty("--muted", light ? "#555" : "#a8a8a8");
  } catch (_) {}

  /** CEP host codes → the app ids the Typeshelf bridge knows. @type {Record<string, "illustrator" | "photoshop" | "indesign">} */
  const APPS = { ILST: "illustrator", PHXS: "photoshop", PHSP: "photoshop", IDSN: "indesign" };

  const host = {
    // Documents have no stable id in ExtendScript; the path (or name, if unsaved) identifies them.
    async listDocuments() {
      const names = await evalJson("typeshelfDocuments()");
      return names.map((/** @type {string} */ name) => ({ id: name, name }));
    },
    /** @param {string} id */
    async documentFonts(id) {
      return evalJson(`typeshelfFonts(${JSON.stringify(id)})`);
    },
    async activeDocumentId() {
      return (await evalScript("typeshelfActiveDocument()")) || null;
    },
    /** @param {() => void} callback */
    onChange(callback) {
      // There is no reliable close event; switching/deactivation lets the panel diff the document list.
      // Not every app sends every event; listening for one it never sends is harmless.
      for (const event of ["documentAfterOpen", "documentAfterActivate", "documentAfterDeactivate", "applicationActivate"]) {
        try {
          cep.addEventListener(event, callback);
        } catch (e) {
          console.error(`Typeshelf: cannot listen for ${event}`, e);
        }
      }
    },
  };

  const app = APPS[String(env.appName)] || "illustrator";
  const client = typeshelfClient({ app, appVersion: String(env.appVersion || ""), transport: nodeTransport });
  typeshelfPanel(client, host);
})();
