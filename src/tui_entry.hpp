/******************************************************************************\
* Copyright (c) 2019-2026, Robert van Engelen, Genivia Inc. All rights reserved.*
* BSD-3 License - see LICENSE.txt                                              *
\******************************************************************************/

/**
@file      tui_entry.hpp
@brief     the only entry points of the query TUI (component src_tui) that the search core (component src_core) calls

The search core (src/ugrep.cpp and friends) used to include query.hpp, which includes ugrep.hpp: an include cycle between
the two components. The core now includes only this header. It declares plain functions and no types, so the core does
not depend on the TUI class layouts. The TUI depends on the core through ugrep.hpp, one way. The definitions are at the
end of src/query.cpp and src/screen.cpp.
*/

#ifndef TUI_ENTRY_HPP
#define TUI_ENTRY_HPP

#include <cstddef>

// default -Q response to keyboard input delay is 3 for 300ms, in steps of 100ms (was in query.hpp)
#ifndef DEFAULT_QUERY_DELAY
#define DEFAULT_QUERY_DELAY 3
#endif

// -Q: run the interactive query UI, same as Query::query()
extern void tui_query();

// the width of the screen in columns, same as Screen::getsize()
extern size_t tui_screen_getsize();

#endif
