// Spike S5 (P0-T12) — minimal Windows test program for the Wine engine.
// Console app: no GUI, so it runs headless in a Linux VM/CI runner.
#include <windows.h>
#include <stdio.h>

int main(void)
{
    LARGE_INTEGER freq, t0, t1;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&t0);

    printf("S5_HELLO_FROM_WINE\n");
    fflush(stdout);

    QueryPerformanceCounter(&t1);
    printf("S5_SELF_MS: %ld\n", (long)((t1.QuadPart - t0.QuadPart) * 1000 / freq.QuadPart));
    return 0;
}
