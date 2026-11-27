import assert from "node:assert/strict";
import test from "node:test";

process.env.TURBO_SURF_ALLOWED_ORIGINS = "[]";
const { BrowserContext, chromium } = await import("./index.mjs");
const HTML = `<!doctype html><style>@keyframes spin{to{transform:rotate(90deg)}}
#target{width:40px;height:20px;transform:none;animation:spin 1s infinite}</style><div id=target></div>`;

test("animation presentation rejects unknown context modes", () => {
  assert.throws(() => new BrowserContext({ animations: "fast-forward" }), /animation/i);
});

test("context animation presentation reaches native geometry and preserves WAAPI", async () => {
  const context = new BrowserContext({ animations: "disabled" });
  try {
    const page = await context.newPage();
    await page.setContent(HTML);
    assert.deepEqual(
      await page.evaluate(() => {
        const target = document.getElementById("target");
        return [
          target.getBoundingClientRect().width,
          getComputedStyle(target).transform,
          typeof target.animate,
        ];
      }),
      [40, "none", "function"],
    );
  } finally {
    await context.close();
  }
});

test("browser newPage accepts an independent animation presentation choice", async () => {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({ animations: "disabled" });
    await page.setContent(HTML);
    assert.equal(await page.locator("#target").isVisible(), true);
    const enabled = await browser.newPage({ animations: "enabled" });
    await enabled.setContent(HTML);
    await assert.rejects(
      enabled.evaluate(() => document.getElementById("target").getBoundingClientRect().width),
      /animation resource/,
    );
    assert.equal(await page.locator("#target").isVisible(), true);
  } finally {
    await browser.close();
  }
});

async function animationReadinessFixture(animation, run, ancestor = false) {
  const context = new BrowserContext();
  try {
    const page = await context.newPage();
    await page.setContent(`<!doctype html><style>
      @keyframes entrance { from { transform:translateY(8px) } to { transform:translateY(0) } }
      button { width:120px; height:40px }
      </style><div id="parent"><button id="target">Open</button></div>`);
    await page.evaluate(
      ({ animation, ancestor }) => {
        globalThis.readinessClicks = 0;
        document.getElementById("target").addEventListener("click", () => readinessClicks++);
        document.getElementById(ancestor ? "parent" : "target").style.animation = animation;
      },
      { animation, ancestor },
    );
    await run(page, page.locator("#target"));
  } finally {
    await context.close();
  }
}

test("supported visibility waits and clicks never query authored animation metadata", async () => {
  await animationReadinessFixture("none", async (page, target) => {
    await page.evaluate(() => {
      globalThis.metadataReads = 0;
      Object.defineProperty(Element.prototype, "getAnimations", {
        configurable: true,
        get() {
          metadataReads++;
          throw new Error("authored-animation-metadata-must-not-run");
        },
      });
    });
    await target.waitFor({ state: "visible", timeout: 1000 });
    await target.click({ timeout: 1000 });
    assert.equal(await target.isVisible(), true);
    assert.equal(await page.evaluate(() => metadataReads), 0);
    assert.equal(await page.evaluate(() => readinessClicks), 1);
  });
});

test("visible waits observe finite animation completion while direct reads remain honest", async () => {
  await animationReadinessFixture("entrance 300ms linear", async (page, target) => {
    await assert.rejects(target.isVisible(), /animation resource.*entrance/);
    await target.waitFor({ state: "visible", timeout: 1000 });
    assert.equal(await target.isVisible(), true);
    assert.equal(
      await page.evaluate(() => document.getElementById("target").getAnimations().length),
      0,
    );
  });
});

test("actionability waits for finite ancestor animation without duplicate clicks", async () => {
  await animationReadinessFixture(
    "entrance 300ms linear",
    async (page, target) => {
      await target.click({ timeout: 1000 });
      assert.equal(await page.evaluate(() => readinessClicks), 1);
      assert.equal(
        await page.evaluate(() => document.getElementById("parent").getAnimations().length),
        0,
      );
    },
    true,
  );
});

