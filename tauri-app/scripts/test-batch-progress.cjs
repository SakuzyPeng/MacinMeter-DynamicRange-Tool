const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const ts = require("typescript");

const root = path.resolve(__dirname, "..");
const sourcePath = path.join(root, "src", "batch-progress.ts");
const source = fs.readFileSync(sourcePath, "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ES2020,
    target: ts.ScriptTarget.ES2020,
  },
  fileName: sourcePath,
  reportDiagnostics: true,
});

if (compiled.diagnostics?.length) {
  throw new Error(
    ts.formatDiagnosticsWithColorAndContext(compiled.diagnostics, {
      getCanonicalFileName: (fileName) => fileName,
      getCurrentDirectory: () => root,
      getNewLine: () => "\n",
    }),
  );
}

const moduleUrl = `data:text/javascript;base64,${Buffer.from(compiled.outputText).toString("base64")}`;

void import(moduleUrl).then(({ BatchProgress }) => {
  const progress = new BatchProgress(3);
  const observed = [];

  // A high-index lane may start and finish first. It represents one third of
  // the batch, not the two items before it, and later low-index events may not
  // make the aggregate go backwards.
  observed.push(progress.update(2, 0));
  observed.push(progress.update(2, 1));
  observed.push(progress.update(0, 0.5));
  observed.push(progress.update(1, 1));
  observed.push(progress.update(0, 0.1));
  observed.push(progress.update(0, 1));

  const expected = [0, 100 / 3, 50, 250 / 3, 250 / 3, 100];
  for (let index = 0; index < observed.length; index += 1) {
    assert.ok(Math.abs(observed[index] - expected[index]) < 1e-12);
  }
  for (let index = 1; index < observed.length; index += 1) {
    assert.ok(observed[index] >= observed[index - 1]);
  }

  progress.reset(2);
  assert.equal(progress.update(1, 1), 50);
  assert.equal(progress.update(99, 1), 50);
  assert.equal(progress.update(0, Number.NaN), 50);
});
