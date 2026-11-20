import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdir, mkdtemp, readFile, realpath, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, join, relative } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const exec = promisify(execFile);
const cli = fileURLToPath(new URL("./bin/turbo-surf-test.mjs", import.meta.url));

test("npm-style bin symlinks execute the CLI entry", async () => {
  const directory = await mkdtemp(join(tmpdir(), "surf-bin-test-"));
  try {
    const bin = join(directory, "turbo-surf-test");
    await symlink(cli, bin);
    const result = await exec(process.execPath, [bin, "--help"], { timeout: 10000 });
    assert.match(result.stdout, /usage: turbo-surf-test/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("CLI loads TS configs and extensionless spec imports with global hooks", async () => {
  const directory = await mkdtemp(join(tmpdir(), "surf-ts-cli-test-"));
  try {
    await mkdir(join(directory, "specs"));
    await writeFile(
      join(directory, "playwright.config.ts"),
      `import {resolve} from 'node:path';export default {testDir:resolve(__dirname,'specs'),testMatch:'*.spec.ts',globalSetup:require.resolve('./setup.ts'),globalTeardown:require.resolve('./teardown.ts')};`,
    );
    await writeFile(
      join(directory, "setup.ts"),
      `import {appendFileSync} from 'node:fs';export default async function(){appendFileSync(__dirname+'/order.txt','setup\\n');return ()=>appendFileSync(__dirname+'/order.txt','cleanup\\n')}`,
    );
    await writeFile(
      join(directory, "teardown.ts"),
      `import {appendFileSync} from 'node:fs';export default ()=>appendFileSync(__dirname+'/order.txt','teardown\\n');`,
    );
    await writeFile(join(directory, "specs/helper.ts"), `export const message: string='loaded';`);
    await writeFile(
      join(directory, "specs/example.spec.ts"),
      `import {test,expect} from '@playwright/test';import {appendFileSync} from 'node:fs';import {message} from './helper';test('typed spec',()=>{expect(message).toBe('loaded');appendFileSync(__dirname+'/../order.txt','body\\n')});`,
    );
    const env = { ...process.env };
    delete env.NODE_TEST_CONTEXT;
    delete env.NODE_OPTIONS;
    const result = await exec(
      process.execPath,
      [cli, "--config", join(directory, "playwright.config.ts")],
      { env, cwd: directory, timeout: 20000 },
    );
    assert.match(result.stdout, /typed spec/);
    assert.equal(
      await readFile(join(directory, "order.txt"), "utf8"),
      "setup\nbody\ncleanup\nteardown\n",
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("webServer stays available throughout global setup and teardown", async () => {
  const directory = await mkdtemp(join(tmpdir(), "surf-global-server-test-"));
  try {
    await mkdir(join(directory, "specs"));
    const quote = (value) => "'" + value.replaceAll("'", "'\\''") + "'";
    const command = [process.execPath, join(directory, "server.mjs")].map(quote).join(" ");
    const order = join(directory, "order.txt");
    await writeFile(
      join(directory, "server.mjs"),
      `import {createServer} from 'node:http';import {appendFileSync} from 'node:fs';appendFileSync(${JSON.stringify(order)},'server\\n');createServer((q,r)=>r.end('ready')).listen(Number(process.argv[2]),'127.0.0.1');`,
    );
    await writeFile(
      join(directory, "playwright.config.mjs"),
      `import {createServer} from 'node:net';const probe=createServer();await new Promise(r=>probe.listen(0,'127.0.0.1',r));const port=probe.address().port;await new Promise(r=>probe.close(r));const url='http://127.0.0.1:'+port;export default {testDir:'specs',testMatch:'*.spec.mjs',use:{baseURL:url},globalSetup:'./setup.mjs',globalTeardown:'./teardown.mjs',webServer:{command:${JSON.stringify(command)}+' '+port,url,timeout:3000,gracefulShutdown:{signal:'SIGTERM',timeout:100}}};`,
    );
    await writeFile(
      join(directory, "setup.mjs"),
      `import {appendFileSync} from 'node:fs';export default async c=>{await fetch(c.use.baseURL);appendFileSync(${JSON.stringify(order)},'setup\\n');return async()=>{await fetch(c.use.baseURL);appendFileSync(${JSON.stringify(order)},'cleanup\\n')}};`,
    );
    await writeFile(
      join(directory, "teardown.mjs"),
      `import {appendFileSync} from 'node:fs';export default async c=>{await fetch(c.use.baseURL);appendFileSync(${JSON.stringify(order)},'teardown\\n')};`,
    );
    await writeFile(
      join(directory, "specs/example.spec.mjs"),
      `import {test} from 'node:test';import {appendFileSync} from 'node:fs';test('body',()=>appendFileSync(${JSON.stringify(order)},'body\\n'));`,
    );
    const env = { ...process.env };
    delete env.NODE_TEST_CONTEXT;
    delete env.NODE_OPTIONS;
    await exec(process.execPath, [cli, "--config", join(directory, "playwright.config.mjs")], {
      env,
      cwd: directory,
      timeout: 15000,
    });
    assert.equal(await readFile(order, "utf8"), "server\nsetup\nbody\ncleanup\nteardown\n");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("CLI forwards explicit alias preloads through isolated attempts", async () => {
  const directory = await mkdtemp(join(tmpdir(), "surf-preload-cli-"));
  try {
    await mkdir(join(directory, "specs"));
    await writeFile(join(directory, "value.mjs"), "export const value = 42;");
    await writeFile(
      join(directory, "alias-loader.mjs"),
      `export function resolve(specifier, context, next) { return specifier === '@app/value' ? {url:new URL('./value.mjs', import.meta.url).href,shortCircuit:true} : next(specifier, context); }`,
    );
    await writeFile(
      join(directory, "preload.mjs"),
      `import {register} from 'node:module';register('./alias-loader.mjs',import.meta.url);`,
    );
    await writeFile(join(directory, "environment.cjs"), `globalThis.environmentPreload = true;`);
    await writeFile(
      join(directory, "playwright.config.mjs"),
      `export default {testDir: './specs',testMatch:'*.spec.mjs',globalSetup:'./setup.mjs',timeout:5000};`,
    );
    await writeFile(
      join(directory, "setup.mjs"),
      `import {value} from '@app/value';export default ()=>{if(value!==42) throw Error('setup alias');};`,
    );
    await writeFile(
      join(directory, "specs/alias.spec.mjs"),
      `import {test,expect} from '@playwright/test';import {value} from '@app/value';import {writeFileSync} from 'node:fs';test('alias in isolated attempt',()=>{expect(value).toBe(42);expect(globalThis.environmentPreload).toBe(true);expect(process.env.NODE_OPTIONS).toBe(undefined);writeFileSync(new URL('../body.txt',import.meta.url),'passed');});`,
    );
    const env = {
      ...process.env,
      NODE_OPTIONS: `--require "${join(directory, "environment.cjs")}"`,
    };
    delete env.NODE_TEST_CONTEXT;
    const result = await exec(
      process.execPath,
      [
        "--import",
        join(directory, "preload.mjs"),
        cli,
        "--config",
        join(directory, "playwright.config.mjs"),
        "--reporter",
        "tap",
      ],
      { cwd: directory, env, timeout: 20000 },
    );
    assert.match(result.stdout, /# pass 1/);
    assert.equal(await readFile(join(directory, "body.txt"), "utf8"), "passed");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

async function runArtifactCli(directory) {
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT;
  delete env.NODE_OPTIONS;
  return exec(process.execPath, [cli, "--config", join(directory, "playwright.config.mjs")], {
    env,
    cwd: directory,
    timeout: 20000,
  });
}

test("real CLI preserves same-named artifacts across files, projects and retries", async () => {
  const directory = await mkdtemp(join(tmpdir(), "surf-output-attempts-"));
  const artifactName = `${basename(directory)}-failure.json`;
  try {
    await mkdir(join(directory, "specs/left"), { recursive: true });
    await mkdir(join(directory, "specs/right"), { recursive: true });
    await writeFile(
      join(directory, "playwright.config.mjs"),
      `export default { testDir:'specs',testMatch:'**/*.spec.mjs',workers:2,retries:1,outputDir:'artifacts',projects:[{name:'alpha/a'},{name:'alpha-a',outputDir:'alternate'}] };`,
    );
    const manifest = join(directory, "attempts.jsonl");
    const source = `import {test} from '@playwright/test';
      import {writeFileSync,appendFileSync} from 'node:fs';import {fileURLToPath} from 'node:url';
      test.describe('shared suite',()=>{
        test.afterEach((_fixtures,info)=>{
          const data={actualFile:fileURLToPath(import.meta.url),file:info.file,project:info.project.name,retry:info.retry,titlePath:info.titlePath,outputDir:info.outputDir,status:info.status};
          const path=info.outputPath(${JSON.stringify(artifactName)});
          writeFileSync(path,JSON.stringify(data));
          appendFileSync(${JSON.stringify(manifest)},JSON.stringify({path,data})+'\\n');
        });
        test('same title',(_fixtures,info)=>{if(info.retry===0)throw Error('intentional first-attempt failure')});
      });`;
    for (const side of ["left", "right"])
      await writeFile(join(directory, "specs", side, "same.spec.mjs"), source);
    await runArtifactCli(directory);
    const rows = (await readFile(manifest, "utf8"))
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line));
    assert.equal(rows.length, 8);
    assert.equal(
      new Set(rows.map((row) => row.path)).size,
      8,
      "every real test/project/retry must retain its own artifact",
    );
    for (const row of rows) {
      assert.equal(basename(row.path), artifactName);
      const expectedRoot = join(
        directory,
        row.data.project === "alpha/a" ? "artifacts" : "alternate",
      );
      assert.equal(dirname(row.data.outputDir), expectedRoot);
      assert.equal(dirname(row.path), row.data.outputDir);
      assert.equal(row.data.file, row.data.actualFile);
      assert.deepEqual(row.data.titlePath, [
        relative(await realpath(join(directory, "specs")), row.data.actualFile),
        "shared suite",
        "same title",
      ]);
      assert.equal(row.data.status, row.data.retry === 0 ? "failed" : "passed");
      assert.deepEqual(JSON.parse(await readFile(row.path, "utf8")), row.data);
    }
    assert.equal(new Set(rows.map((row) => row.data.actualFile)).size, 2);
    assert.deepEqual([...new Set(rows.map((row) => row.data.retry))].sort(), [0, 1]);
  } finally {
    await rm(directory, { recursive: true, force: true });
    await rm(join("/tmp", artifactName), { force: true });
  }
});

test("real CLI defaults to contained test-results paths and rejects symlink escapes", async () => {
  const directory = await mkdtemp(join(tmpdir(), "surf-output-containment-"));
  try {
    await mkdir(join(directory, "specs"));
    await writeFile(
      join(directory, "playwright.config.mjs"),
      "export default {testDir:'specs',testMatch:'*.spec.mjs',workers:1};",
    );
    const report = join(directory, "result.json");
    await writeFile(
      join(directory, "specs/contained.spec.mjs"),
      `import {test} from '@playwright/test';import assert from 'node:assert/strict';import {writeFileSync,mkdirSync,symlinkSync,readdirSync} from 'node:fs';import {join} from 'node:path';
      test('contained output',(_fixtures,info)=>{
        const outside=${JSON.stringify(join(directory, "outside"))};mkdirSync(outside);
        assert.throws(()=>info.outputPath('..','escaped.json'),/output directory/i);
        assert.throws(()=>info.outputPath(outside,'absolute.json'),/output directory/i);
        assert.throws(()=>info.outputPath(info.outputDir+'-sibling','prefix.json'),/output directory/i);
        const nested=info.outputPath('nested','kept.json');writeFileSync(nested,'owned');
        assert.equal(info.outputPath(join(info.outputDir,'nested','kept.json')),nested);
        assert.equal(info.outputPath(),info.outputDir);
        symlinkSync(outside,info.outputPath('jump'),process.platform==='win32'?'junction':'dir');
        assert.throws(()=>info.outputPath('jump','escaped.json'),/symbolic link/i);
        assert.deepEqual(readdirSync(outside),[]);
        writeFileSync(${JSON.stringify(report)},JSON.stringify({outputDir:info.outputDir,nested,file:info.file}));
      });`,
    );
    await runArtifactCli(directory);
    const result = JSON.parse(await readFile(report, "utf8"));
    assert.equal(dirname(result.outputDir), join(directory, "test-results"));
    assert.equal(result.file, await realpath(join(directory, "specs/contained.spec.mjs")));
    assert.equal(await readFile(result.nested, "utf8"), "owned");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("real CLI forwards merged project settings to each isolated test process", async () => {
  const directory = await mkdtemp(join(tmpdir(), "surf-project-settings-"));
  try {
    await mkdir(join(directory, "specs"));
    const report = join(directory, "projects.jsonl");
    await writeFile(
      join(directory, "playwright.config.mjs"),
      `export default {testDir:'specs',testMatch:'*.spec.mjs',workers:2,timeout:4321,retries:1,outputDir:'artifacts',use:{baseURL:'http://base.example.test',locale:'en-GB',userAgent:'inherited-agent',testIdAttribute:'data-base-id'},projects:[{name:'inherited',workers:1},{name:'override',timeout:5432,retries:2,outputDir:'alternate',use:{locale:'fr-FR',userAgent:'override-agent',testIdAttribute:'data-project-id'}}]};`,
    );
    await writeFile(
      join(directory, "specs/settings.spec.mjs"),
      `import test from 'node:test';import {appendFileSync} from 'node:fs';
      test('actual project process',()=>appendFileSync(${JSON.stringify(report)},JSON.stringify({name:process.env.TURBO_PW_PROJECT_NAME,project:JSON.parse(process.env.TURBO_PW_PROJECT),execution:JSON.parse(process.env.TURBO_PW_EXECUTION),use:JSON.parse(process.env.TURBO_PW_USE),baseURL:process.env.TURBO_SHIM_BASE_URL,locale:process.env.TURBO_SHIM_LOCALE,userAgent:process.env.TURBO_SHIM_USER_AGENT,testIdAttribute:process.env.TURBO_SHIM_TESTID_ATTR,nodeOptions:process.env.NODE_OPTIONS,args:process.execArgv})+'\\n'));`,
    );
    await runArtifactCli(directory);
    const rows = (await readFile(report, "utf8")).trim().split("\n").map(JSON.parse);
    assert.deepEqual(
      rows.map((row) => row.name),
      ["inherited", "override"],
    );
    for (const row of rows) {
      const overridden = row.name === "override";
      const use = {
        baseURL: "http://base.example.test",
        locale: overridden ? "fr-FR" : "en-GB",
        userAgent: overridden ? "override-agent" : "inherited-agent",
        testIdAttribute: overridden ? "data-project-id" : "data-base-id",
      };
      assert.deepEqual(row.use, use);
      for (const [key, value] of Object.entries(use)) assert.equal(row[key], value);
      assert.deepEqual(row.project, {
        name: row.name,
        testDir: join(directory, "specs"),
        outputDir: join(directory, overridden ? "alternate" : "artifacts"),
      });
      assert.deepEqual(row.execution, {
        timeout: overridden ? 5432 : 4321,
        retries: overridden ? 2 : 1,
      });
      assert.equal(row.nodeOptions, undefined);
      assert.ok(row.args.includes(`--test-concurrency=${overridden ? 2 : 1}`));
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("real CLI runs global teardown when the setup cleanup rejects", async () => {
  const directory = await mkdtemp(join(tmpdir(), "surf-cleanup-rejection-"));
  try {
    await mkdir(join(directory, "specs"));
    const order = join(directory, "order.txt");
    await writeFile(
      join(directory, "playwright.config.mjs"),
      `export default {testDir:'specs',testMatch:'*.spec.mjs',workers:1,globalSetup:'./setup.mjs',globalTeardown:'./teardown.mjs'};`,
    );
    await writeFile(
      join(directory, "setup.mjs"),
      `import {appendFileSync} from 'node:fs';export default ()=>{appendFileSync(${JSON.stringify(order)},'setup\\n');return ()=>{appendFileSync(${JSON.stringify(order)},'cleanup\\n');throw Error('owned setup cleanup rejected')}};`,
    );
    await writeFile(
      join(directory, "teardown.mjs"),
      `import {appendFileSync} from 'node:fs';export default ()=>appendFileSync(${JSON.stringify(order)},'teardown\\n');`,
    );
    await writeFile(
      join(directory, "specs/body.spec.mjs"),
      `import test from 'node:test';import {appendFileSync} from 'node:fs';test('body completes',()=>appendFileSync(${JSON.stringify(order)},'body\\n'));`,
    );
    await assert.rejects(runArtifactCli(directory), (error) => {
      assert.equal(error.code, 1);
      assert.match(error.stdout, /body completes/);
      assert.match(error.stderr, /owned setup cleanup rejected/);
      return true;
    });
    assert.equal(await readFile(order, "utf8"), "setup\nbody\ncleanup\nteardown\n");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("real CLI HTML reports actual pass, failure and skip results with escaped names", async () => {
  const directory = await mkdtemp(join(tmpdir(), "surf-html-outcomes-"));
  try {
    await mkdir(join(directory, "specs"));
    const report = join(directory, "actual-results.html");
    await writeFile(
      join(directory, "playwright.config.mjs"),
      `export default {testDir:'specs',testMatch:'*.spec.mjs',workers:1,reporter:[['html',{outputFile:${JSON.stringify(report)}}]]};`,
    );
    await writeFile(
      join(directory, "specs/outcomes.spec.mjs"),
      `import test from 'node:test';test('passing <button>&',()=>{});test.skip('skipped <button>&',()=>{throw Error('skipped body ran')});test('failing <button>&',()=>{throw Error('expected child failure')});`,
    );
    await assert.rejects(runArtifactCli(directory), (error) => {
      assert.equal(error.code, 1);
      assert.match(error.stdout, /expected child failure/);
      return true;
    });
    const html = await readFile(report, "utf8");
    assert.match(html, /1 passed · 1 failed · 1 skipped/);
    for (const label of ["passing", "skipped", "failing"])
      assert.ok(html.includes(`${label} &lt;button&gt;&amp;`));
    assert.equal(html.includes("<button>"), false);
    assert.equal(html.includes("skipped body ran"), false);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
