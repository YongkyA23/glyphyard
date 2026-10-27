// Runs inside Illustrator (ExtendScript = ES3: no JSON, no Array#indexOf/forEach, no let/const).
// Every function returns a string; the panel parses it with JSON.parse.

function typeshelfQuote(s) {
    s = String(s);
    var out = '"';
    for (var i = 0; i < s.length; i++) {
        var c = s.charAt(i);
        var code = s.charCodeAt(i);
        if (c === '"' || c === "\\") out += "\\" + c;
        else if (code < 32) out += "\\u" + ("000" + code.toString(16)).slice(-4);
        else out += c;
    }
    return out + '"';
}

function typeshelfDocName(doc) {
    try {
        if (String(doc.path) !== "") return doc.fullName.fsName;
    } catch (e) {} // never saved
    return doc.name;
}

function typeshelfDocuments() {
    var parts = [];
    for (var i = 0; i < app.documents.length; i++) parts.push(typeshelfQuote(typeshelfDocName(app.documents[i])));
    return "[" + parts.join(",") + "]";
}

function typeshelfActiveDocument() {
    try {
        return app.documents.length ? typeshelfDocName(app.activeDocument) : "";
    } catch (e) {
        return "";
    }
}

function typeshelfXmlText(item, tag) {
    var m = item.match(new RegExp("<stFnt:" + tag + ">([^<]*)</stFnt:" + tag + ">"));
    if (!m) return "";
    return m[1].replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, "&");
}

function typeshelfFonts(id) {
    var doc = null;
    for (var d = 0; d < app.documents.length; d++) {
        if (typeshelfDocName(app.documents[d]) === id) doc = app.documents[d];
    }
    if (!doc) return "[]";

    var seen = {};
    var list = [];
    function add(ps, family, style, missing) {
        if (!ps || seen["$" + ps]) return; // "$" keeps names like "constructor" off Object.prototype
        seen["$" + ps] = true;
        list.push("{\"postscriptName\":" + typeshelfQuote(ps) + ",\"family\":" + typeshelfQuote(family) +
            ",\"style\":" + typeshelfQuote(style) + ",\"missing\":" + (missing ? "true" : "false") + "}");
    }
    function installed(ps) {
        try {
            app.textFonts.getByName(ps);
            return true;
        } catch (e) {
            return false;
        }
    }

    // The fonts list Illustrator writes into the file's XMP on save. It still names fonts that are missing
    // now (text frames only report the substitute), so it is the main source on open.
    var xmp = "";
    try {
        xmp = doc.XMPString;
    } catch (e) {}
    var block = xmp.match(/<xmpTPg:Fonts>([\s\S]*?)<\/xmpTPg:Fonts>/);
    if (block) {
        var items = block[1].split("</rdf:li>");
        for (var i = 0; i < items.length; i++) {
            var ps = typeshelfXmlText(items[i], "fontName");
            if (ps) add(ps, typeshelfXmlText(items[i], "fontFamily"), typeshelfXmlText(items[i], "fontFace"), !installed(ps));
        }
    }

    // Text added since the last save. ponytail: capped, because text ranges can be per-character in big files.
    var budget = 20000;
    for (var t = 0; t < doc.textFrames.length && budget > 0; t++) {
        var ranges;
        try {
            ranges = doc.textFrames[t].textRanges;
        } catch (e) {
            continue;
        }
        for (var r = 0; r < ranges.length && budget > 0; r++, budget--) {
            try {
                var f = ranges[r].characterAttributes.textFont;
                add(f.name, f.family, f.style, false);
            } catch (e) {}
        }
    }
    return "[" + list.join(",") + "]";
}
