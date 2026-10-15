#!/usr/bin/env node
//   WRIT_PERF_GATE=1 node scripts/perf-editor.mjs [--only <id>]... [--large] [--out <file.json>]
//   node scripts/perf-editor.mjs --write-files <dir> [--only <id>]... [--large]   (Node 22.18+)
// Chromium: WRIT_PLAYWRIGHT_CORE (path to playwright-core's index.mjs), an installed
// playwright-core with its chromium, or WRIT_PERF_CHROMIUM. Exit 0 within budget,
// 1 over budget, 2 when the gate could not measure. docs/perf-budgets.md describes the gate.
import http from "node:http";
import os from "node:os";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { extname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  BUDGETS,
  METRICS,
  MiB,
  SCENARIOS,
  TOGGLE_BUDGETS,
  formatValue,
  maximum,
  median,
  overBudget,
  percentile,
} from "./perf-editor/budgets.mjs";
import { ROOT, harnessBuildConfig } from "./perf-editor/harness.mjs";

const RARE_WORD = "zebrafish";
const ABSENT_WORD = "qxjzv";
const TYPED = "The quick brown fox jumps over";
const TYPED_WITH_FIND = " jumps";
const WARM_TOGGLES = 6;
const SETTLE_TIMEOUT_MS = 120_000;
const MATCH_COUNT_CAP = 10_000;
// The whole-text count the page checks the status bar against allocates many
// times the document, so only files up to this size are checked by it.
const WHOLE_TEXT_CHECK_MAX_BYTES = 8 * MiB;

class GateSetupError extends Error {
  constructor(message) {
    super(message);
    this.name = "GateSetupError";
  }
}

function parseArgs(argv) {
  const options = { only: [], large: false, out: null, writeFiles: null };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--large") options.large = true;
    else if (arg === "--only" && argv[i + 1]) options.only.push(argv[++i]);
    else if (arg === "--out" && argv[i + 1]) options.out = resolve(argv[++i]);
    else if (arg === "--write-files" && argv[i + 1]) options.writeFiles = resolve(argv[++i]);
    else throw new GateSetupError(`unknown argument: ${arg}`);
  }
  for (const id of options.only) {
    if (!SCENARIOS.some((scenario) => scenario.id === id)) throw new GateSetupError(`no scenario named ${id}`);
  }
  return options;
}

async function loadChromium() {
  const explicit = process.env.WRIT_PLAYWRIGHT_CORE;
  try {
    const module = explicit ? await import(pathToFileURL(resolve(explicit)).href) : await import("playwright-core");
    return module.chromium ?? module.default?.chromium;
  } catch (error) {
    throw new GateSetupError(
      `playwright-core did not load (${error.message}); set WRIT_PLAYWRIGHT_CORE to its index.mjs or install it`,
    );
  }
}

async function buildHarness(outDir) {
  const { build } = await import("vite");
  await build(harnessBuildConfig(outDir));
}

const MIME = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".css": "text/css",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
  ".json": "application/json",
  ".wasm": "application/wasm",
  ".png": "image/png",
};

function serve(dir) {
  return new Promise((resolveServer) => {
    const server = http.createServer(async (request, response) => {
      const url = new URL(request.url ?? "/", "http://127.0.0.1");
      const path = url.pathname === "/" ? "/index.html" : decodeURIComponent(url.pathname);
      const file = resolve(dir, `.${path}`);
      if (!file.startsWith(dir)) {
        response.writeHead(403);
        response.end();
        return;
      }
      try {
        const body = await readFile(file);
        response.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream" });
        response.end(body);
      } catch {
        response.writeHead(404);
        response.end();
      }
    });
    server.listen(0, "127.0.0.1", () => resolveServer(server));
  });
}

async function heapUsed(cdp) {
  await cdp.send("HeapProfiler.collectGarbage");
  await cdp.send("HeapProfiler.collectGarbage");
  const { usedSize } = await cdp.send("Runtime.getHeapUsage");
  return usedSize;
}

