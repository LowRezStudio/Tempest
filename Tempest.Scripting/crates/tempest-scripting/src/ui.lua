-- The `ui` module: Dear ImGui for Lua mods. Runs after prelude.lua (and again
-- on reload). `ui.C` is the raw cimgui API bound through LuaJIT's FFI; the
-- functions below are the curated layer: Lua values in, Lua values out, no
-- pointers. Everything that draws is only valid inside a ui.on_draw handler,
-- which runs once per rendered frame on the game thread.
local ffi = require("ffi")
local spec = tempest._imgui

if not spec.defined then
	ffi.cdef(spec.cdef)
	spec.defined = true
end
if not spec.C then
	local C, addrs = {}, tempest._ui_syms()
	for _, name in ipairs(spec.fns) do
		local a = addrs[name]
		if a then C[name] = ffi.cast("tempest_fn_" .. name, a) end
	end
	spec.C = C
end
local C = spec.C

local ui = { C = C }
for group, values in pairs(spec.enums) do ui[group] = values end
_G.ui = ui
tempest.ui = ui

-- A reload must not leave the game without its mouse.
tempest._ui_capture(false)
if spec.look_pc then
	local pc = tempest.local_player()
	if pc and pc == spec.look_pc then pcall(pc.IgnoreLookInput, pc, false) end
	spec.look_pc = nil
end

-- ---------------------------------------------------------------------------
-- Scratch cdata (ImGui reads/writes them during the call only)
-- ---------------------------------------------------------------------------
local bool1 = ffi.new("bool[1]")
local int1 = ffi.new("int[1]")
local int4 = ffi.new("int[4]")
local float1 = ffi.new("float[1]")
local float4 = ffi.new("float[4]")
local v2a, v2b, v2c, v2d = ffi.new("ImVec2"), ffi.new("ImVec2"), ffi.new("ImVec2"), ffi.new("ImVec2")
local v4a, v4b = ffi.new("ImVec4"), ffi.new("ImVec4")
local textbuf, textcap = nil, 0

local function vec2(x, y) v2a.x, v2a.y = x or 0, y or 0 return v2a end
local function vec2b(x, y) v2b.x, v2b.y = x or 0, y or 0 return v2b end
local function vec4(r, g, b, a) v4a.x, v4a.y, v4a.z, v4a.w = r, g, b, a == nil and 1 or a return v4a end
local function outv2(fn, ...) fn(v2b, ...) return v2b.x, v2b.y end

-- A NUL-terminated copy of `s` in a buffer of at least `size` bytes.
local function text_buffer(s, size)
	if size > textcap then
		textcap = math.max(size, textcap * 2, 256)
		textbuf = ffi.new("char[?]", textcap)
	end
	ffi.copy(textbuf, s)
	return textbuf
end

local in_frame = false
local function check()
	if not in_frame then error("ui widgets are only valid inside a ui.on_draw handler", 3) end
end

function ui.vec2(x, y) return ffi.new("ImVec2", x or 0, y or 0) end
function ui.vec4(x, y, z, w) return ffi.new("ImVec4", x or 0, y or 0, z or 0, w or 0) end

-- ui.color(r, g, b, a): 0..1 floats -> packed ImU32 for the draw functions.
function ui.color(r, g, b, a)
	local function u8(v) return math.floor(math.max(0, math.min(1, v)) * 255 + 0.5) end
	return bit.bor(bit.lshift(u8(a == nil and 1 or a), 24), bit.lshift(u8(b), 16), bit.lshift(u8(g), 8), u8(r))
end
-- ui.rgb(0xRRGGBB, alpha?) -> packed ImU32
function ui.rgb(hex, a)
	return ui.color(bit.band(bit.rshift(hex, 16), 0xFF) / 255, bit.band(bit.rshift(hex, 8), 0xFF) / 255, bit.band(hex, 0xFF) / 255, a)
end

-- ---------------------------------------------------------------------------
-- Frame handlers
-- ---------------------------------------------------------------------------
local handlers, next_id = {}, 0

-- ui.on_draw(fn) -> id: fn() runs every frame between NewFrame and Render.
-- Errors are logged once per distinct message; ImGui recovers the window
-- stack after a failed handler.
function ui.on_draw(fn)
	assert(type(fn) == "function", "ui.on_draw(fn)")
	next_id = next_id + 1
	handlers[next_id] = { fn = fn, mod = tempest.mod() }
	return next_id
end

function ui.off_draw(id)
	handlers[id] = nil
end

