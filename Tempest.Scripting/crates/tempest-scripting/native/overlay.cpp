// The native half of the in-game UI: Dear ImGui on the game's own D3D9 or
// D3D11 device.
//
// Threads:
//   * The game thread builds frames (`tempest_overlay_build_frame`): Win32
//     backend NewFrame, ImGui::NewFrame, the Lua draw handlers, Render, then
//     a deep copy of the draw data is parked in `g_frame`.
//   * The render thread (whichever thread calls Present) initialises the
//     renderer backend on first use and draws the parked copy every Present.
//   Only the copy crosses threads; the ImGui context itself is touched by the
//   render thread solely for backend init/render, which never races NewFrame
//   because the font atlas is built once, before the first frame.
//
// The detours themselves live in Rust (retour); this file provides the
// vtable discovery (a throwaway device per API, like kiero) and the bodies.
#include <stdint.h>
#include <string.h>
#include <stdio.h>

#include "cimgui/imgui/imgui.h"
#include "cimgui/imgui/imgui_internal.h"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <d3d9.h>
#include <d3d11.h>
#include <dxgi.h>
#include "cimgui/imgui/backends/imgui_impl_win32.h"
#include "cimgui/imgui/backends/imgui_impl_dx9.h"
#include "cimgui/imgui/backends/imgui_impl_dx11.h"
extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);
#else
#include <mutex>
#endif