test("finite animation readiness never extends wait or action caller deadlines", async () => {
  await animationReadinessFixture("entrance 3s linear", async (page, target) => {
    await assert.rejects(target.waitFor({ state: "visible", timeout: 40 }), /waitFor.*timed out/);
    await assert.rejects(target.waitFor({ state: "hidden", timeout: 40 }), /waitFor.*timed out/);
    await assert.rejects(target.click({ timeout: 40 }), (error) => error.name === "TimeoutError");
    await assert.rejects(target.waitFor({ state: "visible", timeout: 0 }), /waitFor.*timed out/);
    assert.equal(await page.evaluate(() => readinessClicks), 0);
    assert.equal(
      await page.evaluate(() => document.getElementById("target").getAnimations()[0].playState),
      "running",
    );
  });
});

test("infinite paused and filling animation geometry remains unsupported in waiters", async () => {
  for (const animation of [
    "entrance 100ms linear infinite",
    "entrance 300ms linear paused",
    "entrance 100ms linear forwards",
    "entrance 100ms linear both",
  ]) {
    await animationReadinessFixture(animation, async (page, target) => {
      await assert.rejects(
        target.waitFor({ state: "visible", timeout: 1000 }),
        /animation resource.*entrance/,
      );
      await assert.rejects(target.click({ timeout: 1000 }), /animation resource.*entrance/);
      assert.equal(await page.evaluate(() => readinessClicks), 0);
    });
  }
});

test("waiters do not retry arbitrary geometry exceptions", async () => {
  await animationReadinessFixture("none", async (page, target) => {
    await page.evaluate(() => {
      document.getElementById("target").getBoundingClientRect = () => {
        throw new Error("unrelated-readiness-marker");
      };
    });
    await assert.rejects(
      target.waitFor({ state: "visible", timeout: 1000 }),
      /unrelated-readiness-marker/,
    );
    await assert.rejects(target.click({ timeout: 1000 }), /unrelated-readiness-marker/);
    assert.equal(await page.evaluate(() => readinessClicks), 0);
  });
});

test("dispatch guard failures are never retried after actionability succeeds", async () => {
  await animationReadinessFixture("none", async (page, target) => {
    await page.evaluate(() => {
      document.getElementById("target").innerHTML = "<span>Left</span><span>Right</span>";
      const nativeHit = document.elementFromPoint;
      globalThis.dispatchHitReads = 0;
      document.elementFromPoint = function (...args) {
        if (++dispatchHitReads === 2)
          throw new Error("native live layout animation resource is unsupported: entrance");
        return nativeHit.apply(this, args);
      };
    });
    assert.equal(await target.isVisible(), true);
    await assert.rejects(target.click({ timeout: 1000 }), /animation resource.*entrance/);
    assert.equal(await page.evaluate(() => dispatchHitReads), 2);
    assert.equal(await page.evaluate(() => readinessClicks), 0);
  });
});

test("attachment and detachment waits do not require animation geometry", async () => {
  await animationReadinessFixture("entrance 100ms linear infinite", async (_page, target) => {
    await target.waitFor({ state: "attached", timeout: 0 });
    await target.evaluate((element) => element.remove());
    await target.waitFor({ state: "detached", timeout: 0 });
    await target.waitFor({ state: "hidden", timeout: 0 });
    await assert.rejects(target.waitFor({ state: "visible", timeout: 0 }), /waitFor.*timed out/);
  });
});

test("waiter error policy preserves session-budget failure identity", async (context) => {
  await animationReadinessFixture("none", async (page, target) => {
    const failure = new Error("Execution interrupted by the session execution budget");
    let reads = 0;
    const read = context.mock.method(page, "_readEngineAsync", async () => {
      reads++;
      throw failure;
    });
    await assert.rejects(
      target.waitFor({ state: "visible", timeout: 1000 }),
      (error) => error === failure,
    );
    await assert.rejects(target.click({ timeout: 1000 }), (error) => error === failure);
    assert.equal(reads, 2);
    read.mock.restore();
    assert.equal(await page.evaluate(() => readinessClicks), 0);
  });
});

test("animation error names without a relevant finite effect never start waiter polling", async (context) => {
  await animationReadinessFixture("none", async (page, target) => {
    await page.evaluate(() => {
      globalThis.unprovenReads = 0;
      document.getElementById("target").getBoundingClientRect = () => {
        unprovenReads++;
        throw new Error("native live layout animation resource is unsupported: unrelated");
      };
    });
    let hostPolls = 0;
    const original = page._redrain.bind(page);
    const pump = context.mock.method(page, "_redrain", async () => {
      hostPolls++;
      return original();
    });
    await assert.rejects(
      target.waitFor({ state: "visible", timeout: 1000 }),
      /unsupported: unrelated/,
    );
    await assert.rejects(target.click({ timeout: 1000 }), /unsupported: unrelated/);
    assert.equal(hostPolls, 0);
    pump.mock.restore();
    const reads = await page.evaluate(() => unprovenReads);
    assert.ok(reads >= 2 && reads <= 4, "only a bounded same-task revalidation is allowed");
    assert.equal(await page.evaluate(() => readinessClicks), 0);
  });
});

