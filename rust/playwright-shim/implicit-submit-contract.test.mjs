import assert from "node:assert/strict";
import test from "node:test";

let shim;
try {
  shim = await import(process.env.TURBO_SURF_TEST_SHIM || "./index.mjs");
} catch (error) {
  if (!String(error.message).includes("no prebuilt binary")) throw error;
  console.log("implicit submission: native addon not built, skipping");
  process.exit(0);
}

const cases = [
  {
    name: "first default submitter",
    fields:
      '<input id="focus"><button id="first">First</button><button id="second">Second</button>',
    clicks: ["first"],
    submits: ["first"],
  },
  {
    name: "disabled first default submitter",
    fields:
      '<input id="focus"><button id="first" disabled>First</button><button id="second">Second</button>',
    clicks: [],
    submits: [],
  },
  ...["checkbox", "radio", "range"].map((type) => ({
    name: `${type} uses default submitter`,
    fields: `<input id="focus" type="${type}"><button id="default">Submit</button>`,
    clicks: ["default"],
    submits: ["default"],
  })),
  {
    name: "canceled keydown",
    cancelKeydown: true,
    fields: '<input id="focus"><button id="default">Submit</button>',
    clicks: [],
    submits: [],
  },
  {
    name: "invalid required input",
    fields: '<input id="focus" required><button id="default">Submit</button>',
    clicks: ["default"],
    submits: [],
  },
  {
    name: "submitter formnovalidate bypasses required input",
    fields: '<input id="focus" required><button id="default" formnovalidate>Submit</button>',
    clicks: ["default"],
    submits: ["default"],
  },
];

for (const item of cases) {
  test(`implicit Enter: ${item.name}`, { timeout: 10000 }, async () => {
    const page = shim.newPage();
    try {
      await page.setContent(`<form>${item.fields}</form>`);
      await page.evaluate(
        ({ cancelKeydown }) => {
          globalThis.events = { clicks: [], submits: [], trusted: [] };
          document.addEventListener("click", (event) => {
            events.clicks.push(event.target.id);
            events.trusted.push(event.isTrusted);
          });
          if (cancelKeydown)
            document.addEventListener("keydown", (event) => event.preventDefault());
          document.querySelector("form").addEventListener("submit", (event) => {
            events.submits.push(event.submitter?.id ?? null);
            event.preventDefault();
          });
          document.querySelector("#focus").focus();
        },
        { cancelKeydown: item.cancelKeydown === true },
      );
      await page.keyboard.press("Enter");
      const actual = await page.evaluate(() => events);
      assert.deepEqual(actual.clicks, item.clicks);
      assert.deepEqual(actual.submits, item.submits);
      assert.ok(
        actual.trusted.every((trusted) => trusted === false),
        "shim input must not forge trusted events",
      );
    } finally {
      await page.close();
    }
  });
}