function ui._frame()
	local ids = {}
	for id in pairs(handlers) do ids[#ids + 1] = id end
	table.sort(ids)
	in_frame = true
	for _, id in ipairs(ids) do
		local h = handlers[id]
		if h then
			local err = tempest._ui_protect(h.fn, h.mod)
			if err then
				if err ~= h.err then
					h.err = err
					tempest.log(string.format("[%s] draw handler #%d failed: %s", h.mod, id, err))
				end
			else
				h.err = nil
			end
		end
	end
	in_frame = false
end

function ui._forget(mod)
	for id, h in pairs(handlers) do
		if h.mod == mod then handlers[id] = nil end
	end
end

-- ---------------------------------------------------------------------------
-- Runtime state
-- ---------------------------------------------------------------------------
function ui.ready() return tempest._ui_ready() end
function ui.backend() return tempest._ui_backend() end
-- ui.stats() -> { built, rendered, vertices, lists }
function ui.stats()
	local b, r, v, l = tempest._ui_stats()
	return { built = b, rendered = r, vertices = v, lists = l }
end

-- ui.capture(on): give the mouse to the UI. Draws ImGui's cursor, keeps
-- mouse messages (and keys ImGui uses) away from the game, and parks the
-- local player's look input while it lasts.
function ui.capture(on)
	on = on ~= false
	tempest._ui_capture(on)
	local pc = tempest.local_player()
	if on then
		if pc and not spec.look_pc and pcall(pc.IgnoreLookInput, pc, true) then spec.look_pc = pc end
	elseif spec.look_pc then
		if pc and pc == spec.look_pc then pcall(pc.IgnoreLookInput, pc, false) end
		spec.look_pc = nil
	end
end
function ui.capturing() return tempest._ui_capturing() end
-- ui.want() -> mouse, keyboard, text  (ImGui's WantCapture* flags)
function ui.want() return tempest._ui_want() end
-- ui.scale(s?) -> s: fonts and paddings together (1 = ImGui default)
function ui.scale(s) return tempest._ui_scale(s) end
function ui.io() return C.igGetIO() end
function ui.style() return C.igGetStyle() end
function ui.screen() check() local io = C.igGetIO() return io.DisplaySize.x, io.DisplaySize.y end
function ui.mouse() check() local io = C.igGetIO() return io.MousePos.x, io.MousePos.y end
function ui.framerate() return C.igGetIO().Framerate end
function ui.delta_time() return C.igGetIO().DeltaTime end
function ui.key_down(key) check() return C.igIsKeyDown_Nil(key) end
function ui.key_pressed(key, rep) check() return C.igIsKeyPressed_Bool(key, rep == true) end
function ui.key_released(key) check() return C.igIsKeyReleased_Nil(key) end
function ui.mouse_down(btn) check() return C.igIsMouseDown_Nil(btn or 0) end
function ui.mouse_clicked(btn, rep) check() return C.igIsMouseClicked_Bool(btn or 0, rep == true) end
function ui.mouse_released(btn) check() return C.igIsMouseReleased_Nil(btn or 0) end
function ui.demo() check() C.igShowDemoWindow(nil) end
function ui.metrics() check() C.igShowMetricsWindow(nil) end

-- ---------------------------------------------------------------------------
-- Windows
-- ---------------------------------------------------------------------------
-- ui.begin_window(title, open?, flags?) -> visible, open  (always pair with end_window)
function ui.begin_window(title, open, flags)
	check()
	local p_open = nil
	if open ~= nil then bool1[0] = open p_open = bool1 end
	local visible = C.igBegin(title, p_open, flags or 0)
	return visible, p_open and p_open[0]
end
function ui.end_window() C.igEnd() end

-- ui.window(title, fn) / ui.window(title, { open = bool, flags = n }, fn) -> open
-- fn runs only while the window is visible; End is always called.
function ui.window(title, opts, fn)
	if type(opts) == "function" then fn, opts = opts, nil end
	opts = opts or {}
	local visible, open = ui.begin_window(title, opts.open, opts.flags)
	local ok, err = true, nil
	if visible and fn then ok, err = pcall(fn) end
	C.igEnd()
	if not ok then error(err, 0) end
	return open
end

-- ui.child(id, w, h, fn) / ui.child(id, w, h, { child_flags, flags }, fn)
function ui.child(id, w, h, opts, fn)
	check()
	if type(opts) == "function" then fn, opts = opts, nil end
	opts = opts or {}
	local visible = C.igBeginChild_Str(id, vec2(w, h), opts.child_flags or 0, opts.flags or 0)
	local ok, err = true, nil
	if visible and fn then ok, err = pcall(fn) end
	C.igEndChild()
	if not ok then error(err, 0) end
end

function ui.set_next_window_pos(x, y, cond, pivot_x, pivot_y)
	check() C.igSetNextWindowPos(vec2(x, y), cond or 0, vec2b(pivot_x, pivot_y))
end
function ui.set_next_window_size(w, h, cond) check() C.igSetNextWindowSize(vec2(w, h), cond or 0) end
function ui.set_next_window_collapsed(c, cond) check() C.igSetNextWindowCollapsed(c, cond or 0) end
function ui.set_next_window_focus() check() C.igSetNextWindowFocus() end
function ui.set_next_window_bg_alpha(a) check() C.igSetNextWindowBgAlpha(a) end
function ui.set_next_item_width(w) check() C.igSetNextItemWidth(w) end
function ui.set_next_item_open(open, cond) check() C.igSetNextItemOpen(open, cond or 0) end
function ui.window_pos() check() return outv2(C.igGetWindowPos) end
function ui.window_size() check() return outv2(C.igGetWindowSize) end
function ui.window_width() check() return C.igGetWindowWidth() end
function ui.window_height() check() return C.igGetWindowHeight() end
function ui.set_window_pos(x, y, cond) check() C.igSetWindowPos_Vec2(vec2(x, y), cond or 0) end
function ui.set_window_size(w, h, cond) check() C.igSetWindowSize_Vec2(vec2(w, h), cond or 0) end
function ui.set_window_focus() check() C.igSetWindowFocus_Nil() end
function ui.is_window_hovered(flags) check() return C.igIsWindowHovered(flags or 0) end
function ui.is_window_focused(flags) check() return C.igIsWindowFocused(flags or 0) end
function ui.is_window_collapsed() check() return C.igIsWindowCollapsed() end
function ui.content_region_avail() check() return outv2(C.igGetContentRegionAvail) end
function ui.cursor_pos() check() return outv2(C.igGetCursorPos) end
function ui.set_cursor_pos(x, y) check() C.igSetCursorPos(vec2(x, y)) end
function ui.cursor_screen_pos() check() return outv2(C.igGetCursorScreenPos) end
function ui.set_cursor_screen_pos(x, y) check() C.igSetCursorScreenPos(vec2(x, y)) end
function ui.scroll_y() check() return C.igGetScrollY() end
function ui.set_scroll_y(y) check() C.igSetScrollY_Float(y) end
function ui.scroll_max_y() check() return C.igGetScrollMaxY() end
function ui.set_scroll_here_y(ratio) check() C.igSetScrollHereY(ratio or 0.5) end

-- ---------------------------------------------------------------------------
-- Layout
-- ---------------------------------------------------------------------------
function ui.separator() check() C.igSeparator() end
function ui.separator_text(label) check() C.igSeparatorText(label) end
function ui.same_line(offset, spacing) check() C.igSameLine(offset or 0, spacing or -1) end
function ui.new_line() check() C.igNewLine() end
function ui.spacing() check() C.igSpacing() end
function ui.dummy(w, h) check() C.igDummy(vec2(w, h)) end
function ui.indent(w) check() C.igIndent(w or 0) end
function ui.unindent(w) check() C.igUnindent(w or 0) end
function ui.align_text_to_frame_padding() check() C.igAlignTextToFramePadding() end
function ui.text_line_height() check() return C.igGetTextLineHeight() end
function ui.text_line_height_with_spacing() check() return C.igGetTextLineHeightWithSpacing() end
function ui.frame_height() check() return C.igGetFrameHeight() end
function ui.calc_text_size(text) check() C.igCalcTextSize(v2b, text, nil, false, -1) return v2b.x, v2b.y end
function ui.group(fn)
	check()
	C.igBeginGroup()
	local ok, err = pcall(fn)
	C.igEndGroup()
	if not ok then error(err, 0) end
end
function ui.push_id(id)
	check()
	if type(id) == "number" then C.igPushID_Int(id) else C.igPushID_Str(tostring(id)) end
end
function ui.pop_id() check() C.igPopID() end
function ui.push_item_width(w) check() C.igPushItemWidth(w) end
function ui.pop_item_width() check() C.igPopItemWidth() end
-- ui.push_color(ui.Col.Text, r, g, b, a?) ... ui.pop_color(n?)
function ui.push_color(idx, r, g, b, a) check() C.igPushStyleColor_Vec4(idx, vec4(r, g, b, a)) end
function ui.pop_color(n) check() C.igPopStyleColor(n or 1) end
-- ui.push_var(ui.StyleVar.Alpha, 0.5) / ui.push_var(ui.StyleVar.WindowPadding, x, y)
function ui.push_var(idx, a, b)
	check()
	if b ~= nil then C.igPushStyleVar_Vec2(idx, vec2(a, b)) else C.igPushStyleVar_Float(idx, a) end
end
function ui.pop_var(n) check() C.igPopStyleVar(n or 1) end
function ui.push_text_wrap_pos(x) check() C.igPushTextWrapPos(x or 0) end
function ui.pop_text_wrap_pos() check() C.igPopTextWrapPos() end
function ui.begin_disabled(d) check() C.igBeginDisabled(d ~= false) end
function ui.end_disabled() check() C.igEndDisabled() end

-- ---------------------------------------------------------------------------
-- Text
-- ---------------------------------------------------------------------------
local function fmt(s, ...)
	if select("#", ...) > 0 then return string.format(s, ...) end
	return tostring(s)
end
function ui.text(s, ...) check() C.igTextUnformatted(fmt(s, ...), nil) end
function ui.text_colored(r, g, b, a, s, ...) check() C.igTextColored(vec4(r, g, b, a), "%s", fmt(s, ...)) end
function ui.text_disabled(s, ...) check() C.igTextDisabled("%s", fmt(s, ...)) end
function ui.text_wrapped(s, ...) check() C.igTextWrapped("%s", fmt(s, ...)) end
function ui.label_text(label, s, ...) check() C.igLabelText(label, "%s", fmt(s, ...)) end
function ui.bullet_text(s, ...) check() C.igBulletText("%s", fmt(s, ...)) end
function ui.bullet() check() C.igBullet() end

-- ---------------------------------------------------------------------------
-- Widgets: every one returns the new value (and `changed`) instead of
-- writing through a pointer.
-- ---------------------------------------------------------------------------
function ui.button(label, w, h) check() return C.igButton(label, vec2(w, h)) end
function ui.small_button(label) check() return C.igSmallButton(label) end
function ui.invisible_button(id, w, h, flags) check() return C.igInvisibleButton(id, vec2(w, h), flags or 0) end
function ui.arrow_button(id, dir) check() return C.igArrowButton(id, dir) end
function ui.checkbox(label, value)
	check()
	bool1[0] = value and true or false
	local changed = C.igCheckbox(label, bool1)
	return bool1[0], changed
end
function ui.radio(label, active) check() return C.igRadioButton_Bool(label, active and true or false) end
-- ui.radio_value(label, current, value) -> current (= value when clicked), clicked
function ui.radio_value(label, current, value)
	check()
	int1[0] = current
	local clicked = C.igRadioButton_IntPtr(label, int1, value)
	return int1[0], clicked
end
function ui.progress_bar(fraction, w, h, overlay) check() C.igProgressBar(fraction, vec2(w or -1, h or 0), overlay) end
function ui.selectable(label, selected, flags, w, h)
	check()
	return C.igSelectable_Bool(label, selected and true or false, flags or 0, vec2(w, h))
end

function ui.slider_float(label, v, min, max, format, flags)
	check()
	float1[0] = v
	local changed = C.igSliderFloat(label, float1, min, max, format or "%.3f", flags or 0)
	return float1[0], changed
end
function ui.slider_int(label, v, min, max, format, flags)
	check()
	int1[0] = v
	local changed = C.igSliderInt(label, int1, min, max, format or "%d", flags or 0)
	return int1[0], changed
end
function ui.slider_angle(label, rad, min_deg, max_deg, format, flags)
	check()
	float1[0] = rad
	local changed = C.igSliderAngle(label, float1, min_deg or -360, max_deg or 360, format or "%.0f deg", flags or 0)
	return float1[0], changed
end
function ui.drag_float(label, v, speed, min, max, format, flags)
	check()
	float1[0] = v
	local changed = C.igDragFloat(label, float1, speed or 1, min or 0, max or 0, format or "%.3f", flags or 0)
	return float1[0], changed
end
function ui.drag_int(label, v, speed, min, max, format, flags)
	check()
	int1[0] = v
	local changed = C.igDragInt(label, int1, speed or 1, min or 0, max or 0, format or "%d", flags or 0)
	return int1[0], changed
end

-- Multi-component variants take and return a table of 2..4 numbers.
local function floats_in(t)
	for i = 1, 4 do float4[i - 1] = t[i] or 0 end
	return #t
end
local function floats_out(t, n)
	local out = {}
	for i = 1, n do out[i] = float4[i - 1] end
	return out
end
local slider_float_n = { nil, "igSliderFloat2", "igSliderFloat3", "igSliderFloat4" }
local drag_float_n = { nil, "igDragFloat2", "igDragFloat3", "igDragFloat4" }
local input_float_n = { nil, "igInputFloat2", "igInputFloat3", "igInputFloat4" }
function ui.slider_floats(label, t, min, max, format, flags)
	check()
	local n = floats_in(t)
	local changed = C[assert(slider_float_n[n], "2 to 4 values")](label, float4, min, max, format or "%.3f", flags or 0)
	return floats_out(t, n), changed
end
function ui.drag_floats(label, t, speed, min, max, format, flags)
	check()
	local n = floats_in(t)
	local changed = C[assert(drag_float_n[n], "2 to 4 values")](label, float4, speed or 1, min or 0, max or 0, format or "%.3f", flags or 0)
	return floats_out(t, n), changed
end
function ui.input_floats(label, t, format, flags)
	check()
	local n = floats_in(t)
	local changed = C[assert(input_float_n[n], "2 to 4 values")](label, float4, format or "%.3f", flags or 0)
	return floats_out(t, n), changed
end

function ui.input_text(label, value, maxlen, flags)
	check()
	value = value or ""
	local size = (maxlen or 255) + 1
	local buf = text_buffer(value, size)
	if C.igInputText(label, buf, size, flags or 0, nil, nil) then return ffi.string(buf), true end
	return value, false
end
function ui.input_text_multiline(label, value, w, h, maxlen, flags)
	check()
	value = value or ""
	local size = (maxlen or 4095) + 1
	local buf = text_buffer(value, size)
	if C.igInputTextMultiline(label, buf, size, vec2(w, h), flags or 0, nil, nil) then return ffi.string(buf), true end
	return value, false
end
function ui.input_text_with_hint(label, hint, value, maxlen, flags)
	check()
	value = value or ""
	local size = (maxlen or 255) + 1
	local buf = text_buffer(value, size)
	if C.igInputTextWithHint(label, hint, buf, size, flags or 0, nil, nil) then return ffi.string(buf), true end
	return value, false
end
function ui.input_int(label, v, step, step_fast, flags)
	check()
	int1[0] = v
	local changed = C.igInputInt(label, int1, step or 1, step_fast or 100, flags or 0)
	return int1[0], changed
end
function ui.input_float(label, v, step, step_fast, format, flags)
	check()
	float1[0] = v
	local changed = C.igInputFloat(label, float1, step or 0, step_fast or 0, format or "%.3f", flags or 0)
	return float1[0], changed
end

-- ui.combo(label, index, items, height_in_items?) -> index (1-based), changed
function ui.combo(label, index, items, height)
	check()
	int1[0] = (index or 1) - 1
	local changed = C.igCombo_Str(label, int1, table.concat(items, "\0") .. "\0\0", height or -1)
	return int1[0] + 1, changed
end
-- ui.list_box(label, index, items, height_in_items?) -> index (1-based), changed
function ui.list_box(label, index, items, height)
	check()
	local n = #items
	local arr = ffi.new("const char*[?]", n)
	for i = 1, n do arr[i - 1] = items[i] end
	int1[0] = (index or 1) - 1
	local changed = C.igListBox_Str_arr(label, int1, arr, n, height or -1)
	return int1[0] + 1, changed
end

-- ui.color_edit(label, {r, g, b[, a]}, flags?) -> {r, g, b[, a]}, changed
function ui.color_edit(label, col, flags)
	check()
	local n = floats_in(col)
	local changed = (n >= 4 and C.igColorEdit4 or C.igColorEdit3)(label, float4, flags or 0)
	return floats_out(col, n >= 4 and 4 or 3), changed
end
function ui.color_picker(label, col, flags)
	check()
	local n = floats_in(col)
	local changed
	if n >= 4 then changed = C.igColorPicker4(label, float4, flags or 0, nil) else changed = C.igColorPicker3(label, float4, flags or 0) end
	return floats_out(col, n >= 4 and 4 or 3), changed
end
function ui.color_button(id, r, g, b, a, flags, w, h) check() return C.igColorButton(id, vec4(r, g, b, a), flags or 0, vec2(w, h)) end

-- ---------------------------------------------------------------------------
-- Trees, headers, tabs, menus, popups, tables
-- ---------------------------------------------------------------------------
function ui.collapsing_header(label, flags) check() return C.igCollapsingHeader_TreeNodeFlags(label, flags or 0) end
function ui.tree_node(label, flags) check() return C.igTreeNodeEx_Str(label, flags or 0) end
function ui.tree_pop() check() C.igTreePop() end
-- ui.tree(label, fn, flags?): fn runs when the node is open; TreePop handled.
function ui.tree(label, fn, flags)
	check()
	if not C.igTreeNodeEx_Str(label, flags or 0) then return false end
	local ok, err = pcall(fn)
	C.igTreePop()
	if not ok then error(err, 0) end
	return true
end
function ui.tab_bar(id, fn, flags)
	check()
	if not C.igBeginTabBar(id, flags or 0) then return false end
	local ok, err = pcall(fn)
	C.igEndTabBar()
	if not ok then error(err, 0) end
	return true
end
-- ui.tab_item(label, fn) / ui.tab_item(label, { open = bool, flags = n }, fn) -> open
function ui.tab_item(label, opts, fn)
	check()
	if type(opts) == "function" then fn, opts = opts, nil end
	opts = opts or {}
	local p_open = nil
	if opts.open ~= nil then bool1[0] = opts.open p_open = bool1 end
	local selected = C.igBeginTabItem(label, p_open, opts.flags or 0)
	local open = p_open and p_open[0]
	if selected then
		local ok, err = pcall(fn)
		C.igEndTabItem()
		if not ok then error(err, 0) end
	end
	return open, selected
end
function ui.menu_bar(fn)
	check()
	if not C.igBeginMenuBar() then return false end
	local ok, err = pcall(fn)
	C.igEndMenuBar()
	if not ok then error(err, 0) end
	return true
end
function ui.main_menu_bar(fn)
	check()
	if not C.igBeginMainMenuBar() then return false end
	local ok, err = pcall(fn)
	C.igEndMainMenuBar()
	if not ok then error(err, 0) end
	return true
end
function ui.menu(label, fn, enabled)
	check()
	if not C.igBeginMenu(label, enabled ~= false) then return false end
	local ok, err = pcall(fn)
	C.igEndMenu()
	if not ok then error(err, 0) end
	return true
end
function ui.menu_item(label, shortcut, selected, enabled)
	check()
	return C.igMenuItem_Bool(label, shortcut, selected and true or false, enabled ~= false)
end
function ui.open_popup(id, flags) check() C.igOpenPopup_Str(id, flags or 0) end
function ui.close_current_popup() check() C.igCloseCurrentPopup() end
function ui.popup(id, fn, flags)
	check()
	if not C.igBeginPopup(id, flags or 0) then return false end
	local ok, err = pcall(fn)
	C.igEndPopup()
	if not ok then error(err, 0) end
	return true
end
-- ui.popup_modal(title, fn) / ui.popup_modal(title, { open = bool, flags = n }, fn) -> open
function ui.popup_modal(title, opts, fn)
	check()
	if type(opts) == "function" then fn, opts = opts, nil end
	opts = opts or {}
	local p_open = nil
	if opts.open ~= nil then bool1[0] = opts.open p_open = bool1 end
	local visible = C.igBeginPopupModal(title, p_open, opts.flags or 0)
	local open = p_open and p_open[0]
	if visible then
		local ok, err = pcall(fn)
		C.igEndPopup()
		if not ok then error(err, 0) end
	end
	return open
end
function ui.popup_context_item(id, fn, flags)
	check()
	if not C.igBeginPopupContextItem(id, flags or 1) then return false end
	local ok, err = pcall(fn)
	C.igEndPopup()
	if not ok then error(err, 0) end
	return true
end

-- ui.table(id, columns, fn) / ui.table(id, columns, { flags, w, h }, fn)
function ui.table(id, columns, opts, fn)
	check()
	if type(opts) == "function" then fn, opts = opts, nil end
	opts = opts or {}
	if not C.igBeginTable(id, columns, opts.flags or 0, vec2(opts.w, opts.h), opts.inner_width or 0) then return false end
	local ok, err = pcall(fn)
	C.igEndTable()
	if not ok then error(err, 0) end
	return true
end
function ui.table_setup_column(label, flags, width, user_id) check() C.igTableSetupColumn(label, flags or 0, width or 0, user_id or 0) end
function ui.table_setup_scroll_freeze(cols, rows) check() C.igTableSetupScrollFreeze(cols, rows) end
function ui.table_headers_row() check() C.igTableHeadersRow() end
function ui.table_next_row(flags, min_height) check() C.igTableNextRow(flags or 0, min_height or 0) end
function ui.table_next_column() check() return C.igTableNextColumn() end
function ui.table_set_column_index(i) check() return C.igTableSetColumnIndex(i) end
function ui.table_column_index() check() return C.igTableGetColumnIndex() end
function ui.table_row_index() check() return C.igTableGetRowIndex() end
function ui.table_set_bg_color(target, color, column) check() C.igTableSetBgColor(target, color, column or -1) end

-- ---------------------------------------------------------------------------
-- Item queries, tooltips
-- ---------------------------------------------------------------------------
function ui.is_item_hovered(flags) check() return C.igIsItemHovered(flags or 0) end
function ui.is_item_active() check() return C.igIsItemActive() end
function ui.is_item_focused() check() return C.igIsItemFocused() end
function ui.is_item_clicked(btn) check() return C.igIsItemClicked(btn or 0) end
function ui.is_item_visible() check() return C.igIsItemVisible() end
function ui.is_item_edited() check() return C.igIsItemEdited() end
function ui.is_item_activated() check() return C.igIsItemActivated() end
function ui.is_item_deactivated() check() return C.igIsItemDeactivated() end
function ui.is_item_deactivated_after_edit() check() return C.igIsItemDeactivatedAfterEdit() end
function ui.is_any_item_hovered() check() return C.igIsAnyItemHovered() end
function ui.is_any_item_active() check() return C.igIsAnyItemActive() end
function ui.item_rect_min() check() return outv2(C.igGetItemRectMin) end
function ui.item_rect_max() check() return outv2(C.igGetItemRectMax) end
function ui.item_rect_size() check() return outv2(C.igGetItemRectSize) end
function ui.set_item_default_focus() check() C.igSetItemDefaultFocus() end
function ui.set_keyboard_focus_here(offset) check() C.igSetKeyboardFocusHere(offset or 0) end
-- ui.set_tooltip(text): tooltip for the previous item when hovered.
function ui.set_tooltip(s, ...) check() C.igSetItemTooltip("%s", fmt(s, ...)) end
-- ui.tooltip(fn): arbitrary tooltip content for the previous item.
function ui.tooltip(fn)
	check()
	if not C.igBeginItemTooltip() then return false end
	local ok, err = pcall(fn)
	C.igEndTooltip()
	if not ok then error(err, 0) end
	return true
end

-- ---------------------------------------------------------------------------
-- Plots
-- ---------------------------------------------------------------------------
local function float_array(values)
	local n = #values
	local arr = ffi.new("float[?]", n)
	for i = 1, n do arr[i - 1] = values[i] end
	return arr, n
end
-- ui.plot_lines(label, values, { overlay, min, max, w, h })
function ui.plot_lines(label, values, opts)
	check()
	opts = opts or {}
	local arr, n = float_array(values)
	C.igPlotLines_FloatPtr(label, arr, n, 0, opts.overlay, opts.min or 3.4e38, opts.max or 3.4e38, vec2(opts.w, opts.h), 4)
end
function ui.plot_histogram(label, values, opts)
	check()
	opts = opts or {}
	local arr, n = float_array(values)
	C.igPlotHistogram_FloatPtr(label, arr, n, 0, opts.overlay, opts.min or 3.4e38, opts.max or 3.4e38, vec2(opts.w, opts.h), 4)
end

-- ---------------------------------------------------------------------------
-- Draw lists: ui.fg.* draws over everything, ui.bg.* behind all windows,
-- ui.dl.* into the current window. Colors are packed ImU32 (ui.color / ui.rgb).
-- ---------------------------------------------------------------------------
local function draw_api(get_list)
	local d = {}
	function d.line(x1, y1, x2, y2, color, thickness)
		check() C.ImDrawList_AddLine(get_list(), vec2(x1, y1), vec2b(x2, y2), color, thickness or 1)
	end
	function d.rect(x1, y1, x2, y2, color, rounding, thickness, flags)
		check() C.ImDrawList_AddRect(get_list(), vec2(x1, y1), vec2b(x2, y2), color, rounding or 0, flags or 0, thickness or 1)
	end
	function d.rect_filled(x1, y1, x2, y2, color, rounding, flags)
		check() C.ImDrawList_AddRectFilled(get_list(), vec2(x1, y1), vec2b(x2, y2), color, rounding or 0, flags or 0)
	end
	function d.circle(x, y, radius, color, segments, thickness)
		check() C.ImDrawList_AddCircle(get_list(), vec2(x, y), radius, color, segments or 0, thickness or 1)
	end
	function d.circle_filled(x, y, radius, color, segments)
		check() C.ImDrawList_AddCircleFilled(get_list(), vec2(x, y), radius, color, segments or 0)
	end
	function d.triangle(x1, y1, x2, y2, x3, y3, color, thickness)
		check() C.ImDrawList_AddTriangle(get_list(), vec2(x1, y1), vec2b(x2, y2), ffi.new("ImVec2", x3, y3), color, thickness or 1)
	end
	function d.triangle_filled(x1, y1, x2, y2, x3, y3, color)
		check() C.ImDrawList_AddTriangleFilled(get_list(), vec2(x1, y1), vec2b(x2, y2), ffi.new("ImVec2", x3, y3), color)
	end
	function d.text(x, y, color, s, ...)
		check() C.ImDrawList_AddText_Vec2(get_list(), vec2(x, y), color, fmt(s, ...), nil)
	end
	function d.text_sized(size, x, y, color, s, ...)
		check() C.ImDrawList_AddText_FontPtr(get_list(), nil, size, vec2(x, y), color, fmt(s, ...), nil, 0, nil)
	end
	-- d.polyline({x1, y1, x2, y2, ...}, color, closed?, thickness?)
	function d.polyline(points, color, closed, thickness)
		check()
		local n = math.floor(#points / 2)
		local arr = ffi.new("ImVec2[?]", n)
		for i = 1, n do arr[i - 1].x, arr[i - 1].y = points[2 * i - 1], points[2 * i] end
		C.ImDrawList_AddPolyline(get_list(), arr, n, color, closed and 1 or 0, thickness or 1)
	end
	-- d.image(image, x1, y1, x2, y2, tint?) tint = packed colour (default white)
	function d.image(img, x1, y1, x2, y2, tint)
		check()
		local tex = tempest._ui_texture(img.id)
		if tex == 0 then return end
		local uv0, uv1 = uv()
		C.ImDrawList_AddImage(get_list(), tex, vec2(x1, y1), vec2b(x2, y2), uv0, uv1, tint or 0xFFFFFFFF)
	end
	function d.list() check() return get_list() end
	return d
end
ui.fg = draw_api(function() return C.igGetForegroundDrawList_Nil() end)
ui.bg = draw_api(function() return C.igGetBackgroundDrawList_Nil() end)
ui.dl = draw_api(function() return C.igGetWindowDrawList() end)

-- ---------------------------------------------------------------------------
-- Fonts
-- ---------------------------------------------------------------------------
-- ui.add_font(path, size, opts?) -> font: a TTF at `size` pixels. opts.merge
-- adds its glyphs to the previous font (icon fonts), opts.ranges = "icons"
-- (U+E000..U+F8FF), or { first, last, ... } pairs. Call it at load, not inside
-- a draw handler; the font is usable from the next frame on.
-- ui.add_font(nil, size) is the built-in font at another size.
local ICON_RANGES = { 0xE000, 0xF8FF }
function ui.add_font(path, size, opts)
	opts = opts or {}
	local ranges = opts.ranges
	if ranges == "icons" then ranges = ICON_RANGES end
	local f = tempest._ui_add_font(path, size or 13, opts.merge == true, ranges)
	if not f then return nil end
	return ffi.cast("ImFont*", f)
end
function ui.set_default_font(font) tempest._ui_set_default_font(tonumber(ffi.cast("uintptr_t", font))) end
function ui.push_font(font) check() C.igPushFont(font) end
function ui.pop_font() check() C.igPopFont() end
function ui.font_size() check() return C.igGetFontSize() end
-- ui.with_font(font, fn)
function ui.with_font(font, fn)
	check()
	C.igPushFont(font)
	local ok, err = pcall(fn)
	C.igPopFont()
	if not ok then error(err, 0) end
end

-- ---------------------------------------------------------------------------
-- Images
-- ---------------------------------------------------------------------------
-- ui.load_image(path) -> image { width, height }: PNG/JPEG/BMP/TGA, decoded
-- now and uploaded by the render thread (image:ready() tells). Draw it with
-- ui.image(image, w?, h?), ui.image_button(id, image, w?, h?) or
-- ui.fg.image(image, x1, y1, x2, y2, tint?). image:free() releases it.
local Image = {}
Image.__index = Image
function Image:ready() return tempest._ui_texture(self.id) ~= 0 end
function Image:free() tempest._ui_free_texture(self.id) end
function Image:texture() return tempest._ui_texture(self.id) end
local function new_image(id, w, h) return setmetatable({ id = id, width = w, height = h }, Image) end
function ui.load_image(path)
	local id, w, h = tempest._ui_load_image(path)
	if not id then return nil end
	return new_image(id, w, h)
end
-- ui.image_from_rgba(w, h, bytes): raw RGBA8 pixels in a string (w*h*4 bytes).
function ui.image_from_rgba(w, h, bytes)
	local id = tempest._ui_load_image_rgba(bytes, w, h)
	if not id then return nil end
	return new_image(id, w, h)
end
local function uv(x0, y0, x1, y1)
	v2c.x, v2c.y, v2d.x, v2d.y = x0 or 0, y0 or 0, x1 == nil and 1 or x1, y1 == nil and 1 or y1
	return v2c, v2d
end
-- ui.image(image, w?, h?, opts?) opts: { uv = {x0, y0, x1, y1} }. Draws a placeholder while uploading.
function ui.image(img, w, h, opts)
	check()
	w, h = w or img.width, h or img.height
	local tex = tempest._ui_texture(img.id)
	if tex == 0 then C.igDummy(vec2(w, h)) return end
	local u = opts and opts.uv or nil
	local uv0, uv1 = uv(u and u[1], u and u[2], u and u[3], u and u[4])
	C.igImage(tex, vec2(w, h), uv0, uv1)
end
function ui.image_button(id, img, w, h)
	check()
	w, h = w or img.width, h or img.height
	local tex = tempest._ui_texture(img.id)
	if tex == 0 then C.igDummy(vec2(w, h)) return false end
	local uv0, uv1 = uv()
	v4a.x, v4a.y, v4a.z, v4a.w = 0, 0, 0, 0
	v4b.x, v4b.y, v4b.z, v4b.w = 1, 1, 1, 1
	return C.igImageButton(id, tex, vec2(w, h), uv0, uv1, v4a, v4b)
end
ui.Image = Image

-- ---------------------------------------------------------------------------
-- Diagnostics
-- ---------------------------------------------------------------------------
tempest.command("ui_status", function(pc)
	local s = ui.stats()
	pc:ClientMessage(string.format("ui: backend=%s ready=%s capture=%s frames built=%d rendered=%d vertices=%d",
		ui.backend(), tostring(ui.ready()), tostring(ui.capturing()), s.built, s.rendered, s.vertices))
end, "ImGui backend state")

tempest.command("ui_demo", function(pc)
	if spec.demo_id then
		ui.off_draw(spec.demo_id)
		spec.demo_id = nil
		ui.capture(false)
		pc:ClientMessage("ui demo closed")
	else
		spec.demo_id = ui.on_draw(function() ui.demo() end)
		ui.capture(true)
		pc:ClientMessage("ui demo open (ui_demo again to close)")
	end
end, "toggle the Dear ImGui demo window")

tempest.command("ui_capture", function(pc, rest)
	ui.capture(rest ~= "0" and rest ~= "off" and rest ~= "false")
	pc:ClientMessage("ui capture " .. tostring(ui.capturing()))
end, "ui_capture [0|1]  (give the mouse to the UI)")