test("a classified first guard can revalidate supported geometry without a lingering effect", async () => {
  await animationReadinessFixture("none", async (page, target) => {
    await page.evaluate(() => {
      globalThis.completedGeometryReads = 0;
      const element = document.getElementById("target");
      const nativeRect = element.getBoundingClientRect;
      element.getBoundingClientRect = function () {
        if (++completedGeometryReads === 1)
          throw new Error("native live layout animation resource is unsupported: completed");
        return nativeRect.call(this);
      };
    });
    await target.waitFor({ state: "visible", timeout: 1000 });
    assert.equal(await page.evaluate(() => completedGeometryReads), 2);
    assert.equal(await page.evaluate(() => readinessClicks), 0);
  });
});

test("animation completion before read delivery cannot lose the coherent readiness proof", async (context) => {
  await animationReadinessFixture("entrance 300ms linear", async (page, target) => {
    const original = page._readEngineAsync.bind(page);
    let delayed = false;
    context.mock.method(page, "_readEngineAsync", async (source) => {
      const result = await original(source);
      if (
        !delayed &&
        source.includes("return {value:") &&
        source.includes("getBoundingClientRect")
      ) {
        delayed = true;
        await page.waitForFunction(
          () => document.getElementById("target").getAnimations().length === 0,
          null,
          { timeout: 1000 },
        );
      }
      return result;
    });
    await target.click({ timeout: 1000 });
    assert.equal(delayed, true);
    assert.equal(await target.isVisible(), true);
    assert.equal(await page.evaluate(() => readinessClicks), 1);
  });
});

test("same-name replacement with an infinite effect is not finite readiness", async () => {
  await animationReadinessFixture("entrance 3s linear", async (page, target) => {
    await page.evaluate(() => {
      const element = document.getElementById("target");
      const nativeRect = element.getBoundingClientRect;
      let replaced = false;
      element.getBoundingClientRect = function () {
        if (!replaced) {
          replaced = true;
          const old = element.getAnimations()[0];
          element.style.animation = "none";
          element.getAnimations();
          element.style.animation = "entrance 100ms linear infinite";
          globalThis.replacedAnimationIdentity = old !== element.getAnimations()[0];
        }
        return nativeRect.call(this);
      };
    });
    await assert.rejects(
      target.waitFor({ state: "visible", timeout: 1000 }),
      /animation resource.*entrance/,
    );
    assert.equal(await page.evaluate(() => replacedAnimationIdentity), true);
    assert.equal(await page.evaluate(() => readinessClicks), 0);
  });
});

test("branched click waits for a proven finite hit-test animation before dispatch", async () => {
  await animationReadinessFixture("none", async (page, target) => {
    await page.evaluate(() => {
      document.getElementById("target").innerHTML = "<span>Left</span><span>Right</span>";
      const ripple = document.createElement("span");
      ripple.style.cssText =
        "position:absolute;left:500px;top:300px;width:40px;height:40px;pointer-events:none;animation:entrance 300ms linear";
      document.body.append(ripple);
    });
    assert.equal(await target.isVisible(), true);
    await target.click({ timeout: 1000 });
    assert.equal(await page.evaluate(() => readinessClicks), 1);
  });
});

test("finite hit-test readiness stays within the click deadline", async () => {
  await animationReadinessFixture("none", async (page, target) => {
    await page.evaluate(() => {
      document.getElementById("target").innerHTML = "<span>Left</span><span>Right</span>";
      const ripple = document.createElement("span");
      ripple.style.cssText =
        "position:absolute;left:500px;top:300px;width:40px;height:40px;pointer-events:none;animation:entrance 3s linear";
      document.body.append(ripple);
    });
    await assert.rejects(target.click({ timeout: 40 }), (error) => error.name === "TimeoutError");
    assert.equal(await page.evaluate(() => readinessClicks), 0);
  });
});