const longestTask = (observed) => maximum(observed.longTasks.map((task) => task.duration)) ?? 0;
const keyMs = (samples) => samples.map((sample) => sample.ms);

async function openPage(browser, base, problems) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2 });
  const page = await context.newPage();
  page.on("pageerror", (error) => problems.push(`page error: ${error.message.slice(0, 300)}`));
  page.on("crash", () => problems.push("the renderer crashed"));
  await page.goto(`${base}/index.html`);
  await page.waitForFunction(() => window.__harness?.ready(), null, { timeout: 60_000 });
  await page.waitForTimeout(1000);
  return { context, page };
}

async function typeInFind(page, word) {
  await page.evaluate(() => {
    window.__harness.resetObservations();
    window.__harness.startKeys();
  });
  await page.keyboard.type(word, { delay: 150 });
  const settledAt = await page.evaluate((timeout) => window.__harness.waitFindSettled(timeout), SETTLE_TIMEOUT_MS);
  await page.waitForTimeout(300);
  const keys = await page.evaluate(() => window.__harness.stopKeys());
  const observed = await page.evaluate(() => window.__harness.observations());
  const status = await page.evaluate(() => window.__harness.findStatus());
  if (settledAt === null) throw new GateSetupError(`the count for "${word}" never settled`);
  const lastKey = keys.at(-1);
  if (!lastKey) throw new GateSetupError(`no keystroke was recorded typing "${word}"`);
  return { keys, settleMs: settledAt - lastKey.at, taskMs: longestTask(observed), status };
}

async function clearFind(page) {
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.press("Backspace");
  await page.evaluate((timeout) => window.__harness.waitFindSettled(timeout), SETTLE_TIMEOUT_MS);
  await page.waitForTimeout(300);
}

function expectCount(status, expected, word, problems) {
  const total = Math.min(expected, MATCH_COUNT_CAP);
  const { matches } = status;
  if (matches.invalid) problems.push(`Find called "${word}" an invalid pattern`);
  if (matches.total !== total || matches.capped !== expected > MATCH_COUNT_CAP) {
    problems.push(`Find counted ${matches.total} for "${word}", the file holds ${expected}`);
  }
}

async function checkCounts(page, file, result, problems) {
  const counted = await page.evaluate((timeout) => window.__harness.waitCountsExact(timeout), SETTLE_TIMEOUT_MS);
  if (counted === null) {
    problems.push("the counts never finished");
    return;
  }
  result.counts = { words: counted.words, characters: counted.characters, shown: counted.shown };
  if (!/\bwords?\b/.test(counted.shown) || counted.shown.includes("≈")) {
    problems.push(`the status bar showed "${counted.shown}" for finished counts`);
  }
  if (file.size > WHOLE_TEXT_CHECK_MAX_BYTES) return;
  const whole = await page.evaluate(() => window.__harness.wholeTextCounts());
  if (whole.words !== counted.words || whole.characters !== counted.characters) {
    problems.push(
      `the status bar counted ${counted.words} words and ${counted.characters} characters, the text holds ${whole.words} and ${whole.characters}`,
    );
  }
}

