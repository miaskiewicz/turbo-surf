import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
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