extern "C" {

typedef void (*tempest_log_fn)(const char* msg);

// ---------------------------------------------------------------------------
// Logging / asserts
// ---------------------------------------------------------------------------

static tempest_log_fn g_log = nullptr;

static void olog(const char* fmt, ...) {
    if (!g_log) return;
    char buf[512];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(buf, sizeof buf, fmt, ap);
    va_end(ap);
    g_log(buf);
}

void tempest_overlay_set_log(tempest_log_fn fn) { g_log = fn; }

void tempest_overlay_assert_failed(const char* expr, const char* file, int line) {
    const char* base = file;
    for (const char* p = file; *p; ++p)
        if (*p == '/' || *p == '\\') base = p + 1;
    olog("imgui assertion failed: %s (%s:%d)", expr, base, line);
}

// ---------------------------------------------------------------------------
// Lock + the parked frame
// ---------------------------------------------------------------------------

#ifdef _WIN32
struct Lock {
    SRWLOCK l = SRWLOCK_INIT;
    void lock() { AcquireSRWLockExclusive(&l); }
    void unlock() { ReleaseSRWLockExclusive(&l); }
};
#else
struct Lock {
    std::mutex m;
    void lock() { m.lock(); }
    void unlock() { m.unlock(); }
};
#endif

struct FrameCopy {
    ImDrawData data;  // CmdLists holds clones owned by this copy
};

static Lock g_frame_lock;
static FrameCopy* g_frame = nullptr;  // newest completed frame, owned here

static FrameCopy* clone_frame(const ImDrawData* src) {
    FrameCopy* c = IM_NEW(FrameCopy)();
    c->data = *src;
    for (int i = 0; i < c->data.CmdLists.Size; ++i) c->data.CmdLists[i] = src->CmdLists[i]->CloneOutput();
    return c;
}

static void free_frame(FrameCopy* c) {
    if (!c) return;
    for (int i = 0; i < c->data.CmdLists.Size; ++i) IM_DELETE(c->data.CmdLists[i]);
    IM_DELETE(c);
}

static void park_frame(FrameCopy* c) {
    g_frame_lock.lock();
    FrameCopy* old = g_frame;
    g_frame = c;
    g_frame_lock.unlock();
    free_frame(old);
}

// ---------------------------------------------------------------------------
// Context state
// ---------------------------------------------------------------------------

static volatile int g_ready = 0;      // backends initialised, font atlas built: frames may be built
static volatile int g_building = 0;   // a frame is being built (re-entrancy guard)
static volatile int g_capture = 0;    // the UI owns the mouse (software cursor, messages swallowed)
static float g_scale = 1.0f;
static const char* g_backend = "";
static void* g_hwnd = nullptr;
static unsigned g_frames_built = 0;
static unsigned g_frames_rendered = 0;

static void common_init(void* hwnd) {
    if (!ImGui::GetCurrentContext()) {
        ImGui::CreateContext();
        ImGuiIO& io = ImGui::GetIO();
        io.IniFilename = nullptr;  // no imgui.ini in the game folder
        io.LogFilename = nullptr;
        io.ConfigFlags |= ImGuiConfigFlags_NoMouseCursorChange;  // the game owns the OS cursor
        io.ConfigFlags &= ~ImGuiConfigFlags_NavEnableGamepad;
        io.ConfigErrorRecovery = true;
        io.ConfigErrorRecoveryEnableAssert = false;
        io.ConfigErrorRecoveryEnableDebugLog = false;
        io.ConfigErrorRecoveryEnableTooltip = true;
        ImGui::StyleColorsDark();
    }
#ifdef _WIN32
    if (hwnd && hwnd != g_hwnd) {
        if (g_hwnd) ImGui_ImplWin32_Shutdown();
        ImGui_ImplWin32_Init(hwnd);
        g_hwnd = hwnd;
    }
#else
    (void)hwnd;
#endif
}

const char* tempest_overlay_backend(void) { return g_backend; }
int tempest_overlay_ready(void) { return g_ready; }
void* tempest_overlay_hwnd(void) { return g_hwnd; }

void tempest_overlay_stats(unsigned* built, unsigned* rendered, int* vertices, int* lists) {
    if (built) *built = g_frames_built;
    if (rendered) *rendered = g_frames_rendered;
    g_frame_lock.lock();
    if (vertices) *vertices = g_frame ? g_frame->data.TotalVtxCount : 0;
    if (lists) *lists = g_frame ? g_frame->data.CmdListsCount : 0;
    g_frame_lock.unlock();
}

void tempest_overlay_set_capture(int on) {
    g_capture = on ? 1 : 0;
    if (ImGui::GetCurrentContext()) ImGui::GetIO().MouseDrawCursor = on != 0;
}

int tempest_overlay_get_capture(void) { return g_capture; }

void tempest_overlay_want(int* mouse, int* keyboard, int* text) {
    if (!ImGui::GetCurrentContext()) {
        if (mouse) *mouse = 0;
        if (keyboard) *keyboard = 0;
        if (text) *text = 0;
        return;
    }
    ImGuiIO& io = ImGui::GetIO();
    if (mouse) *mouse = io.WantCaptureMouse;
    if (keyboard) *keyboard = io.WantCaptureKeyboard;
    if (text) *text = io.WantTextInput;
}

// Scale fonts and paddings together, from the pristine dark style.
void tempest_overlay_set_scale(float s) {
    if (s <= 0.0f || !ImGui::GetCurrentContext()) return;
    g_scale = s;
    ImGuiStyle& style = ImGui::GetStyle();
    ImVec4 colors[ImGuiCol_COUNT];
    memcpy(colors, style.Colors, sizeof colors);
    style = ImGuiStyle();
    memcpy(style.Colors, colors, sizeof colors);
    style.ScaleAllSizes(s);
    ImGui::GetIO().FontGlobalScale = s;
}

float tempest_overlay_get_scale(void) { return g_scale; }

// ---------------------------------------------------------------------------
// Frame building (game thread)
// ---------------------------------------------------------------------------

// Run one draw handler under ImGui's error recovery: a handler that raised
// mid-window leaves Begin/End unbalanced, and recovery closes what it left.
int tempest_overlay_run_handler(int (*cb)(void*), void* ud) {
    ImGuiErrorRecoveryState st;
    ImGui::ErrorRecoveryStoreState(&st);
    int ok = cb(ud);
    if (!ok) ImGui::ErrorRecoveryTryToRecoverState(&st);
    return ok;
}

int tempest_overlay_build_frame(void (*draw)(void*), void* ud) {
    if (!g_ready || g_building) return 0;
    g_building = 1;
#ifdef _WIN32
    if (g_hwnd) {
        ImGui_ImplWin32_NewFrame();
    } else
#endif
    {
        ImGuiIO& io = ImGui::GetIO();
        if (io.DeltaTime <= 0.0f) io.DeltaTime = 1.0f / 60.0f;
    }
    ImGui::NewFrame();
    draw(ud);
    ImGui::Render();
    park_frame(clone_frame(ImGui::GetDrawData()));
    ++g_frames_built;
    g_building = 0;
    return 1;
}

// Host tests: a context with no backend. Frames build, nothing renders.
int tempest_overlay_init_headless(float w, float h) {
    common_init(nullptr);
    ImGuiIO& io = ImGui::GetIO();
    io.DisplaySize = ImVec2(w, h);
    io.DeltaTime = 1.0f / 60.0f;
    unsigned char* pixels = nullptr;
    int tw = 0, th = 0;
    io.Fonts->GetTexDataAsRGBA32(&pixels, &tw, &th);
    g_backend = "headless";
    g_ready = 1;
    return 1;
}

#ifdef _WIN32

// ---------------------------------------------------------------------------
// Window messages (game thread)
// ---------------------------------------------------------------------------

static bool is_mouse_message(UINT msg) {
    return (msg >= WM_MOUSEFIRST && msg <= WM_MOUSELAST) || msg == WM_NCMOUSEMOVE || msg == WM_MOUSELEAVE || msg == WM_MOUSEHOVER;
}

static bool is_key_message(UINT msg) {
    return msg == WM_KEYDOWN || msg == WM_KEYUP || msg == WM_SYSKEYDOWN || msg == WM_SYSKEYUP || msg == WM_CHAR || msg == WM_SYSCHAR || msg == WM_UNICHAR;
}

// Feed ImGui; returns 1 when the message must not reach the game.
int tempest_overlay_wndproc(void* hwnd, unsigned msg, uintptr_t wparam, intptr_t lparam) {
    if (!ImGui::GetCurrentContext() || !g_hwnd) return 0;
    ImGui_ImplWin32_WndProcHandler((HWND)hwnd, msg, (WPARAM)wparam, (LPARAM)lparam);
    if (!g_capture) return 0;
    if (is_mouse_message(msg)) return 1;
    if (is_key_message(msg) && ImGui::GetIO().WantCaptureKeyboard) return 1;
    return 0;
}

// ---------------------------------------------------------------------------
// Throwaway window for vtable discovery
// ---------------------------------------------------------------------------

struct ProbeWindow {
    HWND hwnd = nullptr;
    ProbeWindow() {
        WNDCLASSEXW wc{};
        wc.cbSize = sizeof wc;
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.lpfnWndProc = DefWindowProcW;
        wc.hInstance = GetModuleHandleW(nullptr);
        wc.lpszClassName = L"TempestOverlayProbe";
        RegisterClassExW(&wc);
        hwnd = CreateWindowExW(0, wc.lpszClassName, L"Tempest probe", WS_OVERLAPPEDWINDOW, 0, 0, 100, 100, nullptr, nullptr, wc.hInstance, nullptr);
    }
    ~ProbeWindow() {
        if (hwnd) DestroyWindow(hwnd);
        UnregisterClassW(L"TempestOverlayProbe", GetModuleHandleW(nullptr));
    }
};

// ---------------------------------------------------------------------------
// D3D11 / DXGI
// ---------------------------------------------------------------------------

static ID3D11Device* g_device11 = nullptr;
static ID3D11DeviceContext* g_context11 = nullptr;
static ID3D11RenderTargetView* g_rtv11 = nullptr;

// vtable slots of IDXGISwapChain: Present = 8, ResizeBuffers = 13.
int tempest_overlay_find_dxgi(void** present, void** resize) {
    HMODULE d3d11 = GetModuleHandleW(L"d3d11.dll");
    if (!d3d11) return 0;
    typedef HRESULT(WINAPI * CreateFn)(IDXGIAdapter*, D3D_DRIVER_TYPE, HMODULE, UINT, const D3D_FEATURE_LEVEL*, UINT, UINT, const DXGI_SWAP_CHAIN_DESC*, IDXGISwapChain**, ID3D11Device**, D3D_FEATURE_LEVEL*, ID3D11DeviceContext**);
    CreateFn create = (CreateFn)GetProcAddress(d3d11, "D3D11CreateDeviceAndSwapChain");
    if (!create) {
        olog("d3d11.dll has no D3D11CreateDeviceAndSwapChain");
        return 0;
    }
    ProbeWindow probe;
    if (!probe.hwnd) return 0;
    const D3D_FEATURE_LEVEL levels[] = {D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0};
    DXGI_SWAP_CHAIN_DESC desc{};
    desc.BufferDesc.Width = 100;
    desc.BufferDesc.Height = 100;
    desc.BufferDesc.RefreshRate.Numerator = 60;
    desc.BufferDesc.RefreshRate.Denominator = 1;
    desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.BufferCount = 1;
    desc.OutputWindow = probe.hwnd;
    desc.Windowed = TRUE;
    desc.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
    desc.Flags = DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH;
    IDXGISwapChain* sc = nullptr;
    ID3D11Device* dev = nullptr;
    ID3D11DeviceContext* ctx = nullptr;
    D3D_FEATURE_LEVEL got;
    HRESULT hr = create(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, levels, 3, D3D11_SDK_VERSION, &desc, &sc, &dev, &got, &ctx);
    if (FAILED(hr) || !sc) {
        olog("probe D3D11CreateDeviceAndSwapChain failed: 0x%08lx", (unsigned long)hr);
        return 0;
    }
    void** vt = *(void***)sc;
    *present = vt[8];
    *resize = vt[13];
    sc->Release();
    if (ctx) ctx->Release();
    if (dev) dev->Release();
    return 1;
}

static void release_rtv11() {
    if (g_rtv11) {
        g_rtv11->Release();
        g_rtv11 = nullptr;
    }
}

// Present body, before the original runs. `flags` with DXGI_PRESENT_TEST is
// an occlusion probe, not a frame.
void tempest_overlay_present11(void* swapchain_ptr, unsigned flags) {
    if (flags & DXGI_PRESENT_TEST) return;
    IDXGISwapChain* sc = (IDXGISwapChain*)swapchain_ptr;
    ID3D11Device* dev = nullptr;
    if (FAILED(sc->GetDevice(__uuidof(ID3D11Device), (void**)&dev)) || !dev) return;
    if (dev != g_device11) {
        if (g_device11) {
            olog("d3d11 device changed; re-initialising the renderer backend");
            ImGui_ImplDX11_Shutdown();
            release_rtv11();
            if (g_context11) g_context11->Release();
            g_context11 = nullptr;
        }
        DXGI_SWAP_CHAIN_DESC desc{};
        sc->GetDesc(&desc);
        common_init(desc.OutputWindow);
        dev->GetImmediateContext(&g_context11);
        ImGui_ImplDX11_Init(dev, g_context11);
        ImGui_ImplDX11_NewFrame();  // font texture + shaders, before any frame is built
        g_device11 = dev;
        g_backend = "d3d11";
        olog("d3d11 backend ready: device=%p window=%p %ux%u", (void*)dev, (void*)desc.OutputWindow, desc.BufferDesc.Width, desc.BufferDesc.Height);
        g_ready = 1;
    }
    dev->Release();
    ImGui_ImplDX11_NewFrame();
    g_frame_lock.lock();
    if (g_frame && g_frame->data.TotalVtxCount > 0) {
        if (!g_rtv11) {
            ID3D11Texture2D* bb = nullptr;
            if (SUCCEEDED(sc->GetBuffer(0, __uuidof(ID3D11Texture2D), (void**)&bb)) && bb) {
                g_device11->CreateRenderTargetView(bb, nullptr, &g_rtv11);
                bb->Release();
            }
        }
        if (g_rtv11) {
            ID3D11RenderTargetView* saved_rtv = nullptr;
            ID3D11DepthStencilView* saved_dsv = nullptr;
            g_context11->OMGetRenderTargets(1, &saved_rtv, &saved_dsv);
            g_context11->OMSetRenderTargets(1, &g_rtv11, nullptr);
            ImGui_ImplDX11_RenderDrawData(&g_frame->data);
            g_context11->OMSetRenderTargets(1, &saved_rtv, saved_dsv);
            if (saved_rtv) saved_rtv->Release();
            if (saved_dsv) saved_dsv->Release();
            ++g_frames_rendered;
        }
    }
    g_frame_lock.unlock();
}

void tempest_overlay_resize11(void) {
    g_frame_lock.lock();
    release_rtv11();
    g_frame_lock.unlock();
}

// ---------------------------------------------------------------------------
// D3D9
// ---------------------------------------------------------------------------

static IDirect3DDevice9* g_device9 = nullptr;

// vtable slots of IDirect3DDevice9: Reset = 16, Present = 17.
int tempest_overlay_find_d3d9(void** present, void** reset) {
    HMODULE d3d9 = GetModuleHandleW(L"d3d9.dll");
    if (!d3d9) return 0;
    typedef IDirect3D9*(WINAPI * CreateFn)(UINT);
    CreateFn create = (CreateFn)GetProcAddress(d3d9, "Direct3DCreate9");
    if (!create) {
        olog("d3d9.dll has no Direct3DCreate9");
        return 0;
    }
    ProbeWindow probe;
    if (!probe.hwnd) return 0;
    IDirect3D9* d3d = create(D3D_SDK_VERSION);
    if (!d3d) return 0;
    D3DDISPLAYMODE mode{};
    if (FAILED(d3d->GetAdapterDisplayMode(D3DADAPTER_DEFAULT, &mode))) {
        d3d->Release();
        return 0;
    }
    D3DPRESENT_PARAMETERS pp{};
    pp.BackBufferFormat = mode.Format;
    pp.SwapEffect = D3DSWAPEFFECT_DISCARD;
    pp.hDeviceWindow = probe.hwnd;
    pp.Windowed = TRUE;
    IDirect3DDevice9* dev = nullptr;
    HRESULT hr = d3d->CreateDevice(D3DADAPTER_DEFAULT, D3DDEVTYPE_HAL, probe.hwnd, D3DCREATE_SOFTWARE_VERTEXPROCESSING | D3DCREATE_DISABLE_DRIVER_MANAGEMENT, &pp, &dev);
    if (FAILED(hr) || !dev) {
        olog("probe IDirect3D9::CreateDevice failed: 0x%08lx", (unsigned long)hr);
        d3d->Release();
        return 0;
    }
    void** vt = *(void***)dev;
    *reset = vt[16];
    *present = vt[17];
    dev->Release();
    d3d->Release();
    return 1;
}

void tempest_overlay_present9(void* device_ptr, void* hwnd_fallback) {
    IDirect3DDevice9* dev = (IDirect3DDevice9*)device_ptr;
    if (dev != g_device9) {
        if (g_device9) {
            olog("d3d9 device changed; re-initialising the renderer backend");
            ImGui_ImplDX9_Shutdown();
        }
        D3DDEVICE_CREATION_PARAMETERS cp{};
        dev->GetCreationParameters(&cp);
        void* hwnd = cp.hFocusWindow ? (void*)cp.hFocusWindow : hwnd_fallback;
        common_init(hwnd);
        ImGui_ImplDX9_Init(dev);
        ImGui_ImplDX9_NewFrame();
        g_device9 = dev;
        g_backend = "d3d9";
        olog("d3d9 backend ready: device=%p window=%p", (void*)dev, hwnd);
        g_ready = 1;
    }
    ImGui_ImplDX9_NewFrame();
    g_frame_lock.lock();
    if (g_frame && g_frame->data.TotalVtxCount > 0) {
        IDirect3DSurface9* bb = nullptr;
        IDirect3DSurface9* saved = nullptr;
        if (SUCCEEDED(dev->GetBackBuffer(0, 0, D3DBACKBUFFER_TYPE_MONO, &bb)) && bb) {
            dev->GetRenderTarget(0, &saved);
            dev->SetRenderTarget(0, bb);
            if (SUCCEEDED(dev->BeginScene())) {
                ImGui_ImplDX9_RenderDrawData(&g_frame->data);
                dev->EndScene();
                ++g_frames_rendered;
            }
            if (saved) {
                dev->SetRenderTarget(0, saved);
                saved->Release();
            }
            bb->Release();
        }
    }
    g_frame_lock.unlock();
}

void tempest_overlay_reset9_before(void) {
    if (g_device9) ImGui_ImplDX9_InvalidateDeviceObjects();
}

#else  // !_WIN32: host build stubs so the Rust side links

int tempest_overlay_wndproc(void*, unsigned, uintptr_t, intptr_t) { return 0; }
int tempest_overlay_find_dxgi(void**, void**) { return 0; }
int tempest_overlay_find_d3d9(void**, void**) { return 0; }
void tempest_overlay_present11(void*, unsigned) {}
void tempest_overlay_resize11(void) {}
void tempest_overlay_present9(void*, void*) {}
void tempest_overlay_reset9_before(void) {}

#endif

}  // extern "C"
