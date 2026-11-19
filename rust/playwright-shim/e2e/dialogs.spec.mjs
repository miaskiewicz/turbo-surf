import { expect, test } from "@playwright/test";

test("native dialogs suspend the handler and use host accept or dismiss results", async ({
  page,
}) => {
  const seen = [];
  page.on("dialog", async (dialog) => {
    seen.push([dialog.type(), dialog.message(), dialog.defaultValue(), dialog.page() === page]);
    if (dialog.type() === "confirm") await dialog.dismiss();
    else if (dialog.type() === "prompt") await dialog.accept("accepted-value");
    else await dialog.accept();
  });
  await page.setContent(
    `<button onclick="window.results={};results.confirm=confirm('confirm-marker');results.prompt=prompt('prompt-marker','initial');alert('alert-marker');document.querySelector('output').textContent=JSON.stringify(results)">Run</button><output>before</output>`,
  );
  await page.locator("button").click();
  await expect(page.locator("output")).toHaveText('{"confirm":false,"prompt":"accepted-value"}');
  expect(seen).toEqual([
    ["confirm", "confirm-marker", "", true],
    ["prompt", "prompt-marker", "initial", true],
    ["alert", "alert-marker", "", true],
  ]);
});

test("no listener automatically dismisses dialogs without suppressing later script", async ({
  page,
}) => {
  await page.setContent("<p>ready</p>");
  const values = await page.evaluate(() => {
    const confirmation = confirm("confirm");
    const answer = prompt("prompt", "initial");
    const alertResult = alert("alert");
    return [confirmation, answer, alertResult === undefined, "continued"];
  });
  expect(values).toEqual([false, null, true, "continued"]);
});

test("accepting a prompt uses empty replacement text when omitted or explicitly empty", async ({
  page,
}) => {
  page.once("dialog", (dialog) => dialog.accept());
  await page.setContent("<p>ready</p>");
  expect(await page.evaluate(() => prompt("first", "default-value"))).toBe("");
  page.once("dialog", (dialog) => dialog.accept(""));
  expect(await page.evaluate(() => prompt("second", "default-value"))).toBe("");
});

test("dialogs during initial document script execution reach the host before continuation", async ({
  page,
}) => {
  const seen = [];
  page.on("dialog", async (dialog) => {
    seen.push(dialog.message());
    await dialog.accept("from-host");
  });
  await page.setContent(
    "<output></output><script>document.querySelector('output').textContent=prompt('initial','initial-value')</script>",
  );
  await expect(page.locator("output")).toHaveText("from-host");
  expect(seen).toEqual(["initial"]);
});

test("closing a page releases a pending native dialog without an actor command deadlock", async ({
  page,
}) => {
  await page.setContent("<p>ready</p>");
  const opened = page.waitForEvent("dialog");
  const pending = page.evaluate(() => confirm("held")).catch((error) => error.message);
  const dialog = await opened;
  expect(dialog.message()).toBe("held");
  await page.close();
  expect(typeof (await pending)).toBe("string");
});

test("a native dialog cannot be handled twice", async ({ page }) => {
  let duplicate;
  page.on("dialog", async (dialog) => {
    await dialog.accept();
    duplicate = dialog.dismiss().then(
      () => "accepted twice",
      (error) => error.message,
    );
  });
  await page.setContent("<p>ready</p>");
  expect(await page.evaluate(() => confirm("once"))).toBe(true);
  expect(await duplicate).toMatch(/already.*handled|already.*completed/i);
});

test("dialog arguments use Web IDL strings and preserve conversion exceptions", async ({
  page,
}) => {
  const seen = [];
  page.on("dialog", (dialog) => {
    seen.push(dialog.message());
    return dialog.dismiss();
  });
  await page.setContent("<p>ready</p>");
  const values = await page.evaluate(() => {
    confirm({
      toString() {
        return "converted";
      },
    });
    confirm(null);
    try {
      confirm(Symbol("unconvertible"));
    } catch (error) {
      return error.name;
    }
    return "no exception";
  });
  expect(values).toBe("TypeError");
  expect(seen).toEqual(["converted", "null"]);
});

test("sandboxed frames without allow-modals auto-dismiss without a host event", async ({
  page,
}) => {
  const seen = [];
  page.on("dialog", (dialog) => {
    seen.push(dialog.message());
    return dialog.accept("host");
  });
  await page.setContent(
    `<iframe sandbox="allow-scripts allow-same-origin" srcdoc="&lt;script&gt;window.result=[confirm('blocked'),prompt('blocked','initial'),alert('blocked')===undefined];&lt;/script&gt;"></iframe>`,
  );
  await page.waitForFunction(() =>
    Array.isArray(document.querySelector("iframe").contentWindow.result),
  );
  expect(await page.evaluate(() => document.querySelector("iframe").contentWindow.result)).toEqual([
    false,
    null,
    true,
  ]);
  expect(seen).toEqual([]);
});

test("allow-modals frames deliver dialogs to their actual Page", async ({ page }) => {
  const owners = [];
  page.on("dialog", (dialog) => {
    owners.push(dialog.page() === page);
    return dialog.accept("host");
  });
  await page.setContent(
    `<iframe sandbox="allow-scripts allow-same-origin allow-modals" srcdoc="&lt;script&gt;window.result=prompt('allowed','initial');&lt;/script&gt;"></iframe>`,
  );
  await page.waitForFunction(
    () => document.querySelector("iframe").contentWindow.result === "host",
  );
  expect(owners).toEqual([true]);
});

test("dialog messages, defaults, and accepted prompt strings preserve original UTF-16", async ({
  page,
}) => {
  const seen = [];
  page.on("dialog", (dialog) => {
    seen.push([dialog.message().charCodeAt(0), dialog.defaultValue().charCodeAt(0)]);
    return dialog.accept("\udc00\ud800");
  });
  await page.setContent("<p>ready</p>");
  const units = await page.evaluate(() => {
    const result = prompt("\ud800", "\udc00");
    return [result.charCodeAt(0), result.charCodeAt(1)];
  });
  expect(seen).toEqual([[0xd800, 0xdc00]]);
  expect(units).toEqual([0xdc00, 0xd800]);
});
