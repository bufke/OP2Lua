#pragma once

/* Lua internals for saved games (persist_lua.c). Prototypes are opaque pointers here. */

#include "lua.h"

#ifdef __cplusplus
extern "C" {
#endif

/* The prototype of the Lua function at stack index `idx` (not a pseudo-index), or NULL for anything else. */
const void* op2lua_function_proto(lua_State* L, int idx);

/* The prototypes of a chunk, its main one first, depth-first. Returns how many there are; fills `out` only when it
 * has room for all of them. */
int op2lua_proto_list(const void* proto, const void** out, int max);

/* How many upvalues a closure of `proto` has. */
int op2lua_proto_upvalues(const void* proto);

/* Pushes a new closure of `proto`, its upvalues fresh and nil. */
void op2lua_push_closure(lua_State* L, const void* proto);

#ifdef __cplusplus
}
#endif
