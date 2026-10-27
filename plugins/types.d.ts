// Just enough host globals for `bunx tsc -p plugins` to type-check the plug-in scripts.
declare function require(id: string): any;
declare module "photoshop";
declare module "indesign";
declare module "uxp";
declare module "http"; // Node, inside CEP
/** ExtendScript (Illustrator) global. UXP code gets `app` from require("photoshop"/"indesign"). */
declare var app: any;
interface Window {
  /** CEP host bridge; CSInterface.js is a wrapper around it. */
  __adobe_cep__: any;
}
