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
