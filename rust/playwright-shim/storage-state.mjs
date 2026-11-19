function codeUnits(value) {
  if (typeof value !== "string") throw new Error("Storage entry names and values must be strings");
  return Array.from({ length: value.length }, (_, index) => value.charCodeAt(index));
}

function fromUnits(units) {
  const chunks = [];
  for (let offset = 0; offset < units.length; offset += 8192) {
    chunks.push(String.fromCharCode(...units.slice(offset, offset + 8192)));
  }
  return chunks.join("");
}

function mapOrigins(origins, convert) {
  return origins.map(({ origin, localStorage }) => ({
    origin,
    localStorage: localStorage.map(({ name, value }) => ({
      name: convert(name),
      value: convert(value),
    })),
  }));
}

export function encodeStorageOrigins(origins) {
  return JSON.stringify(mapOrigins(origins, codeUnits));
}

export function decodeStorageOrigins(json) {
  return mapOrigins(JSON.parse(json), fromUnits);
}
