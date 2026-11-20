// Spike S5 GUI (P0-T12 residual): Windows GUI test program for the Wine engine.
// Creates a top-level window, paints a solid color, and keeps it visible for a
// few seconds so the "window appears under Wine" path is exercised. Console
// variant (spike-s5-hello.c) is the headless baseline.
#include <windows.h>
#include <stdio.h>

static const char CLASS_NAME[] = "AetherSpikeS5Gui";

static LRESULT CALLBACK WndProc(HWND hwnd, UINT msg, WPARAM wparam, LPARAM lparam)
{
    switch (msg) {
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC hdc = BeginPaint(hwnd, &ps);
        RECT r;
        GetClientRect(hwnd, &r);
        HBRUSH b = CreateSolidBrush(RGB(0x10, 0x18, 0x28));
        FillRect(hdc, &r, b);
        DeleteObject(b);
        SetTextColor(hdc, RGB(0x00, 0xff, 0x88));
        SetBkMode(hdc, TRANSPARENT);
        DrawTextA(hdc, "AetherOS Wine GUI spike", -1, &r,
                  DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        EndPaint(hwnd, &ps);
        return 0;
    }
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcA(hwnd, msg, wparam, lparam);
}

int WINAPI WinMain(HINSTANCE hinst, HINSTANCE prev, LPSTR cmdline, int show)
{
    (void)prev; (void)cmdline;

    WNDCLASSA wc = {0};
    wc.lpfnWndProc = WndProc;
    wc.hInstance = hinst;
    wc.lpszClassName = CLASS_NAME;
    wc.hbrBackground = (HBRUSH)(COLOR_WINDOW + 1);
    if (!RegisterClassA(&wc)) {
        fprintf(stderr, "RegisterClass failed: %lu\n", (unsigned long)GetLastError());
        return 1;
    }

    LARGE_INTEGER freq, t0;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&t0);

    HWND hwnd = CreateWindowExA(0, CLASS_NAME, "AetherOS spike S5 GUI",
                                WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT,
                                480, 320, NULL, NULL, hinst, NULL);
    if (!hwnd) {
        fprintf(stderr, "CreateWindow failed: %lu\n", (unsigned long)GetLastError());
        return 1;
    }

    LARGE_INTEGER t1;
    QueryPerformanceCounter(&t1);
    printf("S5_GUI_WINDOW_CREATED_MS: %ld\n",
           (long)((t1.QuadPart - t0.QuadPart) * 1000 / freq.QuadPart));
    fflush(stdout);

    ShowWindow(hwnd, show);
    UpdateWindow(hwnd);

    // Keep the window alive briefly so a compositor/Wine can present it.
    MSG msg;
    DWORD start = GetTickCount();
    while (GetTickCount() - start < 3000) {
        while (PeekMessageA(&msg, NULL, 0, 0, PM_REMOVE)) {
            if (msg.message == WM_QUIT) goto done;
            TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }
        Sleep(10);
    }
done:
    DestroyWindow(hwnd);
    printf("S5_GUI_DONE\n");
    fflush(stdout);
    return 0;
}
