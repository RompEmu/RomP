#include <stdio.h>
#include <stdarg.h>
#include <stdint.h>
#include <string.h>

uint32_t romp_core_log_min = 1;

void romp_core_log(uint32_t level, const char *fmt, ...) {
    if (!fmt || level < romp_core_log_min) return;
    char buf[4096];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    size_t len = strlen(buf);
    while (len > 0 && (buf[len - 1] == '\n' || buf[len - 1] == '\r')) {
        buf[--len] = '\0';
    }
    fprintf(stderr, "[core L%u] %s\n", level, buf);
}
