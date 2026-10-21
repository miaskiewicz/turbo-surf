// Shared authored localhost control for document factories, SRI reflection and XPath.
(() => {
  const result = {};
  const capture = (run) => {
    try {
      return run();
    } catch (caught) {
      return { error: caught.name, code: caught.code ?? null };
    }
  };
  const inert = new DOMParser().parseFromString(
    '<!doctype html><body><section id="scope"><p data-key="one">alpha</p><p data-key="two">beta<b>nested</b></p></section><p>outside</p>',
    "text/html",
  );
  result.factories = {};
  for (const tag of ["div", "input", "textarea", "script", "link", "iframe"]) {
    result.factories[tag] = capture(() => {
      const node = inert.createElement(tag);
      inert.body.append(node);
      return {
        owner: node.ownerDocument === inert,
        parent: node.parentNode === inert.body,
        interface: Object.prototype.toString.call(node),
      };
    });
  }
  result.tagConversion = capture(() => {
    let calls = 0;
    const node = inert.createElement({
      toString() {
        calls++;
        return "span";
      },
    });
    return { calls, owner: node.ownerDocument === inert, tag: node.localName };
  });
  result.integrity = {};
  for (const tag of ["script", "link"]) {
    const node = inert.createElement(tag);
    result.integrity[tag] = capture(() => {
      const initial = node.integrity;
      node.integrity = "sha256-example";
      const attribute = node.getAttribute("integrity");
      node.integrity = null;
      return { initial, attribute, nullValue: node.integrity };
    });
  }
  result.xpath = {};
  const scope = inert.getElementById("scope");
  const cases = [
    ["absolute", "/html/body/section/p", inert, 7],
    ["relative", "p", scope, 7],
    ["position", "p[2]", scope, 7],
    ["attribute", "p/@data-key", scope, 7],
    ["count", "count(p)", scope, 1],
    ["string", "string(p[2])", scope, 2],
    ["boolean", "boolean(p[@data-key='two'])", scope, 3],
    ["union", "p[1] | p[2]/b", scope, 7],
    ["axis", "p[2]/preceding-sibling::p", scope, 7],
    ["following", "p[1]/following::p", scope, 7],
    ["preceding", "p[2]/b/preceding::p", scope, 7],
    ["invalid", "p[", scope, 7],
    ["badType", "p", scope, 42],
  ];
  for (const [name, expression, context, type] of cases) {
    result.xpath[name] = capture(() => {
      const value = inert.evaluate(expression, context, null, type, null);
      if (value.resultType === 1) return value.numberValue;
      if (value.resultType === 2) return value.stringValue;
      if (value.resultType === 3) return value.booleanValue;
      return Array.from({ length: value.snapshotLength }, (_, index) => {
        const node = value.snapshotItem(index);
        return [node.nodeType, node.nodeName, node.textContent];
      });
    });
  }
  result.expression = capture(() => {
    const expression = inert.createExpression("p[1]", null);
    const value = expression.evaluate(scope, 9, null);
    return {
      name: value.singleNodeValue.localName,
      owner: value.singleNodeValue.ownerDocument === inert,
      native: Function.prototype.toString.call(expression.evaluate),
    };
  });
  result.resolver = capture(() => {
    const resolver = inert.createNSResolver(inert.documentElement);
    return {
      identity: resolver === inert.documentElement,
      html: resolver.lookupNamespaceURI(null),
    };
  });
  result.lifecycle = capture(() => {
    const iterator = inert.evaluate("p", scope, null, 5),
      snapshot = inert.evaluate("p", scope, null, 7),
      first = scope.firstChild;
    const before = iterator.invalidIteratorState;
    first.setAttribute("data-key", "changed");
    const after = iterator.invalidIteratorState;
    const error = capture(() => iterator.iterateNext());
    first.remove();
    return {
      before,
      after,
      error,
      stableSnapshot: snapshot.snapshotItem(0) === first,
      length: snapshot.snapshotLength,
    };
  });
  result.foreign = capture(() => {
    const frame = document.createElement("iframe");
    document.body.append(frame);
    try {
      const child = frame.contentWindow;
      child.document.body.innerHTML = "<p>child</p>";
      const expression = child.document.createExpression("//p"),
        childResult = expression.evaluate(child.document, 7);
      const value = XPathExpression.prototype.evaluate.call(expression, child.document, 7);
      const evaluator = new child.XPathEvaluator();
      return {
        resultRealm: value instanceof XPathResult,
        borrowedDocumentResultRealm:
          Document.prototype.evaluate.call(
            child.document,
            "//p",
            child.document,
            null,
            7,
          ) instanceof XPathResult,
        borrowedDocumentExpressionRealm:
          Document.prototype.createExpression.call(child.document, "//p") instanceof
          XPathExpression,
        parentDocumentForeignContextRealm:
          document.evaluate("//p", child.document, null, 7) instanceof XPathResult,
        borrowedEvaluatorExpressionRealm:
          XPathEvaluator.prototype.createExpression.call(evaluator, "//p") instanceof
          XPathExpression,
        borrowedEvaluatorResultRealm:
          XPathEvaluator.prototype.evaluate.call(
            evaluator,
            "//p",
            child.document,
            null,
            7,
          ) instanceof XPathResult,
        node: value.snapshotItem(0) === child.document.body.firstChild,
        borrowedResult:
          XPathResult.prototype.snapshotItem.call(childResult, 0) ===
          child.document.body.firstChild,
        borrowedEvaluator: XPathEvaluator.prototype.evaluate.call(
          evaluator,
          "count(//p)",
          child.document,
          null,
          1,
        ).numberValue,
        clone: capture(() => structuredClone(value)),
      };
    } finally {
      frame.remove();
    }
  });
  return result;
})();
