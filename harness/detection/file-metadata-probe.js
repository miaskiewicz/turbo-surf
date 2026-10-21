(() => {
  const descriptor = Object.getOwnPropertyDescriptor(File.prototype, "name");
  const result = {
    nameDescriptor: {
      enumerable: descriptor.enumerable,
      configurable: descriptor.configurable,
      set: typeof descriptor.set,
    },
    cases: [],
  };
  for (const [name, opts] of [
    [null, { lastModified: 0 }],
    ["a\ud800b/é", { lastModified: 5.9 }],
    ["f", { lastModified: -1.9 }],
    ["f", { lastModified: Infinity }],
    ["f", { lastModified: 2 ** 64 }],
  ]) {
    const file = new File([new Uint8Array([0, 128, 255])], name, opts);
    result.cases.push({
      name: file.name,
      lastModified: file.lastModified,
      relativePath: file.webkitRelativePath,
      own: Object.keys(file),
      tag: Object.prototype.toString.call(file),
      size: file.size,
    });
  }
  try {
    descriptor.get.call(new Blob([]));
  } catch (error) {
    result.illegalBrand = error.name;
  }
  return result;
})();
