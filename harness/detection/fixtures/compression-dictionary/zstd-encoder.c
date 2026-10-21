/* Independent fixture producer using the official Zstandard C encoder API. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <zstd.h>

static unsigned char *load(const char *path, size_t *length) {
  FILE *file = fopen(path, "rb");
  if (!file || fseek(file, 0, SEEK_END)) exit(2);
  long size = ftell(file);
  if (size < 0 || size > 16 * 1024 * 1024 || fseek(file, 0, SEEK_SET)) exit(2);
  *length = (size_t)size;
  unsigned char *bytes = malloc(*length + 1);
  if (!bytes || fread(bytes, 1, *length, file) != *length) exit(2);
  fclose(file);
  return bytes;
}
int main(int argc, char **argv) {
  if (argc != 4) return 2;
  size_t dictionary_length, input_length;
  unsigned char *dictionary = load(argv[1], &dictionary_length);
  unsigned char *input = load(argv[2], &input_length);
  size_t capacity = ZSTD_compressBound(input_length);
  unsigned char *output = malloc(capacity), *without = malloc(input_length + 1);
  ZSTD_CCtx *context = ZSTD_createCCtx();
  if (!output || !without || !context) return 2;
  size_t length = ZSTD_compress_usingDict(context, output, capacity, input,
      input_length, dictionary, dictionary_length, 9);
  if (ZSTD_isError(length)) return 2;
  size_t ordinary = ZSTD_decompress(without, input_length, output, length);
  if (!ZSTD_isError(ordinary) && ordinary == input_length &&
      !memcmp(without, input, input_length)) return 3;
  FILE *file = fopen(argv[3], "wb");
  if (!file || fwrite(output, 1, length, file) != length || fclose(file)) return 2;
  printf("%s\n", ZSTD_versionString());
  ZSTD_freeCCtx(context);
  free(dictionary); free(input); free(output); free(without);
  return 0;
}