async function runScenario(browser, base, scenario) {
  const problems = [];
  const result = { id: scenario.id, tier: scenario.tier, loadAverage: os.loadavg()[0], problems };
  const { context, page } = await openPage(browser, base, problems);
  try {
    const cdp = await context.newCDPSession(page);
    await cdp.send("HeapProfiler.enable");
    await page.evaluate((on) => window.__harness.setCounts(on), scenario.counts === true);
    const file = await page.evaluate((spec) => window.__harness.addFile(spec), scenario.spec);
    result.file = { size: file.size, lines: file.lines, rareWordCount: file.rareWordCount };
    const heapBefore = await heapUsed(cdp);
    const opened = await page.evaluate((path) => window.__harness.open(path), file.path);
    if (scenario.split) {
      // The file shows in both panes and the typing below goes to the second
      // one, whose editor carries every change into the first.
      const split = await page.evaluate(() => window.__harness.splitRight());
      result.split = split;
      if (split.focused !== "right" || split.leftLength !== split.rightLength || split.rightLength === 0) {
        problems.push(`the split did not show the file twice: ${JSON.stringify(split)}`);
      }
    }
    await page.waitForTimeout(3000);
    result.features = await page.evaluate(() => window.__harness.features());
    if (result.features.tier !== scenario.opensAs) {
      problems.push(`opened as ${result.features.tier}, expected ${scenario.opensAs}`);
    }
    if (scenario.spec.kind === "markdown-blocks") {
      // The numbers count only if the screen shows the drawings, not their source.
      result.blocks = await page.evaluate(() => window.__harness.blocks());
      const { tables, diagrams, failedDiagrams, math } = result.blocks;
      if (tables === 0 || diagrams === 0 || math === 0 || failedDiagrams > 0) {
        problems.push(
          `the blocks on screen were not drawn: ${tables} tables, ${diagrams} diagrams, ${math} formulas, ${failedDiagrams} failed diagrams`,
        );
      }
    }
    if (opened.confirmed !== (scenario.tier === "LargeFileConfirm")) {
      problems.push(opened.confirmed ? "the open asked about the size" : "the open did not ask about the size");
    }
    await page.evaluate(() => window.__harness.dropBytes());
    const heapAfter = await heapUsed(cdp);
    const cold = await page.evaluate(() => window.__harness.firstPaints().filter((paint) => paint.mode === "cold"));
    result.coldFirstPaintMs = cold.map((paint) => paint.elapsedMs);

    const measured = { firstPaintMs: opened.firstPaintMs, heapMiB: (heapAfter - heapBefore) / MiB };

    await page.evaluate(() => {
      window.__harness.focusEditor();
      window.__harness.startKeys();
    });
    await page.keyboard.press("ControlOrMeta+End");
    const atEnd = await page.evaluate(() => window.__harness.waitViewportAtEnd(20_000));
    await page.waitForTimeout(300);
    const jump = await page.evaluate(() => window.__harness.stopKeys());
    if (atEnd === null || jump.length === 0) problems.push("the jump to the end never showed the end");
    else measured.jumpToEndMs = Math.max(jump[0].ms, atEnd - jump[0].at);

    await page.evaluate(() => window.__harness.startKeys());
    await page.keyboard.type(TYPED, { delay: 110 });
    await page.evaluate(() => window.__harness.resetObservations());
    await page.waitForTimeout(3500);
    const typing = keyMs(await page.evaluate(() => window.__harness.stopKeys()));
    measured.typingP50Ms = median(typing);
    measured.typingP95Ms = percentile(typing, 95);
    measured.typingMaxMs = maximum(typing);
    measured.idleTaskMs = longestTask(await page.evaluate(() => window.__harness.observations()));

    await page.keyboard.press("ControlOrMeta+f");
    await page.waitForSelector(".find-input", { timeout: 5000 });
    const rare = await typeInFind(page, RARE_WORD);
    expectCount(rare.status, file.rareWordCount, RARE_WORD, problems);
    await clearFind(page);
    const absent = await typeInFind(page, ABSENT_WORD);
    expectCount(absent.status, 0, ABSENT_WORD, problems);
    const findKeys = [...keyMs(rare.keys), ...keyMs(absent.keys)];
    measured.findKeyP50Ms = median(findKeys);
    measured.findKeyMaxMs = maximum(findKeys);
    measured.findSettleMs = Math.max(rare.settleMs, absent.settleMs);

    await clearFind(page);
    const again = await typeInFind(page, RARE_WORD);
    await page.evaluate(() => window.__harness.focusEditor());
    await page.keyboard.press("ControlOrMeta+End");
    await page.waitForTimeout(500);
    await page.evaluate(() => {
      window.__harness.resetObservations();
      window.__harness.startKeys();
    });
    await page.keyboard.type(TYPED_WITH_FIND, { delay: 120 });
    const editSettledAt = await page.evaluate((timeout) => window.__harness.waitFindSettled(timeout), SETTLE_TIMEOUT_MS);
    await page.waitForTimeout(3500);
    const withFind = await page.evaluate(() => window.__harness.stopKeys());
    const withFindObserved = await page.evaluate(() => window.__harness.observations());
    const afterEdit = await page.evaluate(() => window.__harness.findStatus());
    if (editSettledAt === null) problems.push("the count never settled after typing with Find open");
    expectCount(afterEdit, file.rareWordCount, `${RARE_WORD}" after typing "${TYPED_WITH_FIND}`, problems);
    measured.findTypingP50Ms = median(keyMs(withFind));
    measured.findTypingMaxMs = maximum(keyMs(withFind));
    measured.findTypingTaskMs = longestTask(withFindObserved);
    measured.findTaskMs = Math.max(rare.taskMs, absent.taskMs, again.taskMs);

    if (scenario.counts) await checkCounts(page, file, result, problems);

    result.measured = measured;
    problems.push(...overBudget(scenario.id, measured));
  } catch (error) {
    problems.push(`the run stopped: ${String(error?.message ?? error).split("\n")[0]}`);
    result.stopped = true;
  } finally {
    await context.close().catch(() => undefined);
  }
  result.loadAverageEnd = os.loadavg()[0];
  return result;
}

