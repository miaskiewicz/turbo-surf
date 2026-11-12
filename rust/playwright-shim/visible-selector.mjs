// The extended selector stays a query: evaluating visibility must not mutate the DOM.
// Kept self-contained because the engine compiles it in each document's realm.
export function queryVisibleSelector(root, selector, visible) {
  const split = (source, separator) => {
    const out = [];
    let start = 0;
    let depth = 0;
    let bracket = 0;
    let quote = "";
    for (let i = 0; i < source.length; i++) {
      const c = source[i];
      if (c === "\\") {
        i++;
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
  const sequence = (source) => {
    const tokens = [];
    let buffer = "";
    let depth = 0;
    let bracket = 0;
    let quote = "";
    let relation = " ";
    const flush = () => {
      if (buffer.trim()) {
        tokens.push({ css: buffer.trim(), relation });
        buffer = "";
        relation = " ";
      }
    };
    for (let i = 0; i < source.length; i++) {
      const c = source[i];
      if (c === "\\") {
        buffer += c + (source[++i] ?? "");
        continue;
      }
      if (quote) {
        buffer += c;
        if (c === quote) quote = "";
        continue;
      }
      if (c === '"' || c === "'") {
        quote = c;
        buffer += c;
        continue;
      }
      if (c === "[") bracket++;
      if (c === "]") bracket--;
      if (!bracket && c === "(") depth++;
      if (!bracket && c === ")") depth--;
      if (!depth && !bracket && /\s|[>+~]/.test(c)) {
        flush();
        if (!/\s/.test(c)) relation = c;
      } else buffer += c;
    }
    flush();
    return tokens;
  };
  const matches = (element, source, context) =>
    split(source, (c) => c === ",").some((branch) => {
      const tokens = sequence(branch.trim());
      const visit = (node, index) => {
        if (!node || !compound(node, tokens[index].css, context)) return false;
        if (index === 0) return node !== context || tokens[index].css.includes(":scope");
        const relation = tokens[index].relation;
        if (relation === ">") return visit(node.parentElement, index - 1);
        if (relation === "+") return visit(node.previousElementSibling, index - 1);
        if (relation === "~") {
          for (
            let sibling = node.previousElementSibling;
            sibling;
            sibling = sibling.previousElementSibling
          )
            if (visit(sibling, index - 1)) return true;
          return false;
        }
        for (let ancestor = node.parentElement; ancestor; ancestor = ancestor.parentElement)
          if (visit(ancestor, index - 1)) return true;
        return false;
      };
      return tokens.length > 0 && visit(element, tokens.length - 1);
    });
  const compound = (element, source, context) => {
    let css = "";
    const checks = [];
    let bracket = 0;
    let quote = "";
    for (let i = 0; i < source.length; i++) {
      const c = source[i];
      if (c === "\\") {
        css += c + (source[++i] ?? "");
        continue;
      }
      if (quote) {
        css += c;
        if (c === quote) quote = "";
        continue;
      }
      if (c === '"' || c === "'") {
        quote = c;
        css += c;
        continue;
      }
      if (c === "[") bracket++;
      if (c === "]") bracket--;
      if (!bracket && c === ":") {
        const pseudo = /^:(visible|scope|not|is|where|has)\b/.exec(source.slice(i));
        if (pseudo) {
          const name = pseudo[1];
          const end = i + pseudo[0].length;
          if (name === "visible" && source[end] !== "(") {
            checks.push(() => visible(element));
            i = end - 1;
            continue;
          }
          if (name === "scope" && source[end] !== "(") {
            checks.push(
              () => element === (context.nodeType === 9 ? context.documentElement : context),
            );
            i = end - 1;
            continue;
          }
          if (source[end] === "(") {
            let depth = 1;
            let j = end + 1;
            let innerQuote = "";
            for (; j < source.length && depth; j++) {
              const ch = source[j];
              if (ch === "\\") {
                j++;
                continue;
              }
              if (innerQuote) {
                if (ch === innerQuote) innerQuote = "";
                continue;
              }
              if (ch === '"' || ch === "'") {
                innerQuote = ch;
                continue;
              }
              if (ch === "(") depth++;
              if (ch === ")") depth--;
            }
            if (depth) throw new SyntaxError("Unclosed selector pseudo-class");
            const inner = source.slice(end + 1, j - 1);
            if (inner.includes(":visible") || inner.includes(":scope")) {
              checks.push(() =>
                name === "has"
                  ? query(
                      element,
                      split(inner, (ch) => ch === ",")
                        .map((part) => (/^\s*[>+~]/.test(part) ? `:scope ${part}` : part))
                        .join(","),
                    ).length > 0
                  : name === "not"
                    ? !matches(element, inner, context)
                    : matches(element, inner, context),
              );
              i = j - 1;
              continue;
            }
          }
        }
      }
      css += c;
    }
    return element.matches(css || "*") && checks.every((check) => check());
  };
  const query = (context, source) => {
    const siblings = split(source, (c) => c === ",").some((branch) =>
      /^\s*:scope\s*[+~]/.test(branch),
    );
    const container = siblings ? context.parentElement : context;
    return container
      ? Array.from(container.querySelectorAll("*")).filter((element) =>
          matches(element, source, context),
        )
      : [];
  };
  if (!selector.includes(":visible")) {
    const scoped =
      root.nodeType === 9
        ? selector
        : split(selector, (c) => c === ",")
            .map((branch) => (branch.includes(":scope") ? branch : `:scope ${branch}`))
            .join(",");
    return Array.from(root.querySelectorAll(scoped));
  }
  return query(root, selector);
}
