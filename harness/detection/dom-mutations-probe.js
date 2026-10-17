// Shared localhost mutation oracle. No search-engine requests or cookie imports.
(() => {
  const result = {};
  const error = (run) => {
    try {
      run();
      return null;
    } catch (caught) {
      return {
        name: caught.name,
        code: caught.code ?? null,
        domException: caught instanceof DOMException,
      };
    }
  };
  const child = (tag) => document.createElement(tag);
  const seams = {
    appendChild: (parent, node) => node.appendChild(parent),
    insertBefore: (parent, node) => node.insertBefore(parent, null),
    append: (parent, node) => node.append("prefix", parent),
    prepend: (parent, node) => node.prepend(parent, "suffix"),
    replaceChildren: (parent, node) => node.replaceChildren("prefix", parent),
    before: (parent, node) => node.before("prefix", parent),
    after: (parent, node) => node.after(parent),
    replaceWith: (parent, node) => node.replaceWith(parent),
    adjacentInside: (parent, node) => node.insertAdjacentElement("beforeend", parent),
    adjacentOutside: (parent, node) => node.insertAdjacentElement("beforebegin", parent),
    replaceChild: (parent, node) => parent.replaceChild(parent, node),
  };
  result.cycles = {};
  for (const [name, run] of Object.entries(seams)) {
    const parent = child("section"),
      node = child("div");
    parent.appendChild(node);
    node.textContent = "original";
    result.cycles[name] = {
      error: error(() => run(parent, node)),
      parentType: parent.parentNode?.nodeType ?? null,
      children: parent.innerHTML,
      childParentPreserved: node.parentNode === parent,
    };
  }
  const parent = child("div"),
    a = child("a"),
    b = child("b"),
    c = child("i");
  parent.append(a, b, c);
  parent.insertBefore(b, b);
  result.selfReference = parent.innerHTML;
  parent.prepend("head", c);
  parent.append("tail");
  result.stringOrder = parent.innerHTML;
  parent.replaceChildren(a, b, a);
  result.repeatedArguments = parent.innerHTML;
  a.before(a, "before");
  a.after(a, "after");
  result.viableSiblings = parent.innerHTML;
  a.replaceWith(a);
  result.selfReplacement = parent.innerHTML;
  const fragment = document.createDocumentFragment();
  fragment.append("one", c, "two");
  result.fragmentReplacement = {
    error: error(() => parent.replaceChild(fragment, a)),
    html: parent.innerHTML,
    fragmentChildren: fragment.childNodes.length,
    oldDetached: a.parentNode === null,
  };
  const outsider = child("aside");
  result.notFoundRemoval = {
    error: error(() => parent.removeChild(outsider)),
    html: parent.innerHTML,
    detached: outsider.parentNode === null,
  };
  const host = child("section"),
    shadow = host.attachShadow({ mode: "open" }),
    inside = child("span");
  shadow.appendChild(inside);
  result.shadowCycle = {
    error: error(() => inside.appendChild(host)),
    hostDetached: host.parentNode === null,
    childPreserved: inside.parentNode === shadow,
  };
  result.brands = [null, undefined, 42, "text", {}, Object.create(Node.prototype)].map((value) =>
    error(() => parent.appendChild(value)),
  );
  result.methodShape = [Node.prototype.replaceChild.length, Node.prototype.insertBefore.length];
  const root = document.documentElement,
    doctype = document.doctype;
  result.document = {
    extraRoot: error(() => document.appendChild(child("extra"))),
    text: error(() => document.appendChild(document.createTextNode("text"))),
    wrongOrder: error(() => document.insertBefore(root, doctype)),
    outerHTMLPolicy: error(() => root.insertAdjacentHTML("beforebegin", "<extra></extra>")),
  };
  const replacement = child("html");
  result.document.replacement = error(() => document.replaceChild(replacement, root));
  result.document.identity = document.documentElement === replacement && root.parentNode === null;
  document.replaceChild(root, replacement);
  const encoded = JSON.stringify(result);
  globalThis.__RESULT = encoded;
  return encoded;
})();