test("hit-test readiness refuses infinite paused and filling transparent effects", async () => {
  for (const animation of [
    "entrance 1s linear infinite",
    "entrance 1s linear paused",
    "entrance 1s linear forwards",
  ]) {
    await animationReadinessFixture("none", async (page, target) => {
      await page.evaluate((animation) => {
        document.getElementById("target").innerHTML = "<span>Left</span><span>Right</span>";
        const ripple = document.createElement("span");
        ripple.style.cssText =
          "position:absolute;left:500px;top:300px;width:40px;height:40px;pointer-events:none";
        ripple.style.animation = animation;
        document.body.append(ripple);
      }, animation);
      await assert.rejects(target.click({ timeout: 1000 }), /animation resource.*entrance/);
      assert.equal(await page.evaluate(() => readinessClicks), 0);
    });
  }
});

test("hit-test preflight preserves arbitrary failures without input dispatch", async () => {
  await animationReadinessFixture("none", async (page, target) => {
    await page.evaluate(() => {
      document.getElementById("target").innerHTML = "<span>Left</span><span>Right</span>";
      document.elementFromPoint = () => {
        throw new Error("hit-test-preflight-marker");
      };
    });
    await assert.rejects(target.click({ timeout: 1000 }), /hit-test-preflight-marker/);
    assert.equal(await page.evaluate(() => readinessClicks), 0);
  });
});

test("supported branched clicks do not inspect document animations", async () => {
  await animationReadinessFixture("none", async (page, target) => {
    await page.evaluate(() => {
      document.getElementById("target").innerHTML = "<span>Left</span><span>Right</span>";
      globalThis.documentAnimationReads = 0;
      document.getAnimations = () => {
        documentAnimationReads++;
        throw new Error("unexpected animation metadata");
      };
    });
    await target.click({ timeout: 1000 });
    assert.equal(await page.evaluate(() => documentAnimationReads), 0);
    assert.equal(await page.evaluate(() => readinessClicks), 1);
  });
});

async function dependentGeometryFixture(animation, pointerEvents, run) {
  await animationReadinessFixture("none", async (page, target) => {
    await page.evaluate(
      ({ animation, pointerEvents }) => {
        const ripple = document.createElement("span");
        ripple.id = "dependent-ripple";
        ripple.style.cssText = "position:absolute;width:40px;height:40px;left:500px;top:300px";
        ripple.style.pointerEvents = pointerEvents;
        ripple.style.animation = animation;
        document.body.append(ripple);
        const nativeRect = Element.prototype.getBoundingClientRect;
        document.getElementById("target").getBoundingClientRect = function () {
          nativeRect.call(ripple);
          return nativeRect.call(this);
        };
      },
      { animation, pointerEvents },
    );
    await run(page, target);
  });
}

test("visible waits preserve a real native dependency on a finite transparent animation", async () => {
  await dependentGeometryFixture("entrance 300ms linear", "none", async (page, target) => {
    await assert.rejects(target.isVisible(), /animation resource.*entrance/);
    await target.waitFor({ state: "visible", timeout: 1000 });
    assert.equal(await target.isVisible(), true);
    await target.click({ timeout: 1000 });
    assert.equal(await page.evaluate(() => readinessClicks), 1);
  });
});

test("dependent visibility readiness preserves the caller deadline without input", async () => {
  await dependentGeometryFixture("entrance 3s linear", "none", async (page, target) => {
    await assert.rejects(target.waitFor({ state: "visible", timeout: 40 }), (error) => {
      assert.equal(error.constructor, Error);
      assert.equal(error.message, "turbo-surf: waitFor(state=visible) timed out");
      return true;
    });
    assert.equal(await page.evaluate(() => readinessClicks), 0);
  });
});

test("dependent visibility still rejects interactive paused infinite and filling effects", async () => {
  for (const [animation, pointerEvents] of [
    ["entrance 1s linear", "auto"],
    ["entrance 1s linear paused", "none"],
    ["entrance 1s linear infinite", "none"],
    ["entrance 1s linear forwards", "none"],
  ]) {
    await dependentGeometryFixture(animation, pointerEvents, async (page, target) => {
      await assert.rejects(
        target.waitFor({ state: "visible", timeout: 1000 }),
        /animation resource/,
      );
      assert.equal(await page.evaluate(() => readinessClicks), 0);
    });
  }
});
