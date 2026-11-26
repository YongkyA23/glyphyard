// Photoshop (UXP): on document open/close, send the fonts of all text layers to Glyphyard.
(() => {
  const { app, action } = require("photoshop");
  const uxp = require("uxp");

  /** @param {any} doc */
  function docName(doc) {
    try {
      if (doc.path) return String(doc.path);
    } catch (_) {} // unsaved documents have no path
    return String(doc.title || doc.name || `Untitled ${doc.id}`);
  }

  /** @param {number} id */
  function findDocument(id) {
    for (let i = 0; i < app.documents.length; i++) if (app.documents[i].id === id) return app.documents[i];
    return null;
  }

  /** Text layers, including those nested in groups. @param {any} layers @param {any[]} out */
  function textLayers(layers, out) {
    for (let i = 0; layers && i < layers.length; i++) {
      const layer = layers[i];
      if (layer.kind === "text") out.push(layer);
      else if (layer.layers) textLayers(layer.layers, out);
    }
    return out;
  }

  /** PostScript names of installed fonts, or null when the API isn't there. */
  function installedFonts() {
    try {
      const names = new Set();
      for (let i = 0; i < app.fonts.length; i++) names.add(app.fonts[i].postScriptName);
      return names;
    } catch (_) {
      return null;
    }
  }

  /** @param {number} id */
  async function documentFonts(id) {
    const doc = findDocument(id);
    if (!doc) return [];
    const layers = textLayers(doc.layers, []);
    if (!layers.length) return [];
    // One batchPlay round trip for all layers; Photoshop keeps the original PostScript name even when the
    // font is missing, which is exactly what Glyphyard needs.
    const results = await action.batchPlay(
      layers.map((l) => ({
        _obj: "get",
        _target: [{ _property: "textKey" }, { _ref: "layer", _id: l.id }, { _ref: "document", _id: id }],
      })),
      {},
    );
    const installed = installedFonts();
    const fonts = new Map();
    for (const r of results || []) {
      const ranges = (r && r.textKey && r.textKey.textStyleRange) || [];
      for (const range of ranges) {
        const style = (range && range.textStyle) || {};
        const ps = style.fontPostScriptName;
        if (!ps || fonts.has(ps)) continue;
        fonts.set(ps, {
          postscriptName: ps,
          family: style.fontName || "",
          style: style.fontStyleName || "",
          missing: installed ? !installed.has(ps) : false,
        });
      }
    }
    return [...fonts.values()];
  }

  const host = {
    async listDocuments() {
      const docs = [];
      for (let i = 0; i < app.documents.length; i++) {
        const doc = app.documents[i];
        docs.push({ id: doc.id, name: docName(doc) });
      }
      return docs;
    },
    documentFonts,
    async activeDocumentId() {
      try {
        return app.activeDocument ? app.activeDocument.id : null;
      } catch (_) {
        return null;
      }
    },
    /** @param {() => void} callback */
    onChange(callback) {
      try {
        action.addNotificationListener(["open", "close"], callback);
      } catch (e) {
        console.error("Glyphyard: cannot listen for documents", e);
      }
    },
  };

  const client = typeshelfClient({ app: "photoshop", appVersion: String(uxp.host && uxp.host.version) });
  typeshelfPanel(client, host);
})();
