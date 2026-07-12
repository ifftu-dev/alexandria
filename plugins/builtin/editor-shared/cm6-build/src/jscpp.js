// Standalone JSCPP (C/C++ interpreter) exposed as globalThis.AlexJSCPP.
// JSCPP is CommonJS ({ run, includes }); default-import unwraps module.exports.
import JSCPP from "JSCPP";
export { JSCPP };
export const run = JSCPP.run;
