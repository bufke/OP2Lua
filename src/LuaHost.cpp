// LuaHost.cpp - owns the embedded Lua interpreter for the current mission.
//
// Creates a FRESH lua_State per mission (so every mission starts from a clean global environment),
// opens the safe standard libraries, installs the game bindings (Bindings.cpp), and loads the
// scheduler preamble (after/every/when + the tick pump). RunFile() runs a mission script and
// reports Lua errors without letting them reach the game's C code.

#include "LuaHost.h"

#include <string>

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include "Guard.h"
#include "Log.h"
#include "Mission.h"

namespace OP2Lua {

LuaHost& LuaHost::Get() {
  static LuaHost* instance = new LuaHost();
  return *instance;
}

namespace {

// sol2 calls this when a binding throws (a GameFault from Guard.h, or any other C++ exception): the message, after
// the file and line of the Lua code that made the call, becomes the Lua error.
int BindingError(lua_State* L, sol::optional<const std::exception&>, sol::string_view what) {
  luaL_where(L, 1);
  lua_pushlstring(L, what.data(), what.size());
  lua_concat(L, 2);
  return 1;
}

// The budget of the call in progress (Budget::Call): instructions counted, whether it ran out, and how many calls
// are open (only the outermost starts a fresh budget).
long long gUsed = 0;
bool gExhausted = false;
int gDepth = 0;

// "Name.lua:42" (the file name without its folder), or "?" when Lua can't say.
std::string Where(const lua_Debug& ar) {
  std::string w = ar.short_src;
  const size_t slash = w.find_last_of("\\/");
  if (slash != std::string::npos) w.erase(0, slash + 1);
  if (ar.currentline > 0) w += ":" + std::to_string(ar.currentline);
  else if (ar.linedefined > 0) w += ":" + std::to_string(ar.linedefined);
  return w.empty() ? "?" : w;
}

void CountHook(lua_State* L, lua_Debug* ar);

// The hook's interval: every Budget::kHookEvery instructions, or every instruction once the call has run out, so the
// error reaches the code around a pcall that catches it (at a fixed interval it would land inside the pcall each time).
void SetHookEvery(lua_State* L, int every) { lua_sethook(L, &CountHook, LUA_MASKCOUNT, every); }
void ResetBudget(lua_State* L) {
  if (gExhausted && L != nullptr) SetHookEvery(L, Budget::kHookEvery);
  gUsed = 0;
  gExhausted = false;
}

// The count hook. Outside a call (the runtime's own setup) nothing is counted.
void CountHook(lua_State* L, lua_Debug* ar) {
  if (gDepth == 0) return;
  if (!gExhausted) {
    gUsed += Budget::kHookEvery;
    if (gUsed <= Budget::kInstructions) return;
  }
  std::string where = "?";
  if (lua_getinfo(L, "Sl", ar) != 0) where = Where(*ar);
  if (!gExhausted) {
    gExhausted = true;
    SetHookEvery(L, 1);
    LOG(WARNING) << "a mission call ran more than " << Budget::kInstructions / 1000000
                 << " million Lua instructions (at " << where << "); dropping it";
  }
  lua_pushfstring(L, "%s: the mission's script ran more than %d million instructions in one call (an endless loop?); "
                  "the call was dropped", where.c_str(), static_cast<int>(Budget::kInstructions / 1000000));
  lua_error(L);
}

std::string FunctionWhereOf(lua_State* L, int index);

// __op2lua_run(fn, kind) -> ok, result or error, dropped: the scheduler runs each after/every/when function through
// this, so each gets a budget of its own (the pump's own instructions keep theirs). `dropped` is true when it ran out;
// the Mission log then names it (`kind`, e.g. "every() function", and where it was defined).
int RunScheduled(lua_State* L) {
  lua_settop(L, 2);   // fn: anything callable; lua_pcall reports what isn't, as pcall did
  const std::string kind = lua_isstring(L, 2) ? lua_tostring(L, 2) : "scheduled function";
  const std::string defined = FunctionWhereOf(L, 1);
  lua_settop(L, 1);
  const long long pumpUsed = gUsed;
  ResetBudget(L);
  const int status = lua_pcall(L, 0, 1, 0);
  const bool dropped = gExhausted;
  ResetBudget(L);
  gUsed = pumpUsed;
  if (dropped) {
    const char* why = lua_isstring(L, -1) ? lua_tostring(L, -1) : "ran out of instructions";
    LOG_MISSION(ERROR) << kind << " defined at " << defined << " dropped: " << why;
  }
  lua_pushboolean(L, status == LUA_OK);
  lua_insert(L, -2);
  lua_pushboolean(L, dropped);
  return 3;
}

// Where the function at `index` was defined ("Name.lua:12"), or "?".
std::string FunctionWhereOf(lua_State* L, int index) {
  if (lua_type(L, index) != LUA_TFUNCTION) return "?";
  lua_Debug ar{};
  lua_pushvalue(L, index);
  if (lua_getinfo(L, ">S", &ar) == 0) return "?";
  ar.currentline = -1;
  return Where(ar);
}

}  // namespace

Budget::Call::Call() {
  if (gDepth++ == 0) ResetBudget(LuaHost::Get().lua().lua_state());
}
Budget::Call::~Call() {
  if (--gDepth == 0) ResetBudget(LuaHost::Get().lua().lua_state());
}
bool Budget::Call::Exhausted() const { return gExhausted; }

void LuaHost::Init() {
  // Recreate a fresh state on each call so every mission gets a clean global environment
  // (no leftover globals/callbacks from a previously played mission). Callers must release any
  // outstanding sol references (Mission::Reset) before calling this.
  lua_ = sol::state();

  // The standard libraries a mission may use: no io, os or package. math.random draws from a seed each machine
  // picks for itself, and dofile / loadfile / load run code from outside the mission's own files, so they go too;
  // game.rand() is the multiplayer-safe random.
  lua_.open_libraries(
      sol::lib::base,
      sol::lib::string,
      sol::lib::table,
      sol::lib::math);
  lua_["math"]["random"] = sol::lua_nil;
  lua_["math"]["randomseed"] = sol::lua_nil;
  lua_["dofile"] = sol::lua_nil;
  lua_["loadfile"] = sol::lua_nil;
  lua_["load"] = sol::lua_nil;
  lua_.set_exception_handler(&BindingError);
  gUsed = 0;
  gExhausted = false;
  gDepth = 0;
  SetHookEvery(lua_.lua_state(), Budget::kHookEvery);
  lua_register(lua_.lua_state(), "__op2lua_run", &RunScheduled);

  RegisterBindings(lua_);

  // Runtime preamble: the tick-based scheduler (after/every/when), the time helpers
  // (game.minutes/seconds), and the pump that AIProc drives. Tick-based => multiplayer-safe.
  static const char* kPreamble = R"LUA(
-- The scheduler's own: each after/every/when function runs with a budget of its own (__op2lua_run, LuaHost.cpp, which
-- logs a dropped one). Kept as a local so a mission can't call or replace it.
local _run = __op2lua_run
__op2lua_run = nil
local _timers, _whens = {}, {}
-- Timers / conditions registered *during* a pump (i.e. from inside a firing callback - e.g. an
-- after() that schedules the next after()) are buffered here and merged in once the pump finishes.
-- Without this, the pump's wholesale list rebuild at the end would discard them, so a fired step
-- could never schedule the next step. _pumping selects which list new registrations land in.
local _addTimers, _addWhens = {}, {}
local _pumping = false
local function _pushTimer(e) local t = _pumping and _addTimers or _timers;  t[#t + 1] = e;  return e end
local function _pushWhen(w)  local t = _pumping and _addWhens  or _whens;   t[#t + 1] = w;  return w end

-- Logging. Three channels, each to its own file (all stamped with time + game mark):
--   print(...) / log.mission(...) -> the Mission log (OP2Lua-<name>.log): mission events.
--   log.ai(...)                   -> the AI log (OP2Lua-<name>-AI.log): a scripted AI's debug stream,
--                                    plus the in-game comms panel when log.show_ai_in_game(true).
--   log.show_ai_in_game(on)       -> toggle echoing log.ai lines to the comms panel (off by default).
-- (The framework's own Main log, OP2Lua.log, records mission load/end/crashes - not written from Lua.)
local function _concat(...)
  local p = {}
  for i = 1, select('#', ...) do p[i] = tostring(select(i, ...)) end
  return table.concat(p, '\t')
end
print = function(...) __op2lua_log(_concat(...)) end
log = {
  mission         = function(...) __op2lua_log(_concat(...)) end,
  ai              = function(...) __op2lua_log_ai(_concat(...)) end,
  show_ai_in_game = function(on)  __op2lua_ai_to_comms(on and true or false) end,
  -- log.ai_status(player): overwrite the AI status snapshot file (OP2Lua-<name>-AI-status.json) with
  -- the player's current resources + unit counts. Call from every(marks(1), ...) for a live feed.
  ai_status       = function(player) __op2lua_ai_status(player) end,
}

function after(ticks, fn)
  return _pushTimer({ at = game.tick() + ticks, fn = fn })
end

function every(ticks, fn)
  local e = { at = game.tick() + ticks, every = ticks, fn = fn, cancelled = false }
  _pushTimer(e)
  return { cancel = function() e.cancelled = true end }
end

function when(pred, fn)
  _pushWhen({ pred = pred, fn = fn })
end

-- Absolute scheduling: fire once when the game reaches a given tick / mark (vs after()'s relative).
function at(tick, fn) after(tick - game.tick(), fn) end
function at_mark(m, fn) at(m * 100, fn) end

-- Game time is measured in MARKS (the unit shown in-game; 1 mark = 100 ticks). Prefer marks for ALL
-- scheduling: at_mark(m, fn) fires at absolute mark m; marks(n) converts n marks to ticks for the
-- relative/repeating timers, e.g. after(marks(3), fn) or every(marks(120), fn).
function marks(n) return n * 100 end
function game.mark() return game.tick() // 100 end

-- Legacy time helpers (DEPRECATED - use marks()/at_mark instead; kept for backward compatibility).
function game.minutes(n) return n * game.ticks_per_minute end
function game.seconds(n) return n * game.ticks_per_second end

-- Run fn, reporting (never raising) any error. The log call is itself guarded so a broken
-- print() can never abort the scheduler. Returns true when the instruction limit dropped it (logged by _run); a
-- repeating one (every, when) is then cancelled.
local function safe(fn, kind)
  local ok, err, out = _run(fn, kind)
  if not ok and not out then pcall(function() print('[mission error] ' .. tostring(err)) end) end
  return out
end

-- Driven once per AIProc tick. Deterministic iteration order (arrays) => multiplayer-safe.
function __op2lua_pump()
  local now = game.tick()
  _pumping = true
  local keep = {}
  for _, e in ipairs(_timers) do
    if not e.cancelled then
      if now >= e.at then
        local out = safe(e.fn, e.every and 'every() function' or 'after() function')
        if out then e.cancelled = true end
        if e.every and not e.cancelled then e.at = e.at + e.every; keep[#keep + 1] = e end
      else
        keep[#keep + 1] = e
      end
    end
  end
  _timers = keep

  local wkeep = {}
  for _, w in ipairs(_whens) do
    local ok, fired, out = _run(w.pred, 'when() condition')
    if out then -- dropped: the condition is removed
    elseif ok and fired then safe(w.fn, 'when() function')
    else wkeep[#wkeep + 1] = w end
  end
  _whens = wkeep

  -- Merge anything a callback registered during this pump (after both loops, so a when-callback can
  -- schedule a timer and have it live this same tick), then reset the buffers.
  for _, e in ipairs(_addTimers) do _timers[#_timers + 1] = e end
  for _, w in ipairs(_addWhens) do _whens[#_whens + 1] = w end
  _addTimers, _addWhens, _pumping = {}, {}, false
end
)LUA";
  sol::protected_function_result r = lua_.safe_script(kPreamble, &sol::script_pass_on_error);
  if (!r.valid()) {
    sol::error e = r;
    LOG(ERROR) << "Failed to load OP2Lua runtime preamble: " << e.what();
  }

  initialized_ = true;
  LOG(INFO) << "Lua host initialized (" << LUA_RELEASE << ")";
}

void LuaHost::Shutdown() {
  if (!initialized_) return;
  Mission::Reset();     // release mission callback refs while the state is still alive
  lua_ = sol::state();  // destroy and replace with a fresh empty state
  initialized_ = false;
  LOG(INFO) << "Lua host shut down";
}

bool LuaHost::RunFile(const std::string& path) {
  Guard::Scope guard;   // the script's top level may call the game too
  Budget::Call budget;
  sol::protected_function_result result = lua_.safe_script_file(
      path, &sol::script_pass_on_error);

  if (!result.valid()) {
    sol::error err = result;
    std::string msg = "Error running ";
    msg += path;
    msg += ":\n";
    msg += err.what();
    LOG(ERROR) << msg;
    MessageBoxA(nullptr, msg.c_str(), "OP2Lua", MB_ICONEXCLAMATION | MB_SYSTEMMODAL);
    return false;
  }
  return true;
}

}  // namespace OP2Lua
