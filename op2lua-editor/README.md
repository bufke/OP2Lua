# OP2LuaEditor

A visual editor for building **OP2Lua** mission layouts for *Outpost 2: Divided Destiny*.

## What is it?

OP2LuaEditor lets you author a mission's starting layout without hand-writing Lua. Load an
Outpost 2 map as the backdrop, place units, draw named regions and markers, set the player and
mission properties, and save it straight to a `placement.lua`. It also scaffolds new missions -
cloning and byte-patching the stub DLL and naming it - so you can go from blank to a playable
mission shell without touching a compiler. You then write the mission logic in `<Name>.lua`.

It pairs with the **OP2Lua** runtime: the editor produces the `placement.lua` (the layout), you
write the `<Name>.lua` (the logic), and `OP2LuaCore.dll` runs them in-game.

## Features

- **Open / load a map** as the editing backdrop (reads OP2 1.4.1 tile graphics from the well BMPs).
- **Place units** for each player (1-6) and **draw regions**, round-tripped to/from `placement.lua`. Saving
  rewrites only what you changed: every other field, comment and line of the file stays as it was.
- **Beacons, walls, tubes and markers** from `placement.lua` are drawn on the map for context.
- **Mission properties** - name, map, tech, mission type (Colony, Tutorial, AutoDemo or a multiplayer type),
  players, max tech level; the mission DLL is patched to match.
- **New Mission** scaffolding - clones the stub DLL and patches its name, map, type and player count, writes
  skeleton scripts (a multiplayer type gets a lobby-driven skeleton).
- **Open a mission** from File > Open Mission, the command line, or by dropping its `.lua` on the map.
- Coordinates match the in-game status bar (what you place is where it lands).

## Requirements

- An **Outpost 2 (OPU 1.4.1)** install - the editor loads tile graphics from the game's well files.
- **.NET Framework 4.8**.

Built in VB.NET (WinForms); uses **OP2UtilityDotNet** to read map files.
