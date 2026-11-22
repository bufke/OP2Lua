/* persist_lua.c - the few things the saved-game code (Persist.cpp) needs from inside Lua that its C API doesn't give:
 * a Lua function's prototype, a chunk's prototypes in a fixed order, and a new closure of a given prototype. A saved
 * game names each of the script's functions by its prototype's place in the script, not by its bytecode, so a save
 * never carries code. Built with the Lua sources (C, same headers and flags). */

#define LUA_CORE

#include "lprefix.h"

#include "lua.h"

#include "lapi.h"
#include "lfunc.h"
#include "lgc.h"
#include "lobject.h"
#include "lstate.h"

#include "persist_lua.h"

const void* op2lua_function_proto(lua_State* L, int idx) {
  StkId at;
  if (lua_type(L, idx) != LUA_TFUNCTION || lua_iscfunction(L, idx)) return NULL;
  at = (idx > 0) ? L->ci->func.p + idx : L->top.p + idx;   /* a stack index, as the callers pass */
  return clLvalue(s2v(at))->p;
}

/* Depth-first, parent before children, children in their order in the parent: the same order every time the same
 * source is compiled. */
static int count_protos(const Proto* p) {
  int n = 1, i;
  for (i = 0; i < p->sizep; i++) n += count_protos(p->p[i]);
  return n;
}

static void list_protos(const Proto* p, const void** out, int* at) {
  int i;
  out[(*at)++] = p;
  for (i = 0; i < p->sizep; i++) list_protos(p->p[i], out, at);
}

int op2lua_proto_list(const void* proto, const void** out, int max) {
  const Proto* p = (const Proto*)proto;
  int n, at = 0;
  if (p == NULL) return 0;
  n = count_protos(p);
  if (out == NULL || max < n) return n;
  list_protos(p, out, &at);
  return n;
}

int op2lua_proto_upvalues(const void* proto) { return proto ? ((const Proto*)proto)->sizeupvalues : 0; }

void op2lua_push_closure(lua_State* L, const void* proto) {
  Proto* p = (Proto*)proto;
  LClosure* cl;
  lua_lock(L);
  cl = luaF_newLclosure(L, p->sizeupvalues);
  cl->p = p;
  setclLvalue2s(L, L->top.p, cl);
  api_incr_top(L);
  luaF_initupvals(L, cl);   /* fresh closed upvalues, nil; the caller sets or joins them */
  luaC_checkGC(L);
  lua_unlock(L);
}
