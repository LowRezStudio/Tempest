// Dear ImGui configuration for Tempest.Scripting (IMGUI_USER_CONFIG).
//
// An ImGui assertion inside a game process must not abort(): it is logged
// through the overlay's log sink and execution continues. Lua-side misuse
// (unbalanced Begin/End and friends) is handled by ImGui's error recovery
// around every draw handler, so this only covers genuine internal faults.
#pragma once

#include "cimgui/imgui/imconfig.h"

#ifdef __cplusplus
extern "C" void tempest_overlay_assert_failed(const char* expr, const char* file, int line);
#endif

#define IM_ASSERT(_EXPR)                                                           \
    do {                                                                           \
        if (!(_EXPR)) tempest_overlay_assert_failed(#_EXPR, __FILE__, __LINE__);  \
    } while (0)

// cimgui declares every function with its own export macro; nothing from
// ImGui itself needs to leave the DLL.
#define IMGUI_API
#define IMGUI_IMPL_API
