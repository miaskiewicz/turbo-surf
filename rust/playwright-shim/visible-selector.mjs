// The extended selector stays a query: evaluating visibility must not mutate the DOM.
// Kept self-contained because the engine compiles it in each document's realm.
export function queryVisibleSelector(root, selector, visible, dom) {
  const parent = dom ? dom.parent : (element) => element.parentElement;
  const previous = dom ? dom.previous : (element) => element.previousElementSibling;
  const candidates = dom ? dom.all : (element) => Array.from(element.querySelectorAll("*"));
  const nativeMatches = dom ? dom.matches : (element, source) => element.matches(source);
  const isComment = (source, index) => source[index] === "/" && source[index + 1] === "*";
  const commentEnd = (source, index) => {
    const end = source.indexOf("*/", index + 2);
    return end < 0 ? source.length - 1 : end + 1;
  };
  const escapeEnd = (source, start) => {
    let end = start + 1;
    if (!/[\da-f]/i.test(source[end] || "")) return end;
    for (let count = 0; count < 6 && /[\da-f]/i.test(source[end] || ""); count++) end++;
    if (source[end] === "\r" && source[end + 1] === "\n") return end + 1;
    return "\t\n\f\r ".includes(source[end]) ? end : end - 1;
  };
  const split = (source, separator) => {
    const out = [];
    let start = 0;
    let depth = 0;
    let bracket = 0;
    let quote = "";
    for (let i = 0; i < source.length; i++) {
      const c = source[i];
      if (c === "\\") {
        i = escapeEnd(source, i);
        continue;
      }
      if (quote) {
        if (c === quote) quote = "";
        continue;
      }
      if (c === '"' || c === "'") {
        quote = c;
        continue;
      }
      if (isComment(source, i)) {
        i = commentEnd(source, i);
        continue;
      }
      if (c === "[") bracket++;
      if (c === "]") bracket--;
      if (bracket) continue;
      if (c === "(") depth++;
      if (c === ")") depth--;
      if (!depth && separator(c)) {
        out.push(source.slice(start, i));
        start = i + 1;
      }
    }
    out.push(source.slice(start));
    return out;
  };
  const quotedEnd = (source, start) => {
    for (let index = start + 1; index < source.length; index++) {
      if (source[index] === "\\") index = escapeEnd(source, index);
      else if (source[index] === source[start]) return index;
    }
    return source.length - 1;
  };
  const sequence = (source) => {
    const tokens = [];
    let buffer = "";
    let depth = 0;
    let bracket = 0;
    let relation = " ";
    const flush = () => {
      if (!buffer.trim()) return;
      tokens.push({ css: buffer.trim(), relation });
      buffer = "";
      relation = " ";
    };
    for (let i = 0; i < source.length; i++) {
      const c = source[i];
      if (c === "\\") {
        const end = escapeEnd(source, i);
        buffer += source.slice(i, end + 1);
        i = end;
        continue;
      }
      if (isComment(source, i)) {
        i = commentEnd(source, i);
        continue;
      }
      if (c === '"' || c === "'") {
        const end = quotedEnd(source, i);
        buffer += source.slice(i, end + 1);
        i = end;
        continue;
      }
      if (c === "[") bracket++;
      if (c === "]") bracket--;
      if (!bracket) depth += c === "(" ? 1 : c === ")" ? -1 : 0;
      if (!depth && !bracket && /\s|[>+~]/.test(c)) {
        flush();
        if (!/\s/.test(c)) relation = c;
      } else buffer += c;
    }
    flush();
    return tokens;
  };
  const matches = (element, source, context, forgiving = false) =>
    split(source, (c) => c === ",").some((branch) => {
      const tokens = sequence(branch.trim());
      const visit = (node, index) => {
        if (!node || !compound(node, tokens[index].css, context)) return false;
        if (index === 0)
          return (
            tokens[index].css.includes(":scope") ||
            (node !== context && (!dom || dom.contains(context, node)))
          );
        const relation = tokens[index].relation;
        if (relation === ">") return visit(parent(node), index - 1);
        if (relation === "+") return visit(previous(node), index - 1);
        if (relation === "~") {
          for (let sibling = previous(node); sibling; sibling = previous(sibling))
            if (visit(sibling, index - 1)) return true;
          return false;
        }
        for (let ancestor = parent(node); ancestor; ancestor = parent(ancestor))
          if (visit(ancestor, index - 1)) return true;
        return false;
      };
      try {
        return tokens.length > 0 && visit(element, tokens.length - 1);
      } catch (error) {
        if (forgiving && error.name === "SyntaxError") return false;
        throw error;
      }
    });
  const simplePseudo = (name) => name === "visible" || name === "scope";
  const readPseudo = (source, start) => {
    const match = /^:(visible|scope|not|is|where|has)\b/.exec(source.slice(start));
    if (!match) return null;
    const name = match[1];
    const end = start + match[0].length;
    if (source[end] !== "(") return simplePseudo(name) ? { name, inner: null, end: end - 1 } : null;
    if (simplePseudo(name)) return null;
    let depth = 1;
    let index = end + 1;
    for (; index < source.length && depth; index++) {
      const c = source[index];
      if (c === "\\") index = escapeEnd(source, index);
      else if (isComment(source, index)) index = commentEnd(source, index);
      else if (c === '"' || c === "'") index = quotedEnd(source, index);
      else if (c === "(") depth++;
      else if (c === ")") depth--;
    }
    if (depth) throw new SyntaxError("Unclosed selector pseudo-class");
    return { name, inner: source.slice(end + 1, index - 1), end: index - 1 };
  };
  const pseudoCheck = (element, pseudo, context) => {
    const { name, inner } = pseudo;
    if (name === "visible") return () => visible(element);
    if (name === "scope")
      return () => element === (context.nodeType === 9 ? context.documentElement : context);
    if (name === "has")
      return () =>
        query(
          element,
          split(inner, (ch) => ch === ",")
            .map((part) => (/^\s*[>+~]/.test(part) ? `:scope ${part}` : part))
            .join(","),
        ).length > 0;
    return () =>
      name === "not" ? !matches(element, inner, context) : matches(element, inner, context, true);
  };
  const handlesPseudo = (pseudo) =>
    pseudo &&
    (dom ||
      pseudo.inner === null ||
      pseudo.inner.includes(":visible") ||
      pseudo.inner.includes(":scope"));
  const compound = (element, source, context) => {
    let css = "";
    const checks = [];
    let bracket = 0;
    for (let i = 0; i < source.length; i++) {
      const c = source[i];
      if (c === "\\") {
        const end = escapeEnd(source, i);
        css += source.slice(i, end + 1);
        i = end;
        continue;
      }
      if (isComment(source, i)) {
        i = commentEnd(source, i);
        continue;
      }
      if (c === '"' || c === "'") {
        const end = quotedEnd(source, i);
        css += source.slice(i, end + 1);
        i = end;
        continue;
      }
      if (c === "[") bracket++;
      if (c === "]") bracket--;
      const pseudo = !bracket && c === ":" ? readPseudo(source, i) : null;
      if (handlesPseudo(pseudo)) {
        checks.push(pseudoCheck(element, pseudo, context));
        i = pseudo.end;
      } else css += c;
    }
    return nativeMatches(element, css || "*") && checks.every((check) => check());
  };
  const query = (context, source) => {
    const siblings = split(source, (c) => c === ",").some((branch) =>
      /^\s*:scope\s*[+~]/.test(branch),
    );
    const container = siblings ? parent(context) : context;
    return container
      ? candidates(container).filter((element) => matches(element, source, context))
      : [];
  };
  if (!dom && !selector.includes(":visible")) {
    const scoped =
      root.nodeType === 9
        ? selector
        : split(selector, (c) => c === ",")
            .map((branch) => (branch.includes(":scope") ? branch : `:scope ${branch}`))
            .join(",");
    return Array.from(root.querySelectorAll(scoped));
  }
  if (dom && !selector.includes(":visible")) dom.validate(root, selector);
  return query(root, selector);
}
