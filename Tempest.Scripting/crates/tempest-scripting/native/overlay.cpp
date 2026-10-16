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

#define STB_IMAGE_IMPLEMENTATION
#define STBI_NO_STDIO_DISABLED
#define STBI_ONLY_PNG
#define STBI_ONLY_JPEG
#define STBI_ONLY_BMP
#define STBI_ONLY_TGA
#define STBI_NO_FAILURE_STRINGS
#include "stb_image.h"

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
static void apply_scale(float s);
static char g_ini_path[1024] = {0};
static volatile int g_fonts_dirty = 0;  // atlas changed: the render thread rebuilds the font texture

static const char* g_backend = "";
static void* g_hwnd = nullptr;
static unsigned g_frames_built = 0;
static unsigned g_frames_rendered = 0;

static void common_init(void* hwnd) {
    if (!ImGui::GetCurrentContext()) {
        ImGui::CreateContext();
        ImGuiIO& io = ImGui::GetIO();
        io.IniFilename = g_ini_path[0] ? g_ini_path : nullptr;  // layout file, set by the runtime settings
        io.LogFilename = nullptr;
        io.ConfigFlags |= ImGuiConfigFlags_NoMouseCursorChange;  // the game owns the OS cursor
        io.ConfigFlags &= ~ImGuiConfigFlags_NavEnableGamepad;
        io.ConfigErrorRecovery = true;
        io.ConfigErrorRecoveryEnableAssert = false;
        io.ConfigErrorRecoveryEnableDebugLog = false;
        io.ConfigErrorRecoveryEnableTooltip = true;
        ImGui::StyleColorsDark();
        if (g_scale != 1.0f) apply_scale(g_scale);
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

// Where ImGui persists window positions (empty = nowhere). Before the context exists.
void tempest_overlay_set_ini(const char* path) {
    if (!path) path = "";
    strncpy(g_ini_path, path, sizeof g_ini_path - 1);
    g_ini_path[sizeof g_ini_path - 1] = 0;
    if (ImGui::GetCurrentContext()) ImGui::GetIO().IniFilename = g_ini_path[0] ? g_ini_path : nullptr;
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

// Scale fonts and paddings together, from the pristine dark style. Before
// the context exists the value is kept and applied at init.
static void apply_scale(float s) {
    ImGuiStyle& style = ImGui::GetStyle();
    ImVec4 colors[ImGuiCol_COUNT];
    memcpy(colors, style.Colors, sizeof colors);
    style = ImGuiStyle();
    memcpy(style.Colors, colors, sizeof colors);
    style.ScaleAllSizes(s);
    ImGui::GetIO().FontGlobalScale = s;
}

void tempest_overlay_set_scale(float s) {
    if (s <= 0.0f) return;
    g_scale = s;
    if (ImGui::GetCurrentContext()) apply_scale(s);
}

float tempest_overlay_get_scale(void) { return g_scale; }

// ---------------------------------------------------------------------------
// Fonts (game thread adds, render thread rebuilds the texture)
// ---------------------------------------------------------------------------

struct FontRequest {
    char path[1024];
    float size;
    int merge;
    int use_default;
    ImVector<ImWchar>* ranges;  // leaked on purpose: the atlas keeps the pointer
    ImFont* result;
};

static ImVector<FontRequest*> g_font_requests;  // added from inside a frame: applied when it ends

static ImFont* apply_font(FontRequest* r) {
    ImGuiIO& io = ImGui::GetIO();
    ImFontConfig cfg;
    cfg.MergeMode = r->merge != 0;
    ImFont* f = nullptr;
    if (r->use_default) {
        cfg.SizePixels = r->size;
        f = io.Fonts->AddFontDefault(&cfg);
    } else {
        f = io.Fonts->AddFontFromFileTTF(r->path, r->size, &cfg, r->ranges ? r->ranges->Data : nullptr);
    }
    if (!f) olog("font %s (%.0f px) could not be loaded", r->use_default ? "<default>" : r->path, r->size);
    g_fonts_dirty = 1;
    return f;
}

// ui.add_font: a TTF (or the built-in font when `path` is empty) at `size`
// pixels; `ranges` is a 0-terminated list of (first, last) pairs, or NULL.
// Returns the ImFont* (valid once the atlas was rebuilt), NULL on failure,
// or the request handle's future font when called inside a draw handler.
void* tempest_overlay_add_font(const char* path, float size, int merge, const unsigned short* ranges) {
    if (!ImGui::GetCurrentContext()) {
        olog("ui.add_font: the UI is not initialised yet (wait for ui.ready())");
        return nullptr;
    }
    FontRequest* r = IM_NEW(FontRequest)();
    memset(r, 0, sizeof *r);
    if (path && *path) strncpy(r->path, path, sizeof r->path - 1);
    r->use_default = !(path && *path);
    r->size = size;
    r->merge = merge;
    if (ranges && ranges[0]) {
        r->ranges = IM_NEW(ImVector<ImWchar>)();
        for (const unsigned short* p = ranges; p[0] && p[1]; p += 2) {
            r->ranges->push_back((ImWchar)p[0]);
            r->ranges->push_back((ImWchar)p[1]);
        }
        r->ranges->push_back(0);
    }
    if (g_building) {
        // Mid-frame: the atlas is in use. Apply once the frame is parked.
        g_font_requests.push_back(r);
        return nullptr;
    }
    g_frame_lock.lock();
    ImFont* f = apply_font(r);
    g_frame_lock.unlock();
    IM_DELETE(r);
    return f;
}

void tempest_overlay_set_default_font(void* font) {
    if (!ImGui::GetCurrentContext()) return;
    ImGui::GetIO().FontDefault = (ImFont*)font;
}

int tempest_overlay_fonts_dirty(void) { return g_fonts_dirty; }

// ---------------------------------------------------------------------------
// Textures (decoded on the game thread, uploaded in Present)
// ---------------------------------------------------------------------------

struct Texture {
    int id;
    int w, h;
    unsigned char* pixels;  // RGBA, owned until uploaded
    ImTextureID tex;        // 0 until uploaded
    void* native;           // ID3D11ShaderResourceView* / IDirect3DTexture9*
    int release_in;         // >0: countdown of Presents before the native object goes
};

static ImVector<Texture*> g_textures;
static int g_next_texture = 1;

static Texture* find_texture(int id) {
    for (int i = 0; i < g_textures.Size; ++i)
        if (g_textures[i]->id == id) return g_textures[i];
    return nullptr;
}

static int add_texture(unsigned char* pixels, int w, int h) {
    Texture* t = IM_NEW(Texture)();
    memset(t, 0, sizeof *t);
    t->id = g_next_texture++;
    t->w = w;
    t->h = h;
    t->pixels = pixels;
    g_frame_lock.lock();
    g_textures.push_back(t);
    g_frame_lock.unlock();
    return t->id;
}

// ui.load_image(path): PNG/JPEG/BMP/TGA from disk. Returns the handle (>0) or 0.
int tempest_overlay_load_image_file(const char* path, int* w, int* h) {
    int iw = 0, ih = 0, n = 0;
    unsigned char* px = stbi_load(path, &iw, &ih, &n, 4);
    if (!px) {
        olog("ui.load_image: cannot decode %s", path ? path : "(null)");
        return 0;
    }
    if (w) *w = iw;
    if (h) *h = ih;
    return add_texture(px, iw, ih);
}

// ui.image_from_rgba(w, h, bytes): raw RGBA8 pixels (copied).
int tempest_overlay_load_image_rgba(const unsigned char* rgba, int w, int h) {
    if (!rgba || w <= 0 || h <= 0) return 0;
    size_t size = (size_t)w * (size_t)h * 4;
    unsigned char* px = (unsigned char*)malloc(size);
    if (!px) return 0;
    memcpy(px, rgba, size);
    return add_texture(px, w, h);
}

// The ImTextureID to draw with, 0 while the upload is pending or after free.
unsigned long long tempest_overlay_texture_id(int id) {
    g_frame_lock.lock();
    Texture* t = find_texture(id);
    unsigned long long r = (t && !t->release_in) ? (unsigned long long)t->tex : 0;
    g_frame_lock.unlock();
    return r;
}

void tempest_overlay_free_texture(int id) {
    g_frame_lock.lock();
    Texture* t = find_texture(id);
    if (t && !t->release_in) {
        // Parked frames may still reference it: let a few Presents go by.
        t->release_in = 3;
        t->tex = 0;
    }
    g_frame_lock.unlock();
}

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
    if (g_fonts_dirty) {
        if (strcmp(g_backend, "headless") != 0) return 0;  // the render thread rebuilds first
        unsigned char* pixels = nullptr;
        int tw = 0, th = 0;
        ImGui::GetIO().Fonts->GetTexDataAsRGBA32(&pixels, &tw, &th);
        g_fonts_dirty = 0;
    }
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
    if (g_font_requests.Size > 0) {
        g_frame_lock.lock();
        for (int i = 0; i < g_font_requests.Size; ++i) {
            apply_font(g_font_requests[i]);
            IM_DELETE(g_font_requests[i]);
        }
        g_font_requests.clear();
        g_frame_lock.unlock();
    }
    return 1;
}

// Headless tests: pretend every pending texture was uploaded.
int tempest_overlay_headless_upload(void) {
    int n = 0;
    g_frame_lock.lock();
    for (int i = 0; i < g_textures.Size; ++i) {
        Texture* t = g_textures[i];
        if (t->pixels && !t->tex) {
            t->tex = (ImTextureID)(uintptr_t)(0x1000 + t->id);
            free(t->pixels);
            t->pixels = nullptr;
            ++n;
        }
    }
    g_frame_lock.unlock();
    return n;
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
    g_frame_lock.lock();
    if (g_fonts_dirty) {
        ImGui_ImplDX11_InvalidateDeviceObjects();
        ImGui_ImplDX11_CreateDeviceObjects();
        g_fonts_dirty = 0;
    }
    for (int i = 0; i < g_textures.Size; ++i) {
        Texture* t = g_textures[i];
        if (t->pixels && !t->tex) {
            D3D11_TEXTURE2D_DESC desc{};
            desc.Width = t->w;
            desc.Height = t->h;
            desc.MipLevels = 1;
            desc.ArraySize = 1;
            desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
            desc.SampleDesc.Count = 1;
            desc.Usage = D3D11_USAGE_DEFAULT;
            desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
            D3D11_SUBRESOURCE_DATA init{};
            init.pSysMem = t->pixels;
            init.SysMemPitch = t->w * 4;
            ID3D11Texture2D* tex = nullptr;
            ID3D11ShaderResourceView* srv = nullptr;
            if (SUCCEEDED(g_device11->CreateTexture2D(&desc, &init, &tex)) && tex) {
                g_device11->CreateShaderResourceView(tex, nullptr, &srv);
                tex->Release();
            }
            free(t->pixels);
            t->pixels = nullptr;
            t->native = srv;
            t->tex = (ImTextureID)(uintptr_t)srv;
            if (!srv) olog("texture #%d (%dx%d) upload failed", t->id, t->w, t->h);
        } else if (t->release_in > 0 && --t->release_in == 0) {
            if (t->native) ((ID3D11ShaderResourceView*)t->native)->Release();
            IM_DELETE(t);
            g_textures.erase(g_textures.Data + i);
            --i;
        }
    }
    g_frame_lock.unlock();
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
    g_frame_lock.lock();
    if (g_fonts_dirty) {
        ImGui_ImplDX9_InvalidateDeviceObjects();
        ImGui_ImplDX9_CreateDeviceObjects();
        g_fonts_dirty = 0;
    }
    for (int i = 0; i < g_textures.Size; ++i) {
        Texture* t = g_textures[i];
        if (t->pixels && !t->tex) {
            IDirect3DTexture9* tex = nullptr;
            if (SUCCEEDED(dev->CreateTexture(t->w, t->h, 1, 0, D3DFMT_A8R8G8B8, D3DPOOL_MANAGED, &tex, nullptr)) && tex) {
                D3DLOCKED_RECT rect{};
                if (SUCCEEDED(tex->LockRect(0, &rect, nullptr, 0))) {
                    for (int y = 0; y < t->h; ++y) {
                        unsigned char* dst = (unsigned char*)rect.pBits + y * rect.Pitch;
                        const unsigned char* src = t->pixels + (size_t)y * t->w * 4;
                        for (int x = 0; x < t->w; ++x) {  // RGBA -> BGRA
                            dst[x * 4 + 0] = src[x * 4 + 2];
                            dst[x * 4 + 1] = src[x * 4 + 1];
                            dst[x * 4 + 2] = src[x * 4 + 0];
                            dst[x * 4 + 3] = src[x * 4 + 3];
                        }
                    }
                    tex->UnlockRect(0);
                }
            }
            free(t->pixels);
            t->pixels = nullptr;
            t->native = tex;
            t->tex = (ImTextureID)(uintptr_t)tex;
            if (!tex) olog("texture #%d (%dx%d) upload failed", t->id, t->w, t->h);
        } else if (t->release_in > 0 && --t->release_in == 0) {
            if (t->native) ((IDirect3DTexture9*)t->native)->Release();
            IM_DELETE(t);
            g_textures.erase(g_textures.Data + i);
            --i;
        }
    }
    g_frame_lock.unlock();
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
