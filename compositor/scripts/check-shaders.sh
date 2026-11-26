#!/usr/bin/env bash
# Compile-check the glass shader sources (P3-T05) without a GPU device.
#
# The shaders are GLSL ES 1.00 fragment stages (`#version 100` is added by
# smithay, `//_DEFINES_` is replaced with its variant defines). This script
# strips the ES-only `precision` qualifiers and the define marker, then:
#
# - on macOS, compiles and links each fragment stage against the smithay
#   texture vertex shader with the system OpenGL framework (desktop GLSL 1.20,
#   which shares the `varying` / `texture2D` syntax used here);
# - on Linux, uses `glslangValidator -S frag` when it is installed;
# - otherwise reports SKIP (it never fails the build for a missing tool).
#
# It validates syntax, declarations and types. It does *not* prove the shaders
# run on the target GPU: see compositor/README.md, "Hardware-gated validation".
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
shaders=(
  "$root/src/shaders/glass_downsample.frag"
  "$root/src/shaders/glass_upsample.frag"
  "$root/src/shaders/glass_composite.frag"
)

for shader in "${shaders[@]}"; do
  [ -f "$shader" ] || { echo "missing shader: $shader" >&2; exit 1; }
done

if [ "$(uname -s)" = "Darwin" ]; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  cat > "$tmp/main.c" <<'EOF'
/* Desktop-GLSL compile/link check for the AetherOS glass fragment shaders. */
#include <OpenGL/gl.h>
#include <OpenGL/OpenGL.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static char *slurp(const char *path) {
  FILE *f = fopen(path, "rb");
  if (!f) { perror(path); exit(2); }
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  char *buf = calloc(1, n + 1);
  if (fread(buf, 1, n, f) != (size_t)n) { exit(2); }
  fclose(f);
  return buf;
}

static char *desktopize(const char *src) {
  char *out = calloc(1, strlen(src) + 128);
  strcat(out, "#version 120\n");
  char *copy = strdup(src);
  char *line = strtok(copy, "\n");
  while (line) {
    char *trim = line;
    while (*trim == ' ' || *trim == '\t') { trim++; }
    if (strncmp(trim, "precision ", 10) != 0) {
      strcat(out, line);
      strcat(out, "\n");
    }
    line = strtok(NULL, "\n");
  }
  free(copy);
  return out;
}

static GLuint compile(GLenum type, const char *src, const char *name) {
  GLuint shader = glCreateShader(type);
  glShaderSource(shader, 1, &src, NULL);
  glCompileShader(shader);
  GLint ok = 0;
  glGetShaderiv(shader, GL_COMPILE_STATUS, &ok);
  char log[8192] = {0};
  glGetShaderInfoLog(shader, sizeof(log) - 1, NULL, log);
  printf("%-44s %s%s%s\n", name, ok ? "OK" : "FAIL", log[0] ? " :: " : "", log);
  if (!ok) { exit(1); }
  return shader;
}

int main(int argc, char **argv) {
  CGLPixelFormatAttribute attrs[] = {kCGLPFAAccelerated, (CGLPixelFormatAttribute)0};
  CGLPixelFormatObj pix;
  GLint npix = 0;
  if (CGLChoosePixelFormat(attrs, &pix, &npix) != kCGLNoError || !pix) {
    CGLPixelFormatAttribute soft[] = {(CGLPixelFormatAttribute)0};
    CGLChoosePixelFormat(soft, &pix, &npix);
  }
  CGLContextObj ctx;
  CGLCreateContext(pix, NULL, &ctx);
  CGLSetCurrentContext(ctx);

  /* The vertex stage smithay's `compile_custom_texture_shader` links against. */
  const char *vert =
      "#version 120\n"
      "uniform mat3 matrix;\nuniform mat3 tex_matrix;\n"
      "attribute vec2 vert;\nattribute vec4 vert_position;\n"
      "varying vec2 v_coords;\n"
      "void main() { vec3 p = vec3(vert, 1.0);"
      " v_coords = (tex_matrix * p).xy;"
      " gl_Position = vec4(matrix * p, 1.0); }\n";

  int failures = 0;
  for (int i = 1; i < argc; i++) {
    char *src = desktopize(slurp(argv[i]));
    GLuint vs = compile(GL_VERTEX_SHADER, vert, "texture.vert (smithay)");
    GLuint fs = compile(GL_FRAGMENT_SHADER, src, argv[i]);
    GLuint prog = glCreateProgram();
    glAttachShader(prog, vs);
    glAttachShader(prog, fs);
    glBindAttribLocation(prog, 0, "vert");
    glLinkProgram(prog);
    GLint ok = 0;
    glGetProgramiv(prog, GL_LINK_STATUS, &ok);
    char log[8192] = {0};
    glGetProgramInfoLog(prog, sizeof(log) - 1, NULL, log);
    printf("%-44s %s%s%s\n", "  link", ok ? "OK" : "FAIL", log[0] ? " :: " : "", log);
    if (!ok) { failures++; }
    free(src);
  }
  printf(failures ? "RESULT: %d failure(s)\n" : "RESULT: all glass shaders compile\n", failures);
  return failures ? 1 : 0;
}
EOF
  clang -o "$tmp/check" "$tmp/main.c" -framework OpenGL -Wno-deprecated-declarations
  "$tmp/check" "${shaders[@]}"
elif command -v glslangValidator >/dev/null 2>&1; then
  for shader in "${shaders[@]}"; do
    # glslang wants `#version 100` and the `//_DEFINES_` marker resolved.
    tmp_shader="$(mktemp)"
    { echo "#version 100"; sed -e 's|^//_DEFINES_$||' "$shader"; } > "$tmp_shader"
    glslangValidator -S frag "$tmp_shader" || exit 1
    rm -f "$tmp_shader"
  done
  echo "RESULT: all glass shaders compile"
else
  echo "SKIP: no shader compiler found (macOS OpenGL framework or glslangValidator)"
fi
