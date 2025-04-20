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
extern const uint16_t stride;

static size_t next_stride(size_t size) {
  return (size + (stride - 1)) & ~(stride - 1);
}

static void init_stride(float *buf, float start, float scale) {
  for(uint16_t i = 0; i < stride; ++i) {
    buf[i] = start;
    start += scale;
  }
}

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

  size_t alignment = sizeof(float) * stride;
  float *x_buf = aligned_alloc(alignment, sizeof(float) * x_size * next_stride(size));
  float *y_buf = aligned_alloc(alignment, sizeof(float) * y_size * stride);
  assert(xy_size == 1);
  float *xy_buf = aligned_alloc(alignment, sizeof(float) * xy_size * stride);

  float scale = 2.0f / (size - 1);

  for(unsigned long col = 0UL; col < size; col += stride) {
    float *x_span = x_buf + col * x_size;
    init_stride(x_span, col * scale - 1.0f, scale);
    x(x_span);
  }

  printf("P4 %ld %ld\n", size, size);
  size_t row_size = (size + 7) / 8;
  uint8_t *row_buffer = malloc(row_size);

  for(unsigned long row = 0UL; row < size; row += stride) {
    init_stride(y_buf, -(row * scale - 1.0f), -scale);
    y(NULL, y_buf);

    for(unsigned long i = 0UL; i < stride; ++i) {
      memset(row_buffer, 0, row_size);

      for(unsigned long col = 0UL; col < size; col += stride) {
        float *x_span = x_buf + col * x_size;
        xy(x_span, y_buf + i, xy_buf);
        for(unsigned long j = 0; j < stride; ++j) {
          if(xy_buf[j] >= 0.0f) {
            row_buffer[(col + j) >> 3] |= 0x80 >> ((col + j) & 7);
          }
        }
      }

      fwrite(row_buffer, 1, row_size, stdout);
    }
  }

  exit(EXIT_SUCCESS);
}
