const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const ts = require("typescript");

const root = path.resolve(__dirname, "..");
const sourcePath = path.join(root, "src", "export-utils.ts");
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

void import(moduleUrl).then(
  ({ byteRanges, numberedExportPath, paginateItemIndexes, safeImageScale }) => {
    assert.deepEqual(byteRanges(0, 4), []);
    assert.deepEqual(byteRanges(10, 4), [
      { start: 0, end: 4 },
      { start: 4, end: 8 },
      { start: 8, end: 10 },
    ]);
    assert.throws(() => byteRanges(-1), RangeError);
    assert.throws(() => byteRanges(1, 0), RangeError);

    assert.equal(numberedExportPath("/tmp/report.png", 0, 1), "/tmp/report.png");
    assert.equal(
      numberedExportPath("C:\\exports\\report.final.png", 0, 12),
      "C:\\exports\\report.final_01.png",
    );
    assert.equal(
      numberedExportPath("/tmp/结果", 11, 12),
      "/tmp/结果_12",
    );
    assert.throws(() => numberedExportPath("x.png", 2, 2), RangeError);

    assert.deepEqual(paginateItemIndexes([], 12, 100), []);
    assert.deepEqual(paginateItemIndexes([30, 30, 30], 5, 65), [
      [0, 1],
      [2],
    ]);
    assert.deepEqual(paginateItemIndexes([120, 20], 5, 100), [[0], [1]]);
    assert.deepEqual(paginateItemIndexes([20, Number.NaN, 20], -3, 100), [
      [0, 1, 2],
    ]);
    const largePages = paginateItemIndexes(
      Array.from({ length: 1_000 }, () => 300),
      12,
      12_000,
    );
    assert.ok(largePages.length > 1);
    assert.deepEqual(
      largePages.flat(),
      Array.from({ length: 1_000 }, (_, index) => index),
    );
    for (const page of largePages) {
      assert.ok(page.length * 300 + Math.max(0, page.length - 1) * 12 <= 12_000);
    }

    assert.equal(safeImageScale(100, 100), 3);
    assert.ok(safeImageScale(1_000, 4_048) > 2.8);
    const tallScale = safeImageScale(1_000, 20_000);
    assert.ok(tallScale > 0 && tallScale < 1);
    assert.ok(20_000 * tallScale <= 16_384 + 1e-9);
    const areaScale = safeImageScale(8_000, 8_000);
    assert.ok(8_000 * 8_000 * areaScale * areaScale <= 32 * 1024 * 1024 + 1);
  },
);
