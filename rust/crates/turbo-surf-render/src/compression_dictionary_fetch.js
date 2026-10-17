// Chrome retains these author headers in Headers/Request, but refuses a manual
// Available-Dictionary at network admission, including an exact stored hash.
// Core's own dictionary negotiation runs later and never enters this guest path.
const __dictionaryHeaderKeys = Object.keys;
const __dictionaryHeaderLower = String.prototype.toLowerCase;
function __rejectManualDictionaryHeader(headers) {
  if (headers == null) return;
  const names = __dictionaryHeaderKeys(headers);
  for (let index = 0; index < names.length; index++) {
    if (__baseApply(__dictionaryHeaderLower, names[index], []) === "available-dictionary")
      throw new __exceptionTypeError("Failed to fetch");
  }
}