async function runToggle(browser, base) {
  const problems = [];
  const { context, page } = await openPage(browser, base, problems);
  try {
    for (let i = 0; i < WARM_TOGGLES; i++) {
      await page.evaluate(() => window.__harness.emitWindowShown());
      await page.waitForTimeout(250);
    }
    const paints = await page.evaluate(() => window.__harness.firstPaints());
    return {
      cold: paints.filter((paint) => paint.mode === "cold").map((paint) => paint.elapsedMs),
      // The first warm sample is dropped, as the manual capture drops it.
      warm: paints.filter((paint) => paint.mode === "warm").map((paint) => paint.elapsedMs).slice(1),
      problems,
    };
  } finally {
    await context.close().catch(() => undefined);
  }
}

function printScenario(scenario, result) {
  const load = `${result.loadAverage.toFixed(1)} to ${result.loadAverageEnd.toFixed(1)}`;
  console.log(`\n${scenario.id} (${scenario.tier}, opens as ${result.features?.tier ?? "n/a"}; load average ${load})`);
  const budget = BUDGETS[scenario.id] ?? {};
  for (const metric of METRICS) {
    const value = result.measured?.[metric.key];
    const over = typeof value === "number" && value > budget[metric.key];
    console.log(
      `  ${metric.label.padEnd(28)} ${formatValue(value, metric.unit).padStart(10)}  budget ${formatValue(budget[metric.key], metric.unit).padStart(10)}${over ? "  OVER" : ""}`,
    );
  }
  if (result.counts) console.log(`  ${"Counts shown".padEnd(28)} ${result.counts.shown}`);
  for (const problem of result.problems) console.log(`  problem: ${problem}`);
}

