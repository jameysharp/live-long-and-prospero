#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern void x(float *x_out);
extern void y(float *unused, float *y_out);
extern void xy(const float *x_in, const float *y_in, float *xy_out);

extern const uint16_t x_size;
extern const uint16_t y_size;
extern const uint16_t xy_size;

int main(int argc, char **argv) {
  unsigned long size = 512;
  if(argc > 1) {
    char *end = NULL;
    size = strtoul(argv[1], &end, 0);
    if(*end != '\0') {
      fprintf(stderr, "usage: %s [size]\n", argv[0]);
      exit(EXIT_FAILURE);
    }
  }

  float *x_buf = malloc(sizeof(float) * x_size * size);
  float *y_buf = malloc(sizeof(float) * y_size);
  assert(xy_size == 1);
  float scale = 2.0 / (size - 1);

  for(unsigned col = 0; col < size; ++col) {
    float *x_span = x_buf + col * x_size;
    x_span[0] = col * scale - 1.0;
    x(x_span);
  }

  printf("P4 %ld %ld\n", size, size);
  size_t row_size = (size + 7) / 8;
  uint8_t *row_buffer = malloc(row_size);

  for(unsigned row = 0; row < size; ++row) {
    memset(row_buffer, 0, row_size);

    y_buf[0] = -(row * scale - 1.0);
    y(NULL, y_buf);

    for(unsigned col = 0; col < size; ++col) {
      float *x_span = x_buf + col * x_size;
      float result;
      xy(x_span, y_buf, &result);
      if(result >= 0.0) {
        row_buffer[col >> 3] |= 0x80 >> (col & 7);
      }
    }

    fwrite(row_buffer, 1, row_size, stdout);
  }

  exit(EXIT_SUCCESS);
}
