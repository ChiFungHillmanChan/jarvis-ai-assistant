// Load a Lambda .ts module under test. Lambda code imports siblings without an
// extension (esbuild resolves them); Node's type stripping cannot, so transpile here.
// Relative specifiers load <path>.ts when it exists; JSON and packages resolve normally.
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');
const ts = require('typescript');

const loaded = new Map();

function load(file) {
  if (loaded.has(file)) return loaded.get(file).exports;
  const module = { exports: {} };
  loaded.set(file, module);
  const js = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true },
  }).outputText;
  const fallback = Module.createRequire(file);
  const localRequire = name => {
    if (name.startsWith('.')) {
      const sibling = path.resolve(path.dirname(file), name.replace(/\.ts$/, '') + '.ts');
      if (fs.existsSync(sibling)) return load(sibling);
    }
    return fallback(name);
  };
  new Function('module', 'exports', 'require', js)(module, module.exports, localRequire);
  return module.exports;
}

/** @param {string} pathFromInfra e.g. 'lambda/ai-proxy/config.ts' */
exports.loadTs = pathFromInfra => load(path.resolve(__dirname, '..', pathFromInfra));