async function writeScenarioFiles(scenarios, dir) {
  const generator = pathToFileURL(join(ROOT, "src/__tests__/editor/fixtures/editor-corpus.ts")).href;
  let generateCorpus;
  try {
    ({ generateCorpus } = await import(generator));
  } catch (error) {
    throw new GateSetupError(`the file generator did not load (${error.message}); --write-files needs Node 22.18 or later`);
  }
  await mkdir(dir, { recursive: true });
  for (const scenario of scenarios) {
    const corpus = generateCorpus(scenario.spec);
    const path = join(dir, scenario.spec.name);
    await writeFile(path, corpus.bytes);
    console.log(`perf-editor: wrote ${path} (${corpus.bytes.length} bytes, ${corpus.lines} lines)`);
  }
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const scenarios = SCENARIOS.filter((scenario) =>
    options.only.length ? options.only.includes(scenario.id) : !scenario.large || options.large,
  );
  if (options.writeFiles) {
    await writeScenarioFiles(scenarios, options.writeFiles);
    return 0;
  }
  if (process.env.WRIT_PERF_GATE !== "1") {
    console.log("perf-editor: skipped (set WRIT_PERF_GATE=1 to run)");
    return 0;
  }
  const chromium = await loadChromium();
  const outDir = await mkdtemp(join(os.tmpdir(), "writ-perf-editor-"));
  let server = null;
  let browser = null;
  try {
    await buildHarness(outDir);
    server = await serve(outDir);
    const base = `http://127.0.0.1:${server.address().port}`;
    browser = await chromium.launch({
      headless: true,
      ...(process.env.WRIT_PERF_CHROMIUM ? { executablePath: process.env.WRIT_PERF_CHROMIUM } : { channel: "chromium" }),
      args: [
        "--disable-renderer-backgrounding",
        "--disable-background-timer-throttling",
        "--disable-backgrounding-occluded-windows",
      ],
    });
    const machine = {
      cpu: os.cpus()[0]?.model ?? "unknown",
      cores: os.cpus().length,
      memoryGiB: Math.round(os.totalmem() / 1024 ** 3),
      platform: `${os.platform()} ${os.release()}`,
      browser: browser.version(),
    };
    console.log(`perf-editor: ${machine.browser} on ${machine.cpu} (${machine.cores} cores, ${machine.memoryGiB} GiB)`);

    const results = [];
    let failed = false;
    let stopped = false;
    for (const scenario of scenarios) {
      const result = await runScenario(browser, base, scenario);
      results.push(result);
      printScenario(scenario, result);
      if (result.problems.length) failed = true;
      if (result.stopped) stopped = true;
    }

    const toggleLoad = os.loadavg()[0];
    const toggle = await runToggle(browser, base);
    const cold = [...toggle.cold, ...results.flatMap((result) => result.coldFirstPaintMs ?? [])];
    const coldP95 = percentile(cold, 95);
    const warmP95 = percentile(toggle.warm, 95);
    console.log(
      `\ntoggle (load average ${toggleLoad.toFixed(1)}): cold p50 ${formatValue(median(cold), "ms")} p95 ${formatValue(coldP95, "ms")} (budget ${TOGGLE_BUDGETS.coldP95Ms} ms, ${cold.length} samples); warm p50 ${formatValue(median(toggle.warm), "ms")} p95 ${formatValue(warmP95, "ms")} (budget ${TOGGLE_BUDGETS.warmP95Ms} ms, ${toggle.warm.length} samples)`,
    );
    const toggleProblems = [...toggle.problems];
    if (coldP95 === null || coldP95 > TOGGLE_BUDGETS.coldP95Ms) toggleProblems.push("cold first paint is over budget or unmeasured");
    if (warmP95 === null || warmP95 > TOGGLE_BUDGETS.warmP95Ms) toggleProblems.push("warm first paint is over budget or unmeasured");
    for (const problem of toggleProblems) console.log(`  problem: ${problem}`);
    if (toggleProblems.length) failed = true;

    const out = options.out ?? join(os.tmpdir(), `writ-perf-editor-${Date.now()}.json`);
    const record = {
      at: new Date().toISOString(),
      machine,
      results,
      toggle: { loadAverage: toggleLoad, cold, warm: toggle.warm, problems: toggleProblems },
    };
    await writeFile(out, `${JSON.stringify(record, null, 1)}\n`);
    console.log(`\nperf-editor: results in ${out}`);
    if (stopped) return 2;
    if (failed) {
      console.log("perf-editor: FAILED");
      return 1;
    }
    console.log("perf-editor: OK");
    return 0;
  } finally {
    await browser?.close().catch(() => undefined);
    server?.close();
    await rm(outDir, { recursive: true, force: true });
  }
}

main().then(
  (code) => {
    process.exitCode = code;
  },
  (error) => {
    console.error(`perf-editor: ${error instanceof GateSetupError ? error.message : (error?.stack ?? error)}`);
    process.exitCode = 2;
  },
);
