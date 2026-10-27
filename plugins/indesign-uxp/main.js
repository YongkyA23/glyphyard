// InDesign (UXP): on document open/close, send the document's fonts (with their status) to Typeshelf.
(() => {
  const indesign = require("indesign");
  const uxp = require("uxp");
  const { app } = indesign;
  const INSTALLED = indesign.FontStatus && indesign.FontStatus.INSTALLED;

  /** @param {any} doc */
  function docName(doc) {
    try {
      const full = doc.fullName; // throws for documents that were never saved
      if (full) return String(full.fsName || full.nativePath || full);
    } catch (_) {}
    return String(doc.name);
  }

  /** @param {any} f */
  function toDocFont(f) {
    const [nameFamily = "", nameStyle = ""] = String(f.name || "").split("\t");
    const family = f.fontFamily || nameFamily;
    const style = f.fontStyleName || nameStyle;
    let ps = "";
    try {
      ps = f.postscriptName || "";
    } catch (_) {}
    // ponytail: missing fonts can lack a PostScript name; "Family-Style" without spaces is the usual one.
    if (!ps) ps = `${family}-${style}`.replace(/\s+/g, "");
    let missing = false;
    try {
      missing = INSTALLED !== undefined && String(f.status) !== String(INSTALLED);
    } catch (_) {}
    return { postscriptName: ps, family, style, missing };
  }

  const host = {
    restartNote: "restart InDesign to use them",
    async listDocuments() {
      const docs = [];
      for (let i = 0; i < app.documents.length; i++) {
        const doc = app.documents.item(i);
        docs.push({ id: doc.id, name: docName(doc) });
      }
      return docs;
    },
    /** @param {number} id */
    async documentFonts(id) {
      const doc = app.documents.itemByID(id);
      if (!doc || !doc.isValid) return [];
      const fonts = new Map();
      for (let i = 0; i < doc.fonts.length; i++) {
        try {
          const f = toDocFont(doc.fonts.item(i));
          if (!fonts.has(f.postscriptName)) fonts.set(f.postscriptName, f);
        } catch (e) {
          console.error("Typeshelf: unreadable font", e);
        }
      }
      return [...fonts.values()];
    },
    async activeDocumentId() {
      try {
        return app.documents.length ? app.activeDocument.id : null;
      } catch (_) {
        return null;
      }
    },
    /** @param {() => void} callback */
    onChange(callback) {
      for (const event of ["afterOpen", "afterClose", "afterActivate"]) {
        try {
          app.addEventListener(event, callback);
        } catch (e) {
          console.error(`Typeshelf: cannot listen for ${event}`, e);
        }
      }
    },
  };

  const client = typeshelfClient({ app: "indesign", appVersion: String(uxp.host && uxp.host.version) });
  typeshelfPanel(client, host);
})();
