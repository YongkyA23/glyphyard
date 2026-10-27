// bun plugins/build.ts → plugins/dist/{typeshelf-photoshop.ccx, typeshelf-indesign.ccx,
//   com.typeshelf.illustrator/, com.typeshelf.photoshop/, com.typeshelf.indesign/}
// The CEP folders come from one source (plugins/cep); Photoshop and InDesign use theirs when Creative
// Cloud's UXP installer isn't on the machine.
import { cpSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";

const root = import.meta.dir;
const dist = join(root, "dist");
const SHARED = ["shared/typeshelf-client.js", "shared/index.html"];

/** Plug-in folder + shared files, copied to `dest`. */
function stage(dir: string, dest: string) {
  cpSync(join(root, dir), dest, { recursive: true });
  for (const f of SHARED) cpSync(join(root, f), join(dest, basename(f)));
}

function files(dir: string, prefix = ""): string[] {
  return readdirSync(dir)
    .sort()
    .flatMap((name) =>
      statSync(join(dir, name)).isDirectory() ? files(join(dir, name), `${prefix}${name}/`) : [`${prefix}${name}`],
    );
}

/** Minimal deflate zip (a .ccx is a zip of the plug-in folder with manifest.json at the root). */
function zip(dir: string): Uint8Array {
  const local: Uint8Array[] = [];
  const central: Uint8Array[] = [];
  let offset = 0;
  for (const name of files(dir)) {
    const data = readFileSync(join(dir, name));
    const packed = Bun.deflateSync(data); // raw DEFLATE, as zip method 8 expects
    const nameBytes = new TextEncoder().encode(name);
    // Fields shared by the local and central headers, starting at "version needed":
    // version 2.0, UTF-8 names, deflate, time 0, date 1980-01-01 (reproducible), crc, sizes, name length.
    const fields = (v: DataView, at: number) => {
      v.setUint16(at, 20, true);
      v.setUint16(at + 2, 0x0800, true);
      v.setUint16(at + 4, 8, true);
      v.setUint16(at + 6, 0, true);
      v.setUint16(at + 8, 0x21, true);
      v.setUint32(at + 10, Bun.hash.crc32(data), true);
      v.setUint32(at + 14, packed.length, true);
      v.setUint32(at + 18, data.length, true);
      v.setUint16(at + 22, nameBytes.length, true);
    };
    const head = new Uint8Array(30 + nameBytes.length);
    const hv = new DataView(head.buffer);
    hv.setUint32(0, 0x04034b50, true);
    fields(hv, 4);
    head.set(nameBytes, 30);
    local.push(head, packed);

    const entry = new Uint8Array(46 + nameBytes.length);
    const ev = new DataView(entry.buffer);
    ev.setUint32(0, 0x02014b50, true);
    ev.setUint16(4, 20, true); // version made by
    fields(ev, 6);
    ev.setUint32(42, offset, true);
    entry.set(nameBytes, 46);
    central.push(entry);
    offset += head.length + packed.length;
  }
  const centralSize = central.reduce((n, c) => n + c.length, 0);
  const end = new Uint8Array(22);
  const v = new DataView(end.buffer);
  v.setUint32(0, 0x06054b50, true);
  v.setUint16(8, central.length, true);
  v.setUint16(10, central.length, true);
  v.setUint32(12, centralSize, true);
  v.setUint32(16, offset, true);
  return Buffer.concat([...local, ...central, end]);
}

rmSync(dist, { recursive: true, force: true });
mkdirSync(dist);
for (const app of ["photoshop", "indesign"]) {
  const staging = join(dist, ".stage", app);
  stage(`${app}-uxp`, staging);
  writeFileSync(join(dist, `typeshelf-${app}.ccx`), zip(staging));
}
rmSync(join(dist, ".stage"), { recursive: true, force: true });
// CEP host code and the oldest version supported (CSXS 11): Illustrator 26, Photoshop 23, InDesign 17 — all 2022.
const CEP = [
  ["illustrator", "ILST", "26.0"],
  ["photoshop", "PHXS", "23.0"],
  ["indesign", "IDSN", "17.0"],
];
for (const [app, host, min] of CEP) {
  const id = `com.typeshelf.${app}`;
  const dest = join(dist, id);
  stage("cep", dest);
  const manifest = join(dest, "CSXS", "manifest.xml");
  writeFileSync(
    manifest,
    readFileSync(manifest, "utf8").replaceAll("{{ID}}", id).replaceAll("{{HOST}}", host).replaceAll("{{MIN}}", min),
  );
}
console.log(`Built ${files(dist).length} files in ${dist}`);
